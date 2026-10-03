//! Command-suggestion servicing, acceptance and tab cycling for the chat input.

use std::sync::Arc;

use protocol::CompletionContext;
use ui::{
    ChatAutocompleteAction, ChatAutocompleteApply, ChatAutocompleteDelta, ChatAutocompleteRequest,
    ChatAutocompleteResponse, MAX_CHAT_INPUT_BYTES, PointerPhase, UiAction,
};

use super::UiRuntime;

impl UiRuntime {
    /// Completes `request` against the catalog snapshot; false when it was stale or invalid.
    pub fn complete_chat_autocomplete(
        &mut self,
        player_runtime: &crate::player_runtime::PlayerRuntime,
        request: ChatAutocompleteRequest,
    ) -> bool {
        // UpdateSoftEnum and AvailableCommands arrive unsolicited without an editor request
        // identity, so the immutable catalog is queried locally and applied only through the
        // session/input/request correlation below.
        let command_permission = self
            .local_abilities(player_runtime)
            .map(|update| update.command_permission);
        let context = CompletionContext {
            players: &self.known_player_names,
            command_permission,
        };
        let Ok(completion) = self.chat_autocomplete_catalog.complete_in(
            &request.input,
            usize::from(request.cursor_byte),
            context,
        ) else {
            return false;
        };
        let usage = completion.usage.clone();
        let applied = matches!(
            self.chat_autocomplete
                .apply_response(ChatAutocompleteResponse {
                    session: request.session,
                    input_revision: request.input_revision,
                    request_id: request.request_id,
                    catalog_revision: completion.catalog_revision,
                    suggestions: completion.suggestions,
                }),
            Ok(ChatAutocompleteApply::Applied)
        );
        if applied {
            self.chat_usage_hint = usage;
        }
        applied
    }

    pub fn handle_chat_ui_action(&mut self, action: UiAction) -> bool {
        if matches!(action, UiAction::TabNext | UiAction::TabPrevious) {
            return self.cycle_chat_suggestion(action);
        }
        let Some(suggestion) = self.chat_autocomplete.handle_action(action) else {
            return false;
        };
        self.replace_chat_token(&suggestion);
        true
    }

    pub fn handle_chat_ui_action_with_suggestion_hit(
        &mut self,
        action: UiAction,
        suggestion_hit: Option<usize>,
    ) -> bool {
        if let UiAction::PointerPrimary {
            position: _,
            phase: PointerPhase::Pressed,
        } = action
        {
            let Some(index) = suggestion_hit else {
                return false;
            };
            if !self.chat_autocomplete.select_index(index) {
                return false;
            }
            let Some(suggestion) = self.chat_autocomplete.selected_suggestion() else {
                return false;
            };
            self.replace_chat_token(&suggestion);
            return true;
        }
        self.handle_chat_ui_action(action)
    }

    /// Tab inserts the selected suggestion; repeated Tab steps through the same list.
    fn cycle_chat_suggestion(&mut self, action: UiAction) -> bool {
        if self.chat_autocomplete.suggestions().is_empty() {
            return false;
        }
        if self.chat_tab_cycling {
            self.chat_autocomplete.handle_action(action);
        }
        let Some(selected) = self.chat_autocomplete.selected_suggestion() else {
            return false;
        };
        let list = self.chat_autocomplete.suggestions().to_vec();
        let index = self.chat_autocomplete.selected_index();
        let hint = self.chat_usage_hint.clone();
        let start = self.chat_token_start();
        self.replace_chat_token(&selected);
        // The edit re-arms completion; restoring the list keeps the cycle stable.
        if let Some(request) = self.pending_chat_autocomplete_request.take() {
            let restored = self.chat_autocomplete.apply(
                request,
                ChatAutocompleteDelta {
                    enum_name: Arc::from("catalog"),
                    action: ChatAutocompleteAction::Replace,
                    suggestions: Arc::from(list),
                },
            );
            if restored.is_ok()
                && let Some(index) = index
            {
                self.chat_autocomplete.select_index(index);
            }
        }
        self.chat_usage_hint = hint;
        self.chat_tab_cycling = true;
        self.chat_tab_start = Some(start);
        true
    }

    /// Keeps the original replacement boundary while Tab cycles a suggestion list.
    fn chat_token_start(&self) -> usize {
        if self.chat_tab_cycling
            && let Some(start) = self.chat_tab_start
        {
            return start;
        }
        let head = &self.chat_editor.as_str()[..self.chat_editor.cursor_byte()];
        head.rfind(char::is_whitespace).map_or(0, |at| {
            at + head[at..].chars().next().map_or(1, char::len_utf8)
        })
    }

    /// Replaces the whitespace-delimited token ending at the cursor.
    fn replace_chat_token(&mut self, value: &str) {
        let text = self.chat_editor.as_str();
        let cursor = self.chat_editor.cursor_byte();
        let head = &text[..cursor];
        let start = self.chat_token_start();
        let tail = &text[cursor..];
        if start + value.len() + tail.len() > MAX_CHAT_INPUT_BYTES {
            return;
        }
        let next = format!("{}{value}{tail}", &head[..start]);
        if next == text {
            return;
        }
        let tail_chars = tail.chars().count();
        self.chat_editor.clear();
        self.chat_editor
            .insert(&next)
            .expect("length was checked against the chat input bound");
        for _ in 0..tail_chars {
            self.chat_editor.move_left();
        }
        self.note_chat_editor_change();
    }
}
