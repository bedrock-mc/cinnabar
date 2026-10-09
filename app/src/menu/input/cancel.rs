//! Routes cancel input through selected controls before leaving the menu.

use super::*;

impl MenuRuntime {
    /// A selected vanilla edit box consumes cancel before the screen handles it.
    pub(super) fn go_back_from_input(&mut self) {
        if self.sign_in_focus().is_some() || self.join_request_prompted() {
            self.go_back();
            return;
        }
        if self.clear_settings_slider_selection() {
            return;
        }
        if self.screen == crate::menu::MenuScreen::Inbox
            && (self.feeds.inbox_state.opened.is_some()
                || self.feeds.inbox_state.delete_pending.is_some()
                || self.feeds.inbox_state.filters)
        {
            self.activate_inbox(crate::menu::inbox::Action::Cancel);
            return;
        }
        if self.screen == crate::menu::MenuScreen::AddServer && self.field.is_some() {
            self.edit_field(|editor| editor.place_cursor(editor.cursor_byte()));
            self.field = None;
            return;
        }
        self.go_back();
    }
}
