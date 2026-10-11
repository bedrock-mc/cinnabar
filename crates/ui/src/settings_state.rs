//! Retained settings revisions shared by runtime adapters.
use crate::UserSettings;

/// Retained settings handoff used by menus and live subsystem
/// adapters. Every replacement is complete and monotonically versioned.
#[derive(Clone, Debug, Default)]
pub struct SettingsState {
    generation: u64,
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
        self.user_settings = settings;
        self.generation
    }

    /// Borrows the latest settings and their publication revision.
    #[must_use]
    pub const fn user_settings_update(&self) -> (u64, &UserSettings) {
        (self.generation, &self.user_settings)
    }
}
