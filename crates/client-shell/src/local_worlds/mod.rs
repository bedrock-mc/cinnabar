//! Local single-player worlds on BDS or the built-in Dragonfly server, run by the launcher.
//!
//! [`WorldsMenu`] is a pure screen model; [`LocalWorlds`] wires it to the world manager and tells
//! the attached core where the open world is. The menu module embeds it by calling `attach`,
//! `input`, `menu` and `take_ready`.

mod client;

use launcher::local_worlds::model;
#[cfg(test)]
mod settings_storage_flow_tests;

use std::{io, path::PathBuf};

use launcher::install_layout::InstallLayout;
use launcher_host::local_worlds::Manager;

use bevy::{
    prelude::{App, MessageReader, Plugin, Res, ResMut, Resource, Update},
    window::WindowFocused,
};

use model::{Effect, Input, WorldsMenu};

use client::WorldsClient;

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
        }
    }
}

/// Asks the open world's server to save and stop as the client exits.
impl Drop for LocalWorlds {
    fn drop(&mut self) {
        if let Some(manager) = &self.manager {
            manager.close();
        }
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
        self.client = Some(WorldsClient::spawn(manager, socket_dir)?);
        self.playing = false;
        self.menu = WorldsMenu::default();
        self.input(Input::Refresh);
        self.dispatch(vec![Effect::LoadPrefs]);
        Ok(())
    }

    /// Drops the worker and closes the open world, whose route left with the old core.
    pub fn detach(&mut self) {
        self.client = None;
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
    }

    /// Applies finished world requests; call once per frame.
    pub fn pump(&mut self) {
        let Some(client) = &self.client else { return };
        for event in client.drain() {
            let effects = self.menu.apply(event);
            self.dispatch(effects);
        }
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

    /// Leaves the local world: its server saves and stops.
    pub fn leave_world(&mut self) {
        self.playing = false;
        self.paused = false;
        self.dispatch(vec![Effect::Close]);
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
        worlds.pause_on_unfocus = menu.settings_snapshot().0.value("pause_menu_on_focus_lost") != 0;
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
mod tests {
    use {
        super::*,
        launcher::local_worlds::model::{Input, Screen},
    };

    #[test]
    fn focus_only_pauses_while_playing_and_detached_is_inert() {
        let mut worlds = LocalWorlds::default();
        worlds.focus_changed(false);
        worlds.pump();
        assert!(!worlds.paused, "not in a world");
        worlds.set_playing(true);
        assert!(worlds.paused, "joined while unfocused");
        worlds.focus_changed(true);
        assert!(!worlds.paused);
        worlds.leave_world();
        assert!(!worlds.playing);
    }

    /// The pause menu pauses a single-player world and closing it resumes, as in vanilla.
    #[test]
    fn the_pause_menu_pauses_and_resumes_the_world() {
        let mut worlds = LocalWorlds::default();
        worlds.set_pause_menu(true);
        assert!(!worlds.paused, "online play never pauses");
        worlds.set_pause_menu(false);
        worlds.set_playing(true);
        worlds.set_pause_menu(true);
        assert!(worlds.paused);
        worlds.focus_changed(false);
        worlds.set_pause_menu(false);
        assert!(worlds.paused, "still unfocused");
        worlds.focus_changed(true);
        assert!(!worlds.paused);
    }

    #[test]
    fn input_without_a_client_still_advances_the_model() {
        let mut worlds = LocalWorlds::default();
        worlds.input(Input::BeginCreate);
        assert_eq!(worlds.menu().screen(), Screen::Create);
        worlds.input(Input::Back);
        worlds.input(Input::Refresh);
        assert!(worlds.menu().busy());
    }
}

#[cfg(test)]
mod settings_tests {
    use super::*;

    /// Disabling focus pause preserves explicit pause-menu control.
    #[test]
    fn focus_preference_does_not_disable_explicit_pause() {
        let mut worlds = LocalWorlds::default();
        worlds.set_playing(true);
        worlds.focus_changed(false);
        assert!(worlds.paused);
        worlds.pause_on_unfocus = false;
        worlds.sync_pause();
        assert!(!worlds.paused);
        worlds.set_pause_menu(true);
        assert!(worlds.paused);
    }
}
