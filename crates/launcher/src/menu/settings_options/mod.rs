//! Persisted launcher options and engine-independent input bindings.

pub mod chat;
pub mod control_bindings;
pub mod definitions;
pub mod emotes;
pub mod keybindings;
pub mod language;
pub mod persistence;
pub mod reset;
pub mod runtime;
pub use reset::SettingsGroup;
#[cfg(test)]
mod tests;

use std::collections::BTreeMap;

use serde::{Deserialize, Serialize};

pub use control_bindings::{
    EXTRA_GAMEPAD, EXTRA_KEYS, GAMEPAD_BINDINGS, GAMEPAD_OFFSET, gamepad_icon,
};
pub use definitions::{
    ANIMATION_CHOICES, ANIMATIONS_OPTION, INVERT_CROSSHAIR_OPTION, SETTINGS_OPTIONS,
    SettingDefinition, SettingKind, THIRD_PERSON_CROSSHAIR_OPTION,
};
pub use emotes::EMOTE_SLOT_COUNT;
pub use keybindings::{KEY_BINDINGS, key_name};
pub use persistence::SETTINGS_FILE;

#[derive(Clone, Debug, Default, Eq, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct SettingsOptions {
    values: BTreeMap<String, i32>,
    keys: BTreeMap<String, u16>,
    language: Option<String>,
    /// None means the original custom catalog supplies first-run defaults.
    emote_slots: Option<[Option<String>; EMOTE_SLOT_COUNT]>,
}

impl SettingsOptions {
    /// Returns the saved value, or the registry default for an untouched option.
    pub fn get(&self, index: usize) -> i32 {
        let Some(definition) = SETTINGS_OPTIONS.get(index) else {
            return 0;
        };
        self.values
            .get(definition.name)
            .copied()
            .unwrap_or(definition.default)
    }

    /// Looks up a runtime setting by its JSON-UI controller name.
    pub fn value(&self, name: &str) -> i32 {
        SETTINGS_OPTIONS
            .iter()
            .position(|option| option.name == name)
            .map_or(0, |index| self.get(index))
    }

    /// Validates and snaps a UI or persisted value to its declared range.
    pub fn set(&mut self, index: usize, value: i32) -> bool {
        let Some(definition) = SETTINGS_OPTIONS.get(index) else {
            return false;
        };
        let value = value.clamp(definition.min, definition.max);
        let value = definition.min + ((value - definition.min) / definition.step) * definition.step;
        if self.get(index) == value {
            return false;
        }
        self.values.insert(definition.name.to_owned(), value);
        true
    }
}

/// Sound options shared by reset logic and the host mixer adapter.
pub const VOLUME_SETTINGS: [&str; 11] = [
    "main_volume",
    "music_volume",
    "sound_volume",
    "ambient_volume",
    "block_volume",
    "hostile_volume",
    "neutral_volume",
    "player_volume",
    "record_volume",
    "weather_volume",
    "texttospeech_volume",
];
