//! Applies confirmed launcher resets to live menu state.
use launcher::menu::settings_options::SettingsGroup;
#[cfg(test)]
use launcher::menu::settings_options::{SETTINGS_OPTIONS, SettingsOptions};
use {crate::menu::MenuRuntime, launcher::menu::MenuDialog};

impl MenuRuntime {
    /// Applies a section reset only after its matching confirmation remains open.
    pub(in crate::menu) fn confirm_settings_reset(&mut self, group: SettingsGroup) {
        if self.dialog != Some(MenuDialog::SettingsResetGroup(group)) {
            return;
        }
        self.dialog = None;
        if std::sync::Arc::make_mut(&mut self.settings_options).reset_group(group) {
            self.settings_dirty = true;
            self.settings_apply = true;
        }
        if matches!(group, SettingsGroup::Video | SettingsGroup::Accessibility) {
            let defaults = launcher_host::video_settings::SavedVideoSettings::default();
            self.gui_scale_preference = None;
            self.gui_scale_offset = defaults.gui_scale_offset;
            self.gui_scale_display_offset = defaults.gui_scale_offset;
            if group == SettingsGroup::Video {
                self.activate_settings(launcher::menu::MenuAction::SettingsFullscreen(
                    defaults.fullscreen,
                ));
            }
        }
        self.settings_dropdown = None;
        self.settings_scale_picker = false;
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use launcher::menu::MenuAction;
    /// Changes a registry option without introducing another copy of its default.
    fn change(options: &mut SettingsOptions, name: &str) -> i32 {
        let index = SETTINGS_OPTIONS
            .iter()
            .position(|option| option.name == name)
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
    fn reset_cancel_and_mismatched_confirmation_preserve_values() {
        let mut menu = MenuRuntime::new(true, 2, "Steve".into());
        let value = change(
            std::sync::Arc::make_mut(&mut menu.settings_options),
            "field_of_view",
        );
        menu.activate(MenuAction::SettingsResetGroup(SettingsGroup::Video));
        assert_eq!(menu.settings_options.value("field_of_view"), value);
        menu.activate(MenuAction::SettingsConfirmResetGroup(SettingsGroup::Audio));
        assert_eq!(menu.settings_options.value("field_of_view"), value);
        menu.activate(MenuAction::DismissDialog);
        menu.activate(MenuAction::SettingsConfirmResetGroup(SettingsGroup::Video));
        assert_eq!(menu.settings_options.value("field_of_view"), value);
        menu.activate(MenuAction::SettingsResetGroup(SettingsGroup::Video));
        menu.activate(MenuAction::SettingsConfirmResetGroup(SettingsGroup::Video));
        assert!(menu.dialog.is_none());
        assert_eq!(
            menu.settings_options.value("field_of_view"),
            SettingsOptions::default().value("field_of_view")
        );
        assert!(menu.settings_dirty && menu.settings_apply);
    }
}
