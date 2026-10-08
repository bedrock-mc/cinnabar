//! Death-route state and its one-shot respawn request.

use super::{MenuRuntime, MenuScreen};

/// Real-time delay before ordinary death controls become visible and accept input.
pub(super) const DEATH_CONTROLS_DELAY_SECONDS: f64 = 1.2_f32 as f64;

impl MenuRuntime {
    /// Opens one death route, replacing a world menu; returns false if already shown.
    pub(crate) fn open_death(&mut self) -> bool {
        if self.is_connecting() || self.death_shown || self.visible && !self.over_world() {
            return false;
        }
        self.key_remap = None;
        self.death_shown = true;
        self.death_loading = false;
        self.death_controls_remaining = DEATH_CONTROLS_DELAY_SECONDS;
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

    /// Advances the control reveal on real time, independently of simulation pauses.
    pub(super) fn advance_death_controls(&mut self, delta_seconds: f64) {
        if self.death_shown && delta_seconds.is_finite() && delta_seconds > 0.0 {
            self.death_controls_remaining =
                (self.death_controls_remaining - delta_seconds).max(0.0);
        }
    }

    /// Reports whether a visible death route can accept a button action.
    pub(super) fn death_controls_ready(&self) -> bool {
        self.visible
            && self.screen == MenuScreen::Death
            && self.death_shown
            && !self.death_loading
            && self.death_controls_remaining == 0.0
    }

    /// Retains the loading route and queues one request, including immediate respawn.
    pub(super) fn request_respawn(&mut self) {
        if self.screen == MenuScreen::Death && self.death_shown && !self.death_loading {
            self.intents.respawn = true;
            self.death_loading = true;
            self.dialog = None;
        }
    }

    /// Opens the route and starts the server-requested immediate respawn once.
    pub(crate) fn open_death_with_rules(&mut self, immediate_respawn: bool) -> bool {
        if !self.open_death() {
            return false;
        }
        if immediate_respawn {
            self.request_respawn();
        }
        true
    }

    /// Retires presentation and unsent requests when the session ends or recovers.
    pub(super) fn reset_death(&mut self) {
        self.death_shown = false;
        self.death_loading = false;
        self.death_controls_remaining = 0.0;
        self.intents.respawn = false;
    }
}
