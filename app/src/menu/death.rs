//! Death-route state and respawn requests retained until authoritative recovery.

use {super::MenuRuntime, launcher::menu::MenuScreen};

/// Real-time delay before ordinary death controls become visible and accept input.
#[cfg(test)]
pub(super) const DEATH_CONTROLS_DELAY_SECONDS: f64 = launcher::menu::death::STAGE_SECONDS * 2.0;

impl MenuRuntime {
    /// Opens one death route, replacing a world menu; returns false if already shown.
    pub(crate) fn open_death(&mut self) -> bool {
        if self.is_connecting() || self.death_shown || self.visible && !self.over_world() {
            return false;
        }
        self.key_remap = None;
        self.death_shown = true;
        self.death_loading = false;
        self.death_retry_remaining = None;
        self.death_presentation = launcher::menu::death::DeathPresentation::new(
            self.settings_snapshot().0.value("screen_animations") != 0,
            false,
        );
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

    /// Consumes a pending respawn command only when the outbound queue accepts it.
    pub(crate) fn send_respawn_request(&mut self, send: impl FnOnce() -> bool) -> bool {
        if !self.intents.respawn || !send() {
            return false;
        }
        self.intents.respawn = false;
        true
    }

    /// Advances the control reveal on real time, independently of simulation pauses.
    pub(super) fn advance_death_controls(&mut self, delta_seconds: f64) {
        if self.death_shown {
            self.death_presentation.advance(delta_seconds);
            if let Ok(delta) = std::time::Duration::try_from_secs_f64(delta_seconds)
                && !delta.is_zero()
                && let Some(remaining) = &mut self.death_retry_remaining
            {
                if delta >= *remaining {
                    self.intents.respawn = true;
                    *remaining = std::time::Duration::from_secs_f64(
                        launcher::menu::death::RESPAWN_RETRY_SECONDS,
                    );
                } else {
                    *remaining -= delta;
                }
            }
        }
    }

    /// Reports whether a visible death route can accept a button action.
    pub(super) fn death_controls_ready(&self) -> bool {
        self.visible
            && self.screen == MenuScreen::Death
            && self.death_shown
            && !self.death_loading
            && self.death_presentation.controls_ready()
    }

    /// Retains the loading route and queues one request, including immediate respawn.
    pub(super) fn request_respawn(&mut self) {
        if self.screen == MenuScreen::Death && self.death_shown && !self.death_loading {
            self.intents.respawn = true;
            self.death_loading = true;
            self.death_presentation.respawn_seconds = Some(0.0);
            self.death_retry_remaining = Some(std::time::Duration::from_secs_f64(
                launcher::menu::death::STAGE_SECONDS + launcher::menu::death::RESPAWN_RETRY_SECONDS,
            ));
            self.dialog = None;
        }
    }

    /// Opens the route and starts the server-requested immediate respawn once.
    pub(crate) fn open_death_with_rules(&mut self, immediate_respawn: bool) -> bool {
        if !self.open_death() {
            return false;
        }
        if immediate_respawn {
            self.death_presentation.immediate_respawn = true;
            self.request_respawn();
        }
        true
    }

    /// Retires presentation and unsent requests when the session ends or recovers.
    pub(super) fn reset_death(&mut self) {
        self.death_shown = false;
        self.death_loading = false;
        self.death_presentation = launcher::menu::death::DeathPresentation::default();
        self.death_retry_remaining = None;
        self.intents.respawn = false;
    }
}
