//! Settings actions stay separate from launcher navigation.

use crate::menu::{MenuAction, MenuRuntime};
use std::sync::Arc;

impl MenuRuntime {
    /// Applies settings navigation and edits without starting a game session.
    pub(in crate::menu) fn activate_settings(&mut self, action: MenuAction) {
        match action {
            MenuAction::SettingsResetChat => self.reset_chat_settings(),
            MenuAction::SettingsAdvancedGraphics => {
                self.settings_advanced_graphics = !self.settings_advanced_graphics;
            }
            MenuAction::SettingsScale(offset) => {
                self.set_gui_scale_offset(offset);
                if self.settings_scale_picker {
                    self.settings_scale_picker = false;
                    self.settings_focus = vec![MenuAction::SettingsScalePicker];
                    self.focused = 0;
                }
            }
            MenuAction::SettingsScalePicker => {
                self.settings_dropdown = None;
                self.settings_scale_picker = !self.settings_scale_picker;
                self.settings_focus = vec![if self.settings_scale_picker {
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
                self.settings_slider_selected = None;
                self.settings_section = section;
                if section == crate::menu::settings_storage::SECTION_INDEX {
                    self.refresh_storage();
                }
                self.settings_dropdown = None;
                self.settings_scale_picker = false;
                self.key_remap = None;
            }
            MenuAction::SettingsOption(index, value) => {
                if self.settings_dropdown == Some(index) {
                    self.settings_focus = vec![MenuAction::SettingsDropdown(index)];
                    self.focused = 0;
                }
                self.set_option(index, value);
                self.settings_dropdown = None;
            }
            MenuAction::SettingsLanguage(index) => self.set_language(index),
            MenuAction::SettingsDropdown(index) => {
                self.settings_scale_picker = false;
                self.settings_dropdown = (self.settings_dropdown != Some(index)).then_some(index);
                self.settings_focus = vec![if self.settings_dropdown.is_some() {
                    MenuAction::SettingsOption(index, self.settings_options.get(usize::from(index)))
                } else {
                    MenuAction::SettingsDropdown(index)
                }];
                self.focused = 0;
            }
            MenuAction::SettingsResetGroup(group) => {
                self.dialog = Some(crate::menu::MenuDialog::SettingsResetGroup(group))
            }
            MenuAction::SettingsConfirmResetGroup(group) => self.confirm_settings_reset(group),
            MenuAction::SettingsResetBindings(gamepad) => {
                self.dialog = Some(crate::menu::MenuDialog::SettingsResetBindings(gamepad));
            }
            MenuAction::SettingsConfirmResetBindings(gamepad) => {
                if self.dialog != Some(crate::menu::MenuDialog::SettingsResetBindings(gamepad)) {
                    return;
                }
                self.dialog = None;
                Arc::make_mut(&mut self.settings_options).reset_bindings(gamepad);
                self.key_remap = None;
                self.settings_dirty = true;
                self.settings_apply = true;
            }
            MenuAction::SettingsKey(index) => self.key_remap = Some(index),
            MenuAction::SettingsResetKey(index) => {
                if Arc::make_mut(&mut self.settings_options).reset_key(usize::from(index)) {
                    self.settings_dirty = true;
                    self.settings_apply = true;
                } else {
                    self.message =
                        Some("The default key is assigned to another action.".to_owned());
                }
            }
            _ => {}
        }
    }

    /// Captures a desktop key or pointer button for the pending remap operation.
    pub(in crate::menu) fn capture_key(&mut self, control: semantic_input::PhysicalControl) {
        let Some(index) = self.key_remap.take() else {
            return;
        };
        if Arc::make_mut(&mut self.settings_options).remap(usize::from(index), control) {
            self.settings_dirty = true;
            self.settings_apply = true;
        } else {
            self.message = Some("That key is already assigned to another action.".to_owned());
        }
    }
}
