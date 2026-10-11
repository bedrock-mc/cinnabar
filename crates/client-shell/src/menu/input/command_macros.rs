//! Command drafts stay editable while only valid commands reach persisted settings.
use super::fields::field_editor;
use crate::menu::MenuRuntime;
use launcher::menu::MenuField;
use launcher::menu::settings_options::{COMMAND_MACRO_COUNT, SettingsOptions};
use std::sync::Arc;

impl MenuRuntime {
    /// Saves a valid command edit while leaving invalid drafts available to correct.
    pub(super) fn sync_macro_command(&mut self, slot: usize) {
        let text = self.macro_commands[slot].as_str().to_owned();
        if Arc::make_mut(&mut self.settings_options).set_macro_command(slot, &text) {
            self.settings_dirty = true;
            self.settings_apply = true;
        }
    }
}

/// Seeded macro command boxes, so saved commands show before the first edit.
pub(in crate::menu) fn macro_editors(
    settings: &SettingsOptions,
) -> [ui::ChatEditor; COMMAND_MACRO_COUNT] {
    core::array::from_fn(|slot| {
        let mut editor = field_editor(MenuField::MacroCommand(slot));
        let command = settings.macro_command(slot);
        if !command.is_empty() {
            editor.set_text(command);
        }
        editor
    })
}
