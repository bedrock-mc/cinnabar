//! Settings actions stay separate from launcher navigation.

use std::sync::Arc;
use {
    crate::menu::MenuRuntime,
    launcher::menu::{MenuAction, MenuField, settings_options::macro_row},
};

impl MenuRuntime {
    /// Whether binding capture currently owns desktop input.
    pub fn is_remapping_key(&self) -> bool {
        self.settings.key_remap.is_some()
    }

    /// Applies settings navigation and edits without starting a game session.
    pub(in crate::menu) fn activate_settings(&mut self, action: MenuAction) {
        match action {
            MenuAction::SearchSettings => {
                self.settings.slider_selected = None;
                self.settings.dropdown = None;
                self.settings.scale_picker = false;
                self.settings.key_remap = None;
            }
            MenuAction::ClearSettingsSearch => {
                self.settings_query.set_text("");
                self.settings.slider_selected = None;
                self.settings.dropdown = None;
                self.settings.scale_picker = false;
                self.focus_field(MenuField::SettingsQuery);
                self.settings_input.actions = vec![MenuAction::SearchSettings];
                self.focused = 0;
            }
            MenuAction::SettingsResetChat => self.reset_chat_settings(),
            MenuAction::SettingsAdvancedGraphics => {
                self.settings.advanced_graphics = !self.settings.advanced_graphics;
            }
            MenuAction::SettingsScale(offset) => {
                self.set_gui_scale_offset(offset);
                if self.settings.scale_picker {
                    self.settings.scale_picker = false;
                    self.settings_input.actions = vec![MenuAction::SettingsScalePicker];
                    self.focused = 0;
                }
            }
            MenuAction::SettingsScalePicker => {
                self.settings.dropdown = None;
                self.settings.scale_picker = !self.settings.scale_picker;
                self.settings_input.actions = vec![if self.settings.scale_picker {
                    MenuAction::SettingsScale(self.gui_scale_display_offset)
                } else {
                    MenuAction::SettingsScalePicker
                }];
                self.focused = 0;
            }
            MenuAction::SettingsFullscreen(fullscreen) => {
                self.fullscreen = fullscreen;
                self.fullscreen_change = Some(fullscreen);
            }
            MenuAction::SettingsSection(section) => {
                self.settings_query.set_text("");
                self.settings.slider_selected = None;
                self.settings.section = section;
                if section == launcher::menu::settings_storage::SECTION_INDEX {
                    self.refresh_storage();
                }
                self.settings.dropdown = None;
                self.settings.scale_picker = false;
                self.settings.key_remap = None;
            }
            MenuAction::SettingsOption(index, value) => {
                if self.settings.dropdown == Some(index) {
                    self.settings_input.actions = vec![MenuAction::SettingsDropdown(index)];
                    self.focused = 0;
                }
                self.set_option(index, value);
                self.settings.dropdown = None;
            }
            MenuAction::SettingsVideoPreset(preset) => {
                let overrides = self.session_overrides.len();
                self.session_overrides.retain(|preference| {
                    !launcher::menu::settings_options::video_presets::VideoPreset::controls(
                        launcher::menu::settings_options::SETTINGS_OPTIONS[preference.index()].id,
                    )
                });
                let changed = Arc::make_mut(&mut self.settings_options).apply_video_preset(preset);
                if changed || overrides != self.session_overrides.len() {
                    self.settings_dirty = true;
                    self.settings_apply = true;
                }
                self.settings.dropdown = None;
                self.settings.scale_picker = false;
                self.settings.slider_selected = None;
            }
            MenuAction::SettingsLanguage(index) => self.set_language(index),
            MenuAction::SettingsDropdown(index) => {
                self.settings.scale_picker = false;
                self.settings.dropdown = (self.settings.dropdown != Some(index)).then_some(index);
                self.settings_input.actions = vec![if self.settings.dropdown.is_some() {
                    MenuAction::SettingsOption(index, self.settings_options.get(usize::from(index)))
                } else {
                    MenuAction::SettingsDropdown(index)
                }];
                self.focused = 0;
            }
            MenuAction::SettingsResetGroup(group) => {
                self.dialog = Some(launcher::menu::MenuDialog::SettingsResetGroup(group))
            }
            MenuAction::SettingsConfirmResetGroup(group) => self.confirm_settings_reset(group),
            MenuAction::SettingsResetBindings(gamepad) => {
                self.dialog = Some(launcher::menu::MenuDialog::SettingsResetBindings(gamepad));
            }
            MenuAction::SettingsConfirmResetBindings(gamepad) => {
                if self.dialog != Some(launcher::menu::MenuDialog::SettingsResetBindings(gamepad)) {
                    return;
                }
                self.dialog = None;
                Arc::make_mut(&mut self.settings_options).reset_bindings(gamepad);
                self.settings.key_remap = None;
                self.settings_dirty = true;
                self.settings_apply = true;
            }
            MenuAction::SettingsKey(index) => self.settings.key_remap = Some(index),
            MenuAction::SettingsMacroCommand(slot) => {
                self.focus_field(MenuField::MacroCommand(slot));
            }
            MenuAction::SettingsResetKey(index) => {
                if let Some(slot) = macro_row(usize::from(index)) {
                    if Arc::make_mut(&mut self.settings_options).reset_macro(slot) {
                        self.settings_dirty = true;
                        self.settings_apply = true;
                    } else {
                        self.message = Some("That macro key was not assigned.".to_owned());
                    }
                } else if Arc::make_mut(&mut self.settings_options).reset_key(usize::from(index)) {
                    self.settings_dirty = true;
                    self.settings_apply = true;
                } else {
                    self.message = Some("That default control could not be restored.".to_owned());
                }
            }
            _ => {}
        }
    }

    /// Captures a desktop key or pointer button for the pending remap operation.
    pub(in crate::menu) fn capture_key(&mut self, control: semantic_input::PhysicalControl) {
        let Some(index) = self.settings.key_remap.take() else {
            return;
        };
        let settings = Arc::make_mut(&mut self.settings_options);
        if let Some(slot) = macro_row(usize::from(index)) {
            if settings.remap_macro(slot, control) {
                self.settings_dirty = true;
                self.settings_apply = true;
            } else {
                self.message = Some("That control cannot be assigned to this action.".to_owned());
            }
        } else if settings.remap(usize::from(index), control) {
            self.settings_dirty = true;
            self.settings_apply = true;
        } else {
            self.message = Some("That control cannot be assigned to this action.".to_owned());
        }
    }

    /// Ends capture, clearing only a selected keyboard row's assignments.
    pub(in crate::menu) fn capture_escape(&mut self) {
        let Some(index) = self.settings.key_remap.take().map(usize::from) else {
            return;
        };
        if macro_row(index).is_none() && Arc::make_mut(&mut self.settings_options).unbind_key(index)
        {
            self.settings_dirty = true;
            self.settings_apply = true;
        }
    }
}
