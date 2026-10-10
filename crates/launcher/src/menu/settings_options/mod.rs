//! Persisted launcher options and engine-independent input bindings.

pub mod antialiasing;
pub mod chat;
pub mod control_bindings;
pub mod definitions;
pub mod emotes;
pub mod keybindings;
pub mod language;
pub mod persistence;
pub mod render_distance;
pub mod reset;
pub mod runtime;
pub use chat::CHAT_POSITION_OPTION;
pub use reset::SettingsGroup;
#[cfg(test)]
mod tests;

use std::collections::BTreeMap;

use serde::{Deserialize, Serialize};

pub use control_bindings::{
    EXTRA_GAMEPAD, EXTRA_KEYS, GAMEPAD_BINDINGS, GAMEPAD_OFFSET, gamepad_icon,
};
pub use definitions::{
    ANIMATION_CHOICES, ANIMATIONS_OPTION, DISCORD_PRESENCE_OPTION, FRAME_RATE_UNLIMITED,
    INVERT_CROSSHAIR_OPTION, MAX_FIXED_FRAME_RATE, MIN_FIXED_FRAME_RATE, MOTION_BLUR_CHOICES,
    MOTION_BLUR_OPTION, MOUSE_SENSITIVITY_OPTION, SETTINGS_OPTIONS, SMAA_CHOICES, SMAA_OPTION,
    SettingDefinition, SettingKind, THIRD_PERSON_CROSSHAIR_OPTION, VRR_CHOICES, VRR_OPTION,
};
pub use emotes::EMOTE_SLOT_COUNT;
pub use keybindings::{KEY_BINDINGS, OPEN_NOTIFICATION_KEY, key_name};
pub use persistence::{SETTINGS_FILE, SETTINGS_SCHEMA};
pub use runtime::frame_rate_limit;

pub const SHOW_EXACT_SERVER_PING: &str = "show_exact_server_ping";
pub const OREUI_DARK_MODE: &str = "oreui_dark_mode";

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct SettingsOptions {
    /// Version of the stored settings format.
    schema: u32,
    values: BTreeMap<String, i32>,
    #[serde(skip)]
    anti_aliasing_support: ui::AntiAliasingSupport,
    #[serde(skip)]
    render_distance_defaults: ui::RenderDistanceDefaults,
    keys: BTreeMap<String, u16>,
    language: Option<String>,
    /// None means the original custom catalog supplies first-run defaults.
    emote_slots: Option<[Option<String>; EMOTE_SLOT_COUNT]>,
    server_list: super::server_list::ServerListPreferences,
}

impl Default for SettingsOptions {
    fn default() -> Self {
        Self {
            schema: SETTINGS_SCHEMA,
            values: BTreeMap::new(),
            anti_aliasing_support: ui::AntiAliasingSupport::default(),
            render_distance_defaults: ui::RenderDistanceDefaults::default(),
            keys: BTreeMap::new(),
            language: None,
            emote_slots: None,
            server_list: super::server_list::ServerListPreferences::default(),
        }
    }
}

impl SettingsOptions {
    pub fn server_list(&self) -> &super::server_list::ServerListPreferences {
        &self.server_list
    }

    pub fn apply_server_list(&mut self, action: super::server_list::ServerListAction) -> bool {
        self.server_list.apply(action)
    }

    pub fn exact_server_ping(&self) -> bool {
        self.value(SHOW_EXACT_SERVER_PING) != 0
    }

    pub fn oreui_dark_mode(&self) -> bool {
        self.value(OREUI_DARK_MODE) != 0
    }

    /// Returns the saved value, or the registry default for an untouched option.
    pub fn get(&self, index: usize) -> i32 {
        let Some(definition) = SETTINGS_OPTIONS.get(index) else {
            return 0;
        };
        let value = self
            .values
            .get(definition.name)
            .copied()
            .unwrap_or_else(|| self.option_default(definition));
        if definition.name == "msaa" {
            self.anti_aliasing_support.select(value.max(1) as u32) as i32
        } else {
            value
        }
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
        let value = if definition.name == "msaa" {
            self.anti_aliasing_support
                .counts()
                .min_by_key(|samples| samples.abs_diff(value as u32))
                .unwrap_or(1) as i32
        } else {
            value
        };
        if self
            .values
            .get(definition.name)
            .copied()
            .unwrap_or_else(|| self.option_default(definition))
            == value
            && (definition.name != "render_distance" || self.values.contains_key(definition.name))
        {
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
