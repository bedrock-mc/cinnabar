//! Persisted launcher options and engine-independent input bindings.

pub mod antialiasing;
pub mod bindings;
pub mod chat;
pub mod command_macros;
pub mod control_bindings;
pub mod definitions;
pub mod emotes;
pub mod graphics_api;
pub mod keybindings;
pub mod language;
pub mod layout;
pub mod macro_modifier;
pub mod persistence;
pub mod render_distance;
pub mod reset;
pub mod runtime;
pub mod video_presets;
pub use chat::CHAT_POSITION_OPTION;
pub use reset::SettingsGroup;
#[cfg(test)]
mod registry_tests;
#[cfg(test)]
mod tests;

use std::collections::BTreeMap;

use serde::{Deserialize, Serialize};

pub use bindings::{
    BindingId, EXTRA_GAMEPAD, EXTRA_KEYS, GAMEPAD_BINDINGS, KEY_BINDINGS, SHADER_PACK_BINDINGS,
};
pub use command_macros::{
    COMMAND_MACRO_COUNT, MAX_COMMAND_MACRO_BYTES, macro_message, macro_row, macro_row_index,
    macro_title,
};
pub use control_bindings::{GAMEPAD_OFFSET, gamepad_icon};
pub use definitions::{
    ANIMATION_CHOICES, ANIMATIONS_OPTION, AUTO_UPDATE_OPTION, CINNABAR_VISIBILITY_OPTION,
    DISCORD_PRESENCE_OPTION, FRAME_RATE_UNLIMITED, INSTALL_ON_QUIT_OPTION, INVERT_CROSSHAIR_OPTION,
    MAX_FIXED_FRAME_RATE, MIN_FIXED_FRAME_RATE, MOTION_BLUR_CHOICES, MOTION_BLUR_OPTION,
    MOUSE_SENSITIVITY_OPTION, NATIVE_SKYBOX_OFFSET_OPTION, SETTINGS_OPTIONS, SMAA_CHOICES,
    SMAA_OPTION, SettingDefinition, SettingId, SettingKind, THIRD_PERSON_CROSSHAIR_OPTION,
    UPDATE_CHANNEL_OPTION, VIEW_BOBBING_CHOICES, VRR_CHOICES, VRR_OPTION,
};
pub use emotes::EMOTE_SLOT_COUNT;
pub use keybindings::{
    DEBUG_OVERLAY_KEY, OPEN_NOTIFICATION_KEY, PLAYER_LIST_KEY, custom_key_label, key_name,
};
pub use persistence::{SETTINGS_FILE, SETTINGS_SCHEMA};
pub use runtime::{frame_rate_label, frame_rate_limit};

/// Persisted toggle shared by the new UI's sizing and settings controls.
pub const EXTRA_LARGE_UI: &str = SettingId::GuiAccessibilityScaling.name();

pub const SHOW_EXACT_SERVER_PING: &str = SettingId::ShowExactServerPing.name();
pub const OREUI_DARK_MODE: &str = SettingId::OreuiDarkMode.name();

/// One stored option, including whether the player has made an explicit choice.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct OptionPreference {
    index: usize,
    value: Option<i32>,
}

impl OptionPreference {
    /// Returns the registry index of this stored option.
    pub fn index(self) -> usize {
        self.index
    }
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct SettingsOptions {
    /// Version of the stored settings format.
    #[serde(default = "persistence::legacy_schema")]
    schema: u32,
    values: BTreeMap<String, i32>,
    /// Increments on Graphics API selections so choosing Auto can reset crash recovery.
    graphics_api_revision: u64,
    #[serde(skip)]
    anti_aliasing_support: ui::AntiAliasingSupport,
    #[serde(skip)]
    render_distance_defaults: ui::RenderDistanceDefaults,
    keys: BTreeMap<String, u16>,
    /// Layout labels observed this session, independent of persisted bindings.
    #[serde(skip)]
    key_labels: BTreeMap<u16, String>,
    language: Option<String>,
    /// None means no macro command has been stored yet.
    command_macros: Option<[Option<String>; COMMAND_MACRO_COUNT]>,
    /// None means the original custom catalog supplies first-run defaults.
    emote_slots: Option<[Option<String>; EMOTE_SLOT_COUNT]>,
    server_list: super::server_list::ServerListPreferences,
}

impl Default for SettingsOptions {
    fn default() -> Self {
        Self {
            schema: SETTINGS_SCHEMA,
            values: BTreeMap::new(),
            graphics_api_revision: 0,
            anti_aliasing_support: ui::AntiAliasingSupport::default(),
            render_distance_defaults: ui::RenderDistanceDefaults::default(),
            keys: BTreeMap::new(),
            key_labels: BTreeMap::new(),
            language: None,
            command_macros: None,
            emote_slots: None,
            server_list: super::server_list::ServerListPreferences::default(),
        }
    }
}

impl SettingsOptions {
    /// Captures an option's stored choice without resolving runtime defaults or device limits.
    pub fn option_preference(&self, index: usize) -> Option<OptionPreference> {
        let option = SETTINGS_OPTIONS.get(index)?;
        Some(OptionPreference {
            index,
            value: self.values.get(option.name).copied(),
        })
    }

    /// Restores a captured option exactly, preserving an untouched or legacy preference.
    pub fn restore_option_preference(&mut self, preference: OptionPreference) -> bool {
        let option = SETTINGS_OPTIONS[preference.index];
        match preference.value {
            Some(value) => self.values.insert(option.name.to_owned(), value) != Some(value),
            None => self.values.remove(option.name).is_some(),
        }
    }

    /// The saved check preference, quit-time opt-in and selected release channel.
    pub fn update_preferences(&self) -> super::updates::Preferences {
        super::updates::Preferences {
            checks: self.value(AUTO_UPDATE_OPTION.id) != 0,
            install_on_quit: self.value(INSTALL_ON_QUIT_OPTION.id) != 0,
            channel: update_manifest::channel::Channel::ALL
                .get(self.value(UPDATE_CHANNEL_OPTION.id) as usize)
                .copied()
                .filter(|channel| channel.is_published())
                .unwrap_or_default(),
        }
    }

    pub fn server_list(&self) -> &super::server_list::ServerListPreferences {
        &self.server_list
    }

    pub fn apply_server_list(&mut self, action: super::server_list::ServerListAction) -> bool {
        self.server_list.apply(action)
    }

    pub fn exact_server_ping(&self) -> bool {
        self.value(SettingId::ShowExactServerPing) != 0
    }

    pub fn oreui_dark_mode(&self) -> bool {
        self.value(SettingId::OreuiDarkMode) != 0
    }

    /// Whether launch and Settings layer debug levels for the net, session, core and packs
    /// log categories over the defaults. `CINNABAR_LOG` and `RUST_LOG` take precedence.
    pub fn verbose_logging(&self) -> bool {
        self.value(SettingId::VerboseLogging) != 0
    }

    /// Whether each new network session records a full packet capture to the log directory.
    /// `CINNABAR_PACKET_CAPTURE` takes precedence.
    pub fn record_network_session(&self) -> bool {
        self.value(SettingId::RecordNetworkSession) != 0
    }

    /// Whether new menu screens use the additional accessibility scale step.
    pub fn extra_large_ui(&self) -> bool {
        self.value(SettingId::GuiAccessibilityScaling) != 0
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
        if definition.id == SettingId::GraphicsApi
            && !graphics_api::visible(value as usize, crate::graphics_backend::Platform::current())
        {
            return 0;
        }
        if definition.id == SettingId::Msaa {
            self.anti_aliasing_support.select(value.max(1) as u32) as i32
        } else {
            value
        }
    }

    /// Reads a registered runtime setting without a fallible name lookup.
    ///
    /// ```compile_fail
    /// let options = launcher::menu::settings_options::SettingsOptions::default();
    /// options.value("hide_hud");
    /// ```
    pub fn value(&self, id: SettingId) -> i32 {
        self.get(id.index())
    }

    /// Validates and snaps a UI or persisted value to its declared range.
    pub fn set(&mut self, index: usize, value: i32) -> bool {
        let Some(definition) = SETTINGS_OPTIONS.get(index) else {
            return false;
        };
        if matches!(definition.kind, SettingKind::Host) {
            return false;
        }
        let value = value.clamp(definition.min, definition.max);
        let value = definition.min + ((value - definition.min) / definition.step) * definition.step;
        let value = if definition.id == SettingId::Msaa {
            self.anti_aliasing_support
                .counts()
                .min_by_key(|samples| samples.abs_diff(value as u32))
                .unwrap_or(1) as i32
        } else {
            value
        };
        if definition.id == SettingId::GraphicsApi {
            self.graphics_api_revision = self.graphics_api_revision.wrapping_add(1);
            self.values.insert(definition.name.to_owned(), value);
            return true;
        }
        if self
            .values
            .get(definition.name)
            .copied()
            .unwrap_or_else(|| self.option_default(definition))
            == value
            && (definition.id != SettingId::RenderDistance
                || self.values.contains_key(definition.name))
        {
            return false;
        }
        self.values.insert(definition.name.to_owned(), value);
        true
    }
}

/// Returns sound rows together with their category, independent of registry order.
pub fn volume_settings() -> impl Iterator<Item = &'static SettingDefinition> {
    SETTINGS_OPTIONS
        .iter()
        .filter(|option| option.volume.is_some())
}
