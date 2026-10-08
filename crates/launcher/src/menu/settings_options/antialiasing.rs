//! Device capabilities constrain the vanilla MSAA slider without rewriting saved preferences.

use super::{SETTINGS_OPTIONS, SettingsOptions};

impl SettingsOptions {
    /// Replaces runtime capabilities; they are never persisted with user preferences.
    pub fn set_anti_aliasing_support(&mut self, support: ui::AntiAliasingSupport) -> bool {
        if self.anti_aliasing_support == support {
            return false;
        }
        self.anti_aliasing_support = support;
        true
    }

    /// Returns the sample counts accepted by both the color and depth attachments.
    pub fn anti_aliasing_support(&self) -> ui::AntiAliasingSupport {
        self.anti_aliasing_support
    }

    /// Advances discrete MSAA stops and the existing integral sliders for keyboard input.
    pub fn offset_value(&self, index: usize, direction: i32) -> i32 {
        let Some(option) = SETTINGS_OPTIONS.get(index) else {
            return 0;
        };
        let value = self.get(index);
        if option.name != "msaa" {
            return value.saturating_add(direction * option.step);
        }
        let counts: Vec<_> = self.anti_aliasing_support.counts().collect();
        let position = counts
            .iter()
            .position(|count| *count == value as u32)
            .unwrap_or(0);
        let next = (position as i32 + direction).clamp(0, counts.len() as i32 - 1);
        counts[next as usize] as i32
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Finds the registered control used by persistence and UI actions.
    fn index() -> usize {
        SETTINGS_OPTIONS
            .iter()
            .position(|option| option.name == "msaa")
            .unwrap()
    }

    #[test]
    fn antialiasing_round_trips_samples_and_keeps_device_caps_out_of_storage() {
        let mut settings = SettingsOptions::default();
        assert_eq!(
            settings.user_settings().video.anti_aliasing_samples,
            ui::DEFAULT_ANTI_ALIASING_SAMPLES
        );
        assert!(settings.set(index(), 8));
        settings.set_anti_aliasing_support(ui::AntiAliasingSupport::from_counts([1, 4]));
        assert_eq!(settings.get(index()), 4);
        assert_eq!(settings.offset_value(index(), -1), 1);
        let bytes = serde_json::to_vec(&settings).unwrap();
        let loaded = SettingsOptions::decode(&bytes).unwrap();
        assert_eq!(loaded.user_settings().video.anti_aliasing_samples, 8);
        assert!(
            !String::from_utf8(bytes)
                .unwrap()
                .contains("anti_aliasing_support")
        );
    }

    #[test]
    fn antialiasing_rejects_non_power_of_two_persisted_values_and_resets_with_video() {
        let mut settings = SettingsOptions::decode(br#"{"values":{"msaa":3}}"#).unwrap();
        assert_eq!(
            settings.get(index()),
            ui::DEFAULT_ANTI_ALIASING_SAMPLES as i32
        );
        settings.set(index(), 7);
        assert_eq!(settings.get(index()), 8);
        settings.reset_group(super::super::SettingsGroup::Video);
        assert_eq!(
            settings.get(index()),
            ui::DEFAULT_ANTI_ALIASING_SAMPLES as i32
        );
    }
}
