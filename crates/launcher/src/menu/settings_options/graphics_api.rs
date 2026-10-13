//! Graphics API choices persist through the existing settings registry and apply at startup.

use super::{SettingDefinition, SettingKind, SettingsOptions, definitions::SettingChoice};
use crate::graphics_backend::{Backend, Platform};

/// Returns the backend represented by a stable saved choice; zero is Auto.
pub const fn backend(choice: usize) -> Option<Backend> {
    if choice == 0 || choice > Backend::ALL.len() {
        None
    } else {
        Some(Backend::ALL[choice - 1])
    }
}

/// Whether a stored choice names Auto or an API provided by the selected platform.
pub fn visible(choice: usize, platform: Platform) -> bool {
    choice == 0 || backend(choice).is_some_and(|backend| platform.backends().contains(&backend))
}

/// Derives names and labels from the policy's backend registry.
const fn choices() -> [SettingChoice; Backend::ALL.len() + 1] {
    let mut choices = [SettingChoice {
        name: "auto",
        label: "Auto",
    }; Backend::ALL.len() + 1];
    let mut i = 0;
    while i < Backend::ALL.len() {
        choices[i + 1] = SettingChoice {
            name: Backend::ALL[i].name(),
            label: Backend::ALL[i].label(Platform::current()),
        };
        i += 1;
    }
    choices
}

/// Choice values remain stable even when a platform hides an unavailable API.
pub const CHOICES: &[SettingChoice] = &choices();
/// The registry owner for the restart-only graphics preference.
pub const OPTION: SettingDefinition = SettingDefinition {
    name: "graphics_api",
    label: "Graphics API",
    kind: SettingKind::Dropdown(CHOICES),
    min: 0,
    max: CHOICES.len() as i32 - 1,
    step: 1,
    default: 0,
};

impl SettingsOptions {
    /// Reads the saved startup override; unsupported transferred preferences become Auto.
    pub fn graphics_api(&self) -> Option<Backend> {
        backend(self.value(OPTION.name) as usize)
            .filter(|backend| Platform::current().backends().contains(backend))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
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
            assert_eq!(loaded.value(OPTION.name), expected);
            assert_eq!(loaded.user_settings().video, video);
            if let Some(backend) = backend(choice) {
                assert_eq!(definition.name, backend.name());
            }
            options.reset_group(super::super::SettingsGroup::Video);
            assert_eq!(options.graphics_api(), None);
        }
    }
}
