//! Launcher-only update actions. Closing a session never triggers installation by itself.

use super::{MenuAction, MenuRuntime, MenuScreen};

impl MenuRuntime {
    /// Routes update controls while refusing restart during play or a pending connection.
    pub(super) fn activate_update(&mut self, action: MenuAction) {
        use crate::lifecycle::update;
        match action {
            MenuAction::UpdateRestart => {
                let idle = self.launcher
                    && self.visible
                    && self.screen == MenuScreen::Home
                    && !self.over_world()
                    && !self.connecting
                    && self.pending_connect.is_none();
                if update::request_restart(idle) {
                    self.exit_requested = true;
                }
            }
            MenuAction::UpdateRetry => update::retry(),
            MenuAction::UpdateNotes => update::open_notes(),
            MenuAction::UpdateToggle => update::toggle(),
            _ => {}
        }
    }
}
