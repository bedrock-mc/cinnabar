//! Applies confirmed launcher resets to live menu state.
#[cfg(test)]
use launcher::menu::settings_options::SettingsOptions;
use launcher::menu::settings_options::{SETTINGS_OPTIONS, SettingsGroup};
use {crate::menu::MenuRuntime, launcher::menu::MenuDialog};

use launcher::menu::settings_options::SettingId;
impl MenuRuntime {
    /// Applies a section reset only after its matching confirmation remains open.
    pub(in crate::menu) fn confirm_settings_reset(&mut self, group: SettingsGroup) {
        if self.dialog != Some(MenuDialog::SettingsResetGroup(group)) {
            return;
        }
        self.dialog = None;
        let overrides = self.session_overrides.len();
        self.session_overrides.retain(|preference| {
            !SETTINGS_OPTIONS[preference.index()]
                .reset_group
                .contains(&group)
        });
        let changed = std::sync::Arc::make_mut(&mut self.settings_options).reset_group(group);
        if changed || overrides != self.session_overrides.len() {
            self.settings_dirty = true;
            self.settings_apply = true;
        }
        if SettingId::GuiScale
            .definition()
            .reset_group
            .contains(&group)
        {
            let defaults = launcher_host::video_settings::SavedVideoSettings::default();
            self.gui_scale_preference = None;
            self.gui_scale_offset = defaults.gui_scale_offset;
            self.gui_scale_display_offset = defaults.gui_scale_offset;
            if SettingId::FullScreen
                .definition()
                .reset_group
                .contains(&group)
            {
                self.activate_settings(launcher::menu::MenuAction::SettingsFullscreen(
                    defaults.fullscreen,
                ));
            }
        }
        self.settings.dropdown = None;
        self.settings.scale_picker = false;
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
        assert_eq!(menu.settings_options.value(SettingId::FieldOfView), value);
        menu.activate(MenuAction::SettingsConfirmResetGroup(SettingsGroup::Audio));
        assert_eq!(menu.settings_options.value(SettingId::FieldOfView), value);
        menu.activate(MenuAction::DismissDialog);
        menu.activate(MenuAction::SettingsConfirmResetGroup(SettingsGroup::Video));
        assert_eq!(menu.settings_options.value(SettingId::FieldOfView), value);
        menu.activate(MenuAction::SettingsResetGroup(SettingsGroup::Video));
        menu.activate(MenuAction::SettingsConfirmResetGroup(SettingsGroup::Video));
        assert!(menu.dialog.is_none());
        assert_eq!(
            menu.settings_options.value(SettingId::FieldOfView),
            SettingsOptions::default().value(SettingId::FieldOfView)
        );
        assert!(menu.settings_dirty && menu.settings_apply);
    }

    #[test]
    fn group_reset_replaces_session_baselines_and_saves_defaults() {
        use launcher::menu::settings_options::persistence::SETTINGS_FILE;

        let default = SettingId::Gamma.definition().default;
        for effective in [default, SettingId::Gamma.definition().max] {
            let layout = launcher::test_support::scratch("reset-session-overrides");
            let path = layout.server_file().with_file_name(SETTINGS_FILE);
            let skin = crate::player_skin::LocalPlayerSkin::generated_default("Reset");
            let mut menu =
                MenuRuntime::new_with_layout(true, Some(2), "Reset".into(), layout, skin);
            menu.set_named_option(SettingId::Gamma, SettingId::Gamma.definition().min);
            menu.set_named_option(SettingId::MainVolume, 30);
            menu.flush_user_settings().unwrap();
            menu.set_session_option(SettingId::Gamma, Some(effective));
            menu.set_session_option(SettingId::MainVolume, Some(40));
            menu.settings_dirty = false;
            menu.settings_apply = false;

            menu.activate(MenuAction::SettingsResetGroup(SettingsGroup::Video));
            menu.activate(MenuAction::SettingsConfirmResetGroup(SettingsGroup::Video));
            assert!(menu.settings_dirty && menu.settings_apply);
            assert_eq!(menu.settings_options.value(SettingId::Gamma), default);
            assert_eq!(menu.settings_options.value(SettingId::MainVolume), 40);
            menu.flush_user_settings().unwrap();
            let loaded = SettingsOptions::load(&path);
            assert_eq!(loaded.value(SettingId::Gamma), default);
            assert_eq!(loaded.value(SettingId::MainVolume), 30);

            menu.set_session_option(SettingId::Gamma, None);
            assert_eq!(menu.settings_options.value(SettingId::Gamma), default);
            menu.set_session_option(SettingId::MainVolume, None);
            assert_eq!(menu.settings_options.value(SettingId::MainVolume), 30);
            std::fs::remove_file(path).unwrap();
        }
    }
}
