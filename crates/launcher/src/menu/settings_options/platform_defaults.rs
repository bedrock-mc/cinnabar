//! Defaults that depend on where the client runs; saved choices always win over them.

use super::{SettingDefinition, SettingsOptions};

/// Untouched options a browser starts with instead of the desktop default. A tab shares the GPU
/// with the page and the browser, so it starts without multisampling or clouds.
pub const BROWSER_DEFAULTS: &[(&str, i32)] = &[("msaa", 1), ("render_clouds", 0)];

/// Whether this build starts from [`BROWSER_DEFAULTS`].
pub const PREFERS_PERFORMANCE: bool = cfg!(target_arch = "wasm32");

impl SettingsOptions {
    /// The value an untouched `option` reads and a reset restores: the device's render-distance
    /// recommendation, a browser's cheaper default, or the registry default.
    pub(super) fn option_default(&self, option: &SettingDefinition) -> i32 {
        if option.name == "render_distance" {
            return self.render_distance_defaults.recommended as i32;
        }
        self.performance_defaults
            .then(|| {
                BROWSER_DEFAULTS
                    .iter()
                    .find(|(name, _)| *name == option.name)
            })
            .flatten()
            .map_or(option.default, |&(_, value)| value)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::menu::settings_options::{SETTINGS_OPTIONS, SettingsGroup};

    fn browser() -> SettingsOptions {
        SettingsOptions {
            performance_defaults: true,
            ..SettingsOptions::default()
        }
    }

    fn index(name: &str) -> usize {
        SETTINGS_OPTIONS
            .iter()
            .position(|option| option.name == name)
            .unwrap()
    }

    #[test]
    fn browsers_start_without_multisampling_or_clouds_and_desktops_keep_theirs() {
        let browser = browser();
        assert_eq!(browser.user_settings().video.anti_aliasing_samples, 1);
        assert_eq!(browser.value("render_clouds"), 0);
        let desktop = SettingsOptions {
            performance_defaults: false,
            ..SettingsOptions::default()
        };
        assert_eq!(
            desktop.user_settings().video.anti_aliasing_samples,
            ui::DEFAULT_ANTI_ALIASING_SAMPLES
        );
        assert_eq!(desktop.value("render_clouds"), 1);
    }

    #[test]
    fn a_browser_keeps_saved_choices_and_resets_to_its_own_defaults() {
        let mut options = browser();
        assert!(options.set(index("render_clouds"), 1));
        assert!(options.set(index("msaa"), 4));
        let saved = serde_json::to_vec(&options).unwrap();
        let mut loaded = SettingsOptions::decode(&saved).unwrap();
        loaded.performance_defaults = true;
        assert_eq!(loaded.value("render_clouds"), 1);
        assert_eq!(loaded.value("msaa"), 4);
        assert!(loaded.reset_group(SettingsGroup::Video));
        assert_eq!(loaded.value("render_clouds"), 0);
        assert_eq!(loaded.value("msaa"), 1);
    }

    #[test]
    fn every_browser_default_names_a_registered_option_within_its_range() {
        for (name, value) in BROWSER_DEFAULTS {
            let option = SETTINGS_OPTIONS[index(name)];
            assert!((option.min..=option.max).contains(value), "{name}");
        }
    }
}
