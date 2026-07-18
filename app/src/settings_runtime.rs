use bevy::prelude::Resource;
use ui::UserSettings;

/// App-owned retained settings handoff used by menus and live subsystem
/// adapters. Every replacement is complete and monotonically versioned.
#[derive(Resource, Clone, Debug, Default)]
pub struct RuntimeSettings {
    generation: u64,
    user_settings: UserSettings,
}

impl RuntimeSettings {
    pub fn replace_user_settings(&mut self, settings: UserSettings) -> u64 {
        self.generation = self.generation.saturating_add(1);
        self.user_settings = settings;
        self.generation
    }

    #[must_use]
    pub const fn user_settings_update(&self) -> (u64, &UserSettings) {
        (self.generation, &self.user_settings)
    }
}
