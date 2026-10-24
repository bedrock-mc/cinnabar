//! Retained settings revisions shared by runtime adapters.
use crate::UserSettings;
use semantic_input::ControlSettings;

/// Retained settings handoff used by menus and live subsystem
/// adapters. Every replacement is complete and monotonically versioned.
#[derive(Clone, Debug, Default)]
pub struct SettingsState {
    generation: u64,
    controls_generation: u64,
    user_settings: UserSettings,
}

impl SettingsState {
    /// Write back the fullscreen adapter's already-applied window state without
    /// publishing a complete settings replacement. A window toggle must not
    /// apply unrelated camera or VSync defaults through their generation readers.
    pub fn set_fullscreen(&mut self, fullscreen: bool) {
        self.user_settings.video.fullscreen = fullscreen;
    }

    /// Publishes a complete settings replacement with its next revision.
    pub fn replace_user_settings(&mut self, settings: UserSettings) -> u64 {
        self.generation = self.generation.saturating_add(1);
        if self.user_settings.controls != settings.controls {
            self.controls_generation = self.controls_generation.saturating_add(1);
        }
        self.user_settings = settings;
        self.generation
    }

    /// Borrows the latest settings and their publication revision.
    #[must_use]
    pub const fn user_settings_update(&self) -> (u64, &UserSettings) {
        (self.generation, &self.user_settings)
    }

    /// Borrows controls with a revision that changes only when controls change.
    #[must_use]
    pub const fn controls_update(&self) -> (u64, &ControlSettings) {
        (self.controls_generation, &self.user_settings.controls)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn unrelated_settings_do_not_replace_controls() {
        let mut state = SettingsState::default();
        let controls = state.controls_update().1.clone();
        let controls_generation = state.controls_update().0;
        let default = render_api::VsyncMode::DEFAULT.enabled();
        for vsync in [!default, default, !default] {
            let mut user = state.user_settings_update().1.clone();
            user.video.vsync = vsync.into();
            let previous_generation = state.user_settings_update().0;
            state.replace_user_settings(user);
            assert!(state.user_settings_update().0 > previous_generation);
            assert_eq!(state.controls_update(), (controls_generation, &controls));
        }
    }

    #[test]
    fn controls_edits_advance_the_controls_revision_and_equal_replacements_do_not() {
        let mut state = SettingsState::default();
        let mut user = state.user_settings_update().1.clone();
        user.controls.invert_mouse_y = !user.controls.invert_mouse_y;
        let previous = state.controls_update().0;
        state.replace_user_settings(user.clone());
        assert!(state.controls_update().0 > previous);
        assert_eq!(state.controls_update().1, &user.controls);
        let changed = state.controls_update().0;
        state.replace_user_settings(user);
        assert_eq!(state.controls_update().0, changed);
    }
}
