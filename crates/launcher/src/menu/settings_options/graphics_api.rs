//! Graphics API choices persist through the existing settings registry and apply at startup.

use super::{SettingDefinition, SettingId, SettingsOptions, definitions::SettingChoice};
use crate::graphics_backend::{Backend, Platform};

/// Returns the backend represented by a current saved choice; zero is Auto.
pub const fn backend(choice: usize) -> Option<Backend> {
    if choice == 0 || choice > Backend::AVAILABLE.len() {
        None
    } else {
        Some(Backend::AVAILABLE[choice - 1])
    }
}

/// Whether a stored choice names Auto or an API provided by the selected platform.
pub fn visible(choice: usize, platform: Platform) -> bool {
    choice == 0 || backend(choice).is_some_and(|backend| platform.backends().contains(&backend))
}

/// Derives names and labels from the policy's backend registry.
const fn choices() -> [SettingChoice; Backend::AVAILABLE.len() + 1] {
    let mut choices = [SettingChoice {
        name: "auto",
        label: "Auto",
    }; Backend::AVAILABLE.len() + 1];
    let mut i = 0;
    while i < Backend::AVAILABLE.len() {
        choices[i + 1] = SettingChoice {
            name: Backend::AVAILABLE[i].name(),
            label: Backend::AVAILABLE[i].label(Platform::current()),
        };
        i += 1;
    }
    choices
}

/// Choices share the native candidate registry; persistence migrates older indices.
pub const CHOICES: &[SettingChoice] = &choices();
/// The registry owner for the restart-only graphics preference.
pub const OPTION: SettingDefinition = SettingId::GraphicsApi.definition();

impl SettingsOptions {
    /// Identifies explicit API selections, including choosing the current Auto value again.
    pub fn graphics_api_revision(&self) -> u64 {
        self.graphics_api_revision
    }

    /// Reads the saved startup override; unsupported transferred preferences become Auto.
    pub fn graphics_api(&self) -> Option<Backend> {
        backend(self.value(OPTION.id) as usize)
            .filter(|backend| Platform::current().backends().contains(backend))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn selecting_auto_again_and_changing_apis_persists_a_new_recovery_revision() {
        use super::super::SETTINGS_OPTIONS;
        let index = SETTINGS_OPTIONS
            .iter()
            .position(|option| option.name == OPTION.name)
            .unwrap();
        let mut options = SettingsOptions::default();
        assert_eq!(options.graphics_api_revision(), 0);
        for choice in [0, 1, 0, 0] {
            let previous = options.graphics_api_revision();
            assert!(options.set(index, choice));
            assert_ne!(options.graphics_api_revision(), previous);
            let loaded = SettingsOptions::decode(&serde_json::to_vec(&options).unwrap()).unwrap();
            assert_eq!(
                loaded.graphics_api_revision(),
                options.graphics_api_revision()
            );
            options = loaded;
        }
        let previous = options.graphics_api_revision();
        options.reset_group(super::super::SettingsGroup::Video);
        assert_ne!(options.graphics_api_revision(), previous);
        let previous = options.graphics_api_revision();
        options.reset_group(super::super::SettingsGroup::Audio);
        assert_eq!(options.graphics_api_revision(), previous);
    }

    #[test]
    fn choices_derive_from_policy_and_round_trip_without_changing_runtime_video() {
        use super::super::SETTINGS_OPTIONS;
        let index = SETTINGS_OPTIONS
            .iter()
            .position(|option| option.name == OPTION.name)
            .unwrap();
        for (choice, definition) in CHOICES.iter().enumerate() {
            let mut options = SettingsOptions::default();
            let video = options.user_settings().video;
            options.set(index, choice as i32);
            let loaded = SettingsOptions::decode(&serde_json::to_vec(&options).unwrap()).unwrap();
            let expected = if visible(choice, Platform::current()) {
                choice as i32
            } else {
                0
            };
            assert_eq!(loaded.value(OPTION.id), expected);
            assert_eq!(loaded.user_settings().video, video);
            if let Some(backend) = backend(choice) {
                assert_eq!(definition.name, backend.name());
            }
            options.reset_group(super::super::SettingsGroup::Video);
            assert_eq!(options.graphics_api(), None);
        }
    }
    #[test]
    fn offered_choices_match_native_candidates_and_omit_gl() {
        assert_eq!(CHOICES.len(), Backend::AVAILABLE.len() + 1);
        assert!(
            !CHOICES
                .iter()
                .any(|choice| matches!(choice.name, "gl" | "gles"))
        );
        for platform in [
            Platform::Windows,
            Platform::Linux,
            Platform::Android,
            Platform::Macos,
            Platform::Ios,
        ] {
            let offered: Vec<_> = (0..CHOICES.len())
                .filter(|&choice| visible(choice, platform))
                .filter_map(backend)
                .collect();
            assert_eq!(offered, platform.backends());
        }
    }

    #[test]
    fn legacy_gl_reads_as_auto_and_metal_keeps_its_meaning() {
        for schema in [None, Some(1)] {
            for (old, expected) in [(3, None), (4, Some(Backend::Metal))] {
                let mut saved = serde_json::json!({"values": {"graphics_api": old}});
                if let Some(schema) = schema {
                    saved["schema"] = schema.into();
                }
                let loaded = SettingsOptions::decode(&serde_json::to_vec(&saved).unwrap()).unwrap();
                let expected = expected.filter(|api| Platform::current().backends().contains(api));
                assert_eq!(loaded.graphics_api(), expected);
                let reloaded =
                    SettingsOptions::decode(&serde_json::to_vec(&loaded).unwrap()).unwrap();
                assert_eq!(reloaded.graphics_api(), expected);
                assert_eq!(reloaded.schema, super::super::SETTINGS_SCHEMA);
            }
        }
    }
}
