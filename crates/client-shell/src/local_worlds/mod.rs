//! Local single-player worlds on BDS or the built-in Dragonfly server, run by the launcher.
//!
//! [`WorldsMenu`] is a pure screen model; [`LocalWorlds`] wires it to the world manager and tells
//! the original native session which opening to retain. The menu module embeds it through native attachment,
//! `input`, `menu` and `take_ready`.

mod client;
mod focus;
#[cfg(all(feature = "native-networking", not(target_arch = "wasm32")))]
mod native;

use launcher::local_worlds::model;
#[cfg(test)]
mod settings_storage_flow_tests;

#[cfg(feature = "native-networking")]
use std::io;
use std::{collections::HashMap, sync::Arc};

#[cfg(feature = "native-networking")]
use launcher::install_layout::InstallLayout;
use launcher_host::local_worlds::{Host, Manager};

use bevy::{
    prelude::{App, Plugin, ResMut, Resource, Update},
    window::WindowFocused,
};

use launcher::local_worlds::world::WorldThumbnail;
use model::{Effect, Event, Input, Screen, WorldsMenu};

#[cfg(all(feature = "native-networking", not(target_arch = "wasm32")))]
pub use client::NativeLocalWorldRetirement;
use client::WorldsClient;

/// Shown for an import with no attached worker; one cut off by a detach never publishes.
const IMPORT_UNAVAILABLE: &str = "Local worlds are not ready; try the import again in a moment.";

/// Menu model plus the world manager and the worker that serves it.
#[derive(Resource)]
pub struct LocalWorlds {
    menu: WorldsMenu,
    /// Opened on the first attach and kept across account attachment changes.
    manager: Option<Manager>,
    client: Option<WorldsClient>,
    #[cfg(all(feature = "native-networking", not(target_arch = "wasm32")))]
    native_playing: Option<launcher_host::local_worlds::NativeLocalWorldSelection>,
    playing: bool,
    focused: bool,
    pause_menu: bool,
    pause_on_unfocus: bool,
    /// The pause last sent to the world.
    paused: bool,
    /// Saved worlds' menu thumbnails by id, read as the list arrives.
    thumbnails: HashMap<String, WorldThumbnail>,
    /// The signed-in account, kept for a manager attached later.
    host: Host,
}

impl Default for LocalWorlds {
    fn default() -> Self {
        Self {
            menu: WorldsMenu::default(),
            manager: None,
            client: None,
            #[cfg(all(feature = "native-networking", not(target_arch = "wasm32")))]
            native_playing: None,
            playing: false,
            focused: true,
            pause_menu: false,
            pause_on_unfocus: true,
            paused: false,
            thumbnails: HashMap::new(),
            host: Host::SignedOut,
        }
    }
}

/// Asks the open world's server to save and stop as the client exits.
impl Drop for LocalWorlds {
    fn drop(&mut self) {
        self.detach();
    }
}

impl LocalWorlds {
    /// Preserves the existing manager across account attachment changes.
    #[cfg(feature = "native-networking")]
    fn attach_manager(&mut self, layout: &InstallLayout) -> io::Result<Manager> {
        let manager = match &self.manager {
            Some(manager) => manager.clone(),
            None => {
                let manager = launcher_host::local_worlds::open(layout)
                    .map_err(|error| io::Error::other(error.to_string()))?;
                manager.set_host(self.host.clone());
                self.manager = Some(manager.clone());
                manager
            }
        };
        if let Some(previous) = self.client.take() {
            previous.revoke();
        }
        Ok(manager)
    }

    /// Drops the worker and closes the open world, whose original session has retired.
    pub fn detach(&mut self) {
        #[cfg(all(feature = "native-networking", not(target_arch = "wasm32")))]
        {
            self.native_playing = None;
        }
        // Revoked first, so a request the worker already took cannot reopen the world.
        if let Some(client) = self.client.take() {
            client.revoke();
        }
        self.end_import();
        self.playing = false;
        if let Some(manager) = &self.manager {
            manager.close();
        }
    }

    /// Names the account that later world starts make their operator.
    pub fn set_host(&mut self, host: Host) {
        if self.host == host {
            return;
        }
        if let Some(manager) = &self.manager {
            manager.set_host(host.clone());
        }
        self.host = host;
    }

    pub fn menu(&self) -> &WorldsMenu {
        &self.menu
    }

    pub fn input(&mut self, input: Input) {
        let effects = self.menu.update(input);
        self.dispatch(effects);
        if self.client.is_none() {
            self.end_import();
        }
    }

    /// Fails an import no worker will answer, so its modal never stays up.
    fn end_import(&mut self) {
        if self.menu.screen() == Screen::Importing {
            let effects = self.menu.apply(Event::Imported {
                worlds: Vec::new(),
                errors: vec![IMPORT_UNAVAILABLE.to_owned()],
            });
            self.dispatch(effects);
        }
    }

    /// Applies finished world requests; call once per frame.
    pub fn pump(&mut self) {
        let Some(client) = &self.client else { return };
        for reply in client.drain() {
            for (id, thumbnail) in reply.thumbnails {
                self.thumbnails.entry(id).or_insert(thumbnail);
            }
            if let Event::Deleted(id) = &reply.event {
                self.thumbnails.remove(id);
            }
            let effects = self.menu.apply(reply.event);
            self.dispatch(effects);
        }
    }

    /// World `id`'s menu thumbnail, once one was captured.
    #[must_use]
    pub fn thumbnail(&self, id: &str) -> Option<WorldThumbnail> {
        self.thumbnails.get(id).cloned()
    }

    /// Keeps `image` as world `id`'s thumbnail and saves it beside the world.
    pub fn save_thumbnail(&mut self, id: &str, image: Vec<u8>) {
        let image = Arc::from(image);
        if let Some(client) = &self.client {
            client.save_thumbnail(id.to_owned(), Arc::clone(&image));
        }
        self.thumbnails
            .insert(id.to_owned(), WorldThumbnail::new(id, image));
    }

    /// Takes the id of a world that finished opening; the caller then joins the session endpoint.
    pub fn take_ready(&mut self) -> Option<String> {
        self.menu.take_ready()
    }

    /// Records whether the player is in the local world; only then does the world pause.
    pub fn set_playing(&mut self, playing: bool) {
        self.playing = playing;
        self.sync_pause();
    }

    /// Readies a local server while the worlds are on screen, so an open skips its startup work.
    pub fn prepare(&self) {
        self.dispatch(vec![Effect::Prepare]);
    }

    /// Leaves the local world: its server saves and stops, or briefly keeps running where
    /// starts are slow.
    pub fn leave_world(&mut self) {
        self.playing = false;
        self.paused = false;
        #[cfg(all(feature = "native-networking", not(target_arch = "wasm32")))]
        if let Some(client) = self.client.as_ref() {
            if let Some(selection) = self.native_playing.take() {
                client.leave_native(selection);
            }
            return;
        }
        self.dispatch(vec![Effect::Leave]);
        if self.client.is_some() {
            self.input(Input::Refresh);
        }
    }

    /// The pause menu is open over the world; single-player pauses the game behind it.
    pub fn set_pause_menu(&mut self, open: bool) {
        self.pause_menu = open;
        self.sync_pause();
    }

    /// Pauses the world when the window loses focus and resumes it on regain.
    pub fn focus_changed(&mut self, focused: bool) {
        self.focused = focused;
        self.sync_pause();
    }

    /// Sends a pause change when playing and the wanted state moved; BDS ignores it.
    fn sync_pause(&mut self) {
        let wanted = self.playing && (self.pause_menu || (self.pause_on_unfocus && !self.focused));
        if wanted != self.paused {
            self.paused = wanted;
            self.dispatch(vec![Effect::SetPaused(wanted)]);
        }
    }

    fn dispatch(&self, effects: Vec<Effect>) {
        for effect in effects {
            if let Effect::OpenUrl(url) = effect {
                launcher_host::desktop::open_url(url);
            } else if let Some(client) = &self.client {
                client.send(effect);
            }
        }
    }
}

fn pump_local_worlds(mut worlds: ResMut<LocalWorlds>) {
    worlds.pump();
}

/// Registers [`LocalWorlds`] (inert until attached) and its per-frame systems.
pub struct LocalWorldsPlugin;

impl Plugin for LocalWorldsPlugin {
    fn build(&self, app: &mut App) {
        app.add_message::<WindowFocused>()
            .init_resource::<LocalWorlds>()
            .add_systems(Update, (pump_local_worlds, focus::pause_on_focus));
    }
}

#[cfg(test)]
impl LocalWorlds {
    /// Delivers a core event without a core, dropping the effects it would send back.
    pub(crate) fn apply(&mut self, event: model::Event) {
        self.menu.apply(event);
    }
}

#[cfg(test)]
mod tests;
