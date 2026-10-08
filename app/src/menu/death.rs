//! Death-route state and its one-shot respawn request.

use super::{MenuAction, MenuRuntime, MenuScreen};

impl MenuRuntime {
    /// Opens one death route, replacing a world menu; returns false if already shown.
    pub(crate) fn open_death(&mut self) -> bool {
        if self.is_connecting() || self.death_shown || self.visible && !self.over_world() {
            return false;
        }
        self.key_remap = None;
        self.death_shown = true;
        self.death_loading = false;
        self.dialog = None;
        self.history.reset(MenuScreen::Death);
        self.show_top();
        true
    }

    /// Recovery permits a later death and closes the retained loading screen.
    pub(crate) fn note_player_alive(&mut self) {
        self.reset_death();
        if self.screen == MenuScreen::Death && self.visible {
            self.set_visible(false);
            self.history.reset(MenuScreen::Home);
            self.screen = MenuScreen::Home;
        }
    }

    /// Attempts one pending respawn request; returns whether the queue accepted it.
    pub(crate) fn send_respawn_request(&mut self, send: impl FnOnce() -> bool) -> bool {
        if !self.intents.respawn || !send() {
            return false;
        }
        self.intents.respawn = false;
        true
    }

    /// Opens the route and starts the server-requested immediate respawn once.
    pub(crate) fn open_death_with_rules(&mut self, immediate_respawn: bool) {
        if self.open_death() && immediate_respawn {
            self.activate(MenuAction::Respawn);
        }
    }

    /// Retires presentation and unsent requests when the session ends or recovers.
    pub(super) fn reset_death(&mut self) {
        self.death_shown = false;
        self.death_loading = false;
        self.intents.respawn = false;
    }
}
