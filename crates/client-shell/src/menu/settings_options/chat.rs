//! Chat settings share the persisted option registry with the main settings screen.

use std::sync::Arc;

use crate::menu::MenuRuntime;
use launcher::menu::settings_options::{SettingsGroup, SettingsOptions};

impl MenuRuntime {
    /// Supplies a cheap settings snapshot even while the launcher menu is hidden.
    pub fn settings_snapshot(&self) -> (Arc<SettingsOptions>, Option<u16>) {
        (Arc::clone(&self.settings_options), self.settings.dropdown)
    }

    /// Restores the chat popup's registered options without changing other sections.
    pub(in crate::menu) fn reset_chat_settings(&mut self) {
        for option in SettingsGroup::Chat.options() {
            self.set_option(option.id.index() as u16, option.default);
        }
    }
}
