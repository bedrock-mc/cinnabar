//! Local worlds choose their server and terrain independently through the core.
//!
//! [`WorldsMenu`] is a pure screen model; [`LocalWorlds`] wires it to the core's control
//! channel. The menu module embeds it by calling `attach`, `input`, `menu` and `take_ready`.

mod client;
use launcher::local_worlds::form;
mod launch;
use launcher::local_worlds::model;
use launcher::local_worlds::progress;
use launcher::local_worlds::prompt;
#[cfg(test)]
mod settings_storage_flow_tests;

use std::{io, path::PathBuf};

use bevy::{
    prelude::{App, MessageReader, Plugin, Res, ResMut, Resource, Update},
    window::WindowFocused,
};

pub(crate) use form::{
    FLAT_WORLD_LABEL, MAX_SEED_CHARS, MAX_WORLD_NAME_CHARS, NORMAL_WORLD_LABEL,
    difficulty_description, difficulty_label, game_mode_description, game_mode_label,
    world_type_label,
};
pub(crate) use launch::core_args;
pub(crate) use model::{Effect, Event, Input, Screen, Tab, WorldsMenu, WorldsView};
pub(crate) use progress::{Progress, Stage};
pub(crate) use prompt::{Prompt, PromptButton, PromptFor};

use client::WorldsClient;

/// Menu model plus the control-channel worker that serves it.
#[derive(Resource)]
pub(crate) struct LocalWorlds {
    menu: WorldsMenu,
    client: Option<WorldsClient>,
    playing: bool,
    focused: bool,
    pause_menu: bool,
    pause_on_unfocus: bool,
    /// The pause last sent to the core.
    paused: bool,
}

impl Default for LocalWorlds {
    fn default() -> Self {
        Self {
            menu: WorldsMenu::default(),
            client: None,
            playing: false,
            focused: true,
            pause_menu: false,
            pause_on_unfocus: true,
            paused: false,
        }
    }
}

impl LocalWorlds {
    /// Connects to the core at `socket_dir` and requests the world list.
    pub(crate) fn attach(&mut self, socket_dir: PathBuf) -> io::Result<()> {
        self.client = Some(WorldsClient::spawn(socket_dir)?);
        self.playing = false;
        self.menu = WorldsMenu::default();
        self.input(Input::Refresh);
        self.dispatch(vec![Effect::LoadPrefs]);
        Ok(())
    }

    /// Drops the worker; the core keeps the open world until [`Self::leave_world`].
    pub(crate) fn detach(&mut self) {
        self.client = None;
        self.playing = false;
    }

    pub(crate) fn menu(&self) -> &WorldsMenu {
        &self.menu
    }

    pub(crate) fn input(&mut self, input: Input) {
        let effects = self.menu.update(input);
        self.dispatch(effects);
    }

    /// Applies finished control-channel requests; call once per frame.
    pub(crate) fn pump(&mut self) {
        let Some(client) = &self.client else { return };
        for event in client.drain() {
            let effects = self.menu.apply(event);
            self.dispatch(effects);
        }
    }

    /// Takes the id of a world that finished opening; the caller then joins the game socket.
    pub(crate) fn take_ready(&mut self) -> Option<String> {
        self.menu.take_ready()
    }

    /// Records whether the player is in the local world; only then does the world pause.
    pub(crate) fn set_playing(&mut self, playing: bool) {
        self.playing = playing;
        self.sync_pause();
    }

    /// Leaves the local world: the core saves and stops it.
    pub(crate) fn leave_world(&mut self) {
        self.playing = false;
        self.paused = false;
        self.dispatch(vec![Effect::Close]);
    }

    /// The pause menu is open over the world; single-player pauses the game behind it.
    pub(crate) fn set_pause_menu(&mut self, open: bool) {
        self.pause_menu = open;
        self.sync_pause();
    }

    /// Pauses the world when the window loses focus and resumes it on regain.
    pub(crate) fn focus_changed(&mut self, focused: bool) {
        self.focused = focused;
        self.sync_pause();
    }

    /// Sends a pause change when playing and the wanted state moved; the core ignores it for BDS.
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
                crate::desktop::open_url(url);
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
    desktop_focus: Option<Res<client_presentation::camera::CursorFocus>>,
    driven: Option<Res<crate::camera::DrivenInput>>,
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
pub(crate) struct LocalWorldsPlugin;

impl Plugin for LocalWorldsPlugin {
    fn build(&self, app: &mut App) {
        app.init_resource::<LocalWorlds>()
            .add_systems(Update, (pump_local_worlds, pause_on_focus));
    }
}

#[cfg(test)]
mod tests {
    use super::*;

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
