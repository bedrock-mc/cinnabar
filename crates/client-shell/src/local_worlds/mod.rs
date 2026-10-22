//! Local single-player worlds on BDS or the built-in Dragonfly server, run by the launcher.
//!
//! [`WorldsMenu`] is a pure screen model; [`LocalWorlds`] wires it to the world manager and tells
//! the attached core where the open world is. The menu module embeds it by calling `attach`,
//! `input`, `menu` and `take_ready`.

use launcher::menu::settings_options::SettingId;
mod client;

use launcher::local_worlds::model;
#[cfg(test)]
mod settings_storage_flow_tests;

use std::{collections::HashMap, io, path::PathBuf, sync::Arc};

use launcher::install_layout::InstallLayout;
use launcher_host::local_worlds::Manager;

use bevy::{
    prelude::{App, MessageReader, Plugin, Res, ResMut, Resource, Update},
    window::WindowFocused,
};

use launcher::local_worlds::world::WorldThumbnail;
use model::{Effect, Event, Input, Screen, WorldsMenu};

use client::WorldsClient;

/// Shown for an import with no attached worker; one cut off by a detach never publishes.
const IMPORT_UNAVAILABLE: &str = "Local worlds are not ready; try the import again in a moment.";

/// Menu model plus the world manager and the worker that serves it.
#[derive(Resource)]
pub struct LocalWorlds {
    menu: WorldsMenu,
    /// Opened on the first attach and kept across core restarts.
    manager: Option<Manager>,
    client: Option<WorldsClient>,
    playing: bool,
    focused: bool,
    pause_menu: bool,
    pause_on_unfocus: bool,
    /// The pause last sent to the world.
    paused: bool,
    /// Saved worlds' menu thumbnails by id, read as the list arrives.
    thumbnails: HashMap<String, WorldThumbnail>,
}

impl Default for LocalWorlds {
    fn default() -> Self {
        Self {
            menu: WorldsMenu::default(),
            manager: None,
            client: None,
            playing: false,
            focused: true,
            pause_menu: false,
            pause_on_unfocus: true,
            paused: false,
            thumbnails: HashMap::new(),
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
    /// Opens the install's worlds, routes the core at `socket_dir` to the open one and requests
    /// the world list.
    pub fn attach(&mut self, socket_dir: PathBuf, layout: &InstallLayout) -> io::Result<()> {
        let manager = match &self.manager {
            Some(manager) => manager.clone(),
            None => {
                let manager = launcher_host::local_worlds::open(layout)
                    .map_err(|error| io::Error::other(error.to_string()))?;
                self.manager = Some(manager.clone());
                manager
            }
        };
        if let Some(previous) = self.client.take() {
            previous.revoke();
        }
        self.client = Some(WorldsClient::spawn(manager, socket_dir)?);
        self.playing = false;
        self.menu = WorldsMenu::default();
        self.input(Input::Refresh);
        self.dispatch(vec![Effect::LoadPrefs]);
        Ok(())
    }

    /// Drops the worker and closes the open world, whose route left with the old core.
    pub fn detach(&mut self) {
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

    /// Leaves the local world: its server saves and stops.
    pub fn leave_world(&mut self) {
        self.playing = false;
        self.paused = false;
        self.dispatch(vec![Effect::Close]);
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

/// Applies the desktop focus preference before forwarding focus changes to the local world.
fn pause_on_focus(
    mut focus: MessageReader<WindowFocused>,
    mut worlds: ResMut<LocalWorlds>,
    menu: Option<Res<crate::menu::MenuRuntime>>,
    desktop_focus: Option<Res<client_state::CursorFocus>>,
    driven: Option<Res<client_state::DrivenInput>>,
) {
    if let Some(menu) = menu {
        worlds.pause_on_unfocus = menu
            .settings_snapshot()
            .0
            .value(SettingId::PauseMenuOnFocusLost)
            != 0;
        worlds.sync_pause();
    }
    if let Some(desktop_focus) = desktop_focus {
        focus.clear();
        worlds.focus_changed(driven.is_some() || desktop_focus.available());
    } else {
        for message in focus.read() {
            worlds.focus_changed(message.focused);
        }
    }
}

/// Registers [`LocalWorlds`] (inert until attached) and its per-frame systems.
pub struct LocalWorldsPlugin;

impl Plugin for LocalWorldsPlugin {
    fn build(&self, app: &mut App) {
        app.add_message::<WindowFocused>()
            .init_resource::<LocalWorlds>()
            .add_systems(Update, (pump_local_worlds, pause_on_focus));
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
