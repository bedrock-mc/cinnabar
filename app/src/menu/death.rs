//! Death-route state and its one-shot respawn request.

use super::{MenuAction, MenuRuntime, MenuScreen};

impl MenuRuntime {
    /// Shows one death route, replacing a world menu when health reaches zero.
    pub(crate) fn open_death(&mut self) {
        if self.is_connecting() || self.death_shown || self.visible && !self.over_world() {
            return;
        }
        self.death_shown = true;
        self.death_loading = false;
        self.dialog = None;
        self.history.reset(MenuScreen::Death);
        self.show_top();
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

    /// Returns the death screen's respawn press once for the session driver.
    pub(crate) fn take_respawn_request(&mut self) -> bool {
        std::mem::take(&mut self.intents.respawn)
    }

    /// Opens the route and starts the server-requested immediate respawn once.
    pub(crate) fn open_death_with_rules(&mut self, immediate_respawn: bool) {
        self.open_death();
        if immediate_respawn && self.screen == MenuScreen::Death && self.visible {
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
