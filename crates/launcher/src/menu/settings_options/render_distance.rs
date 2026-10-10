//! Device recommendations supply defaults without replacing saved choices.

use super::{SettingDefinition, SettingsOptions};

impl SettingsOptions {
    /// Updates the runtime recommendation without persisting a guessed user preference.
    pub fn set_render_distance_device(&mut self, device: ui::RenderDistanceDevice) -> bool {
        let defaults = device.defaults();
        if self.render_distance_defaults == defaults {
            return false;
        }
        self.render_distance_defaults = defaults;
        true
    }

    /// Returns the recommendation and the unextended device levels for settings hosts.
    pub fn render_distance_defaults(&self) -> ui::RenderDistanceDefaults {
        self.render_distance_defaults
    }

    /// Resolves device-sensitive defaults for untouched options and resets.
    pub(super) fn option_default(&self, option: &SettingDefinition) -> i32 {
        if option.name == "render_distance" {
            self.render_distance_defaults.recommended as i32
        } else {
            option.default
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::menu::settings_options::{SETTINGS_OPTIONS, SettingsGroup};

    /// Uses the same registered slider that accepts persisted values.
    fn index() -> usize {
        SETTINGS_OPTIONS
            .iter()
            .position(|option| option.name == "render_distance")
            .unwrap()
    }

    /// Represents a desktop with separate RAM and graphics memory.
    fn device(ram_gib: u64, gpu_gib: u64) -> ui::RenderDistanceDevice {
        ui::RenderDistanceDevice {
            physical_memory_bytes: ram_gib << 30,
            dedicated_graphics_memory_bytes: gpu_gib << 30,
            use_full_graphics_memory: false,
        }
    }

    #[test]
    fn untouched_settings_follow_the_device_after_loading_and_after_unrelated_saves() {
        let mut options =
            SettingsOptions::decode(br#"{"schema":1,"values":{"gamma":60}}"#).unwrap();
        options.set_render_distance_device(device(8, 4));
        assert_eq!(options.user_settings().video.render_distance_chunks, 28);
        let mut reloaded = SettingsOptions::decode(&serde_json::to_vec(&options).unwrap()).unwrap();
        reloaded.set_render_distance_device(device(16, 4));
        assert_eq!(reloaded.user_settings().video.render_distance_chunks, 35);
    }

    #[test]
    fn explicit_choice_equal_to_the_guess_survives_device_changes_and_video_reset() {
        let mut options = SettingsOptions::default();
        options.set_render_distance_device(device(8, 4));
        assert!(options.set(index(), options.value("render_distance")));
        let mut loaded = SettingsOptions::decode(&serde_json::to_vec(&options).unwrap()).unwrap();
        loaded.set_render_distance_device(device(16, 4));
        assert_eq!(loaded.value("render_distance"), 28);
        assert!(loaded.reset_group(SettingsGroup::Video));
        assert_eq!(loaded.value("render_distance"), 35);
    }

    #[test]
    fn saved_legacy_minimum_and_owner_maximum_always_win() {
        for value in [4, render_api::PHASE0_MAX_VIEW_RADIUS_CHUNKS] {
            let bytes = format!(r#"{{"schema":1,"values":{{"render_distance":{value}}}}}"#);
            let mut loaded = SettingsOptions::decode(bytes.as_bytes()).unwrap();
            loaded.set_render_distance_device(device(2, 0));
            assert_eq!(loaded.value("render_distance"), value);
        }
        let option = SETTINGS_OPTIONS[index()];
        assert_eq!(option.min, ui::MIN_RENDER_DISTANCE_CHUNKS as i32);
        assert_eq!(option.step, 1);
        assert_eq!(option.max, render_api::PHASE0_MAX_VIEW_RADIUS_CHUNKS);
    }
}
