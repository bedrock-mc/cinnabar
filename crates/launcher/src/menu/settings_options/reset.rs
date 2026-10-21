//! Reset membership belongs to each registered setting.

use super::{SETTINGS_OPTIONS, SettingId, SettingsOptions};

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum SettingsGroup {
    Video,
    Accessibility,
    Audio,
    Chat,
    Discord,
}

impl SettingsGroup {
    /// Returns the settings restored by this group, in registry order.
    pub fn options(self) -> impl Iterator<Item = &'static super::SettingDefinition> {
        SETTINGS_OPTIONS
            .iter()
            .filter(move |option| option.reset_group.contains(&self))
    }
}

impl SettingsOptions {
    /// Restores one group's defaults without changing bindings or unrelated settings.
    pub fn reset_group(&mut self, group: SettingsGroup) -> bool {
        let mut changed = false;
        for option in group.options() {
            if option.id == SettingId::RenderDistance {
                changed |= self.values.remove(option.name).is_some();
            } else {
                changed |= self.set(option.id.index(), option.default);
            }
        }
        changed
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Changes a registry option without introducing another copy of its default.
    fn change(options: &mut SettingsOptions, id: SettingId) -> i32 {
        let index = SETTINGS_OPTIONS
            .iter()
            .position(|option| option.id == id)
            .unwrap();
        let option = SETTINGS_OPTIONS[index];
        let value = if option.default == option.min {
            option.max
        } else {
            option.min
        };
        assert!(options.set(index, value));
        value
    }

    #[test]
    fn section_reset_restores_defaults_and_preserves_unrelated_settings() {
        for (group, member, unrelated) in [
            (
                SettingsGroup::Video,
                SettingId::FieldOfView,
                SettingId::MainVolume,
            ),
            (
                SettingsGroup::Accessibility,
                SettingId::ChatBackgroundOpacity,
                SettingId::FieldOfView,
            ),
            (
                SettingsGroup::Audio,
                SettingId::MainVolume,
                SettingId::ChatBackgroundOpacity,
            ),
        ] {
            let mut options = SettingsOptions::default();
            change(&mut options, member);
            let retained = change(&mut options, unrelated);
            let key = options.key_control(0);
            assert!(options.reset_group(group));
            let default = SETTINGS_OPTIONS
                .iter()
                .find(|option| option.id == member)
                .unwrap()
                .default;
            assert_eq!(options.value(member), default);
            assert_eq!(options.value(unrelated), retained);
            assert_eq!(options.key_control(0), key);
        }
    }
}
