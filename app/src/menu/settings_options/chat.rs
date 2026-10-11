//! Chat settings share the persisted option registry with the main settings screen.

use std::sync::Arc;

use crate::menu::MenuRuntime;
use launcher::menu::settings_options::{SETTINGS_OPTIONS, SettingsOptions};

impl MenuRuntime {
    /// Supplies a cheap settings snapshot even while the launcher menu is hidden.
    pub(crate) fn settings_snapshot(&self) -> (Arc<SettingsOptions>, Option<u16>) {
        (Arc::clone(&self.settings_options), self.settings_dropdown)
    }

    /// Restores the chat popup's registered options without changing other sections.
    pub(in crate::menu) fn reset_chat_settings(&mut self) {
        for (index, option) in SETTINGS_OPTIONS.iter().enumerate() {
            if matches!(
                option.name,
                "hide_chat"
                    | "toggle_emote_chat"
                    | "toggle_tts"
                    | "chat_typeface"
                    | "chat_font_size"
                    | "chat_line_spacing"
                    | "chat_color"
                    | "mentions_color"
            ) {
                self.set_option(index as u16, option.default);
            }
        }
    }
}
