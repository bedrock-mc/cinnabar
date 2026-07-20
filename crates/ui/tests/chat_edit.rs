use std::sync::Arc;

use ui::{
    ChatAutocompleteAction, ChatAutocompleteApply, ChatAutocompleteDelta, ChatAutocompleteResponse,
    ChatAutocompleteState, ChatClipboard, ChatEditor, ChatEditorError, ChatHistory, ChatPasteError,
    ChatRateLimit, ChatSendError, ChatSendQueue, MAX_CHAT_AUTOCOMPLETE,
    MAX_CHAT_AUTOCOMPLETE_BYTES, MAX_CHAT_HISTORY, MAX_CHAT_INPUT_BYTES, UiAction,
};

#[derive(Default)]
struct ClipboardFixture {
    requested_maximum: Option<usize>,
    value: Option<Arc<str>>,
}

impl ChatClipboard for ClipboardFixture {
    type Error = ();

    fn read_text_bounded(&mut self, maximum_bytes: usize) -> Result<Option<Arc<str>>, Self::Error> {
        self.requested_maximum = Some(maximum_bytes);
        Ok(self.value.take())
    }
}

#[test]
fn editor_never_splits_utf8_and_caps_clipboard_before_commit() {
    let mut editor = ChatEditor::new(16).unwrap();
    editor.insert("a🙂bc").unwrap();
    editor.move_left();
    editor.backspace();
    assert_eq!(editor.as_str(), "a🙂c");
    assert!(core::str::from_utf8(editor.bytes()).is_ok());

    let before = editor.clone();
    assert_eq!(
        editor.insert("0123456789abcdef"),
        Err(ChatEditorError::InputTooLong { maximum: 16 })
    );
    assert_eq!(editor, before);
}

#[test]
fn selection_replacement_and_deletion_use_character_boundaries() {
    let mut editor = ChatEditor::new(MAX_CHAT_INPUT_BYTES).unwrap();
    editor.insert("one🙂two").unwrap();
    editor.select_left();
    editor.select_left();
    editor.select_left();
    editor.insert("2").unwrap();
    assert_eq!(editor.as_str(), "one🙂2");

    editor.select_left();
    editor.select_left();
    editor.backspace();
    assert_eq!(editor.as_str(), "one");
    assert!(editor.selection().is_none());
}

#[test]
fn clipboard_is_bounded_by_remaining_capacity_before_read() {
    let mut editor = ChatEditor::new(8).unwrap();
    editor.insert("abc").unwrap();
    let mut clipboard = ClipboardFixture {
        value: Some(Arc::from("12345")),
        ..ClipboardFixture::default()
    };

    editor.paste_from(&mut clipboard).unwrap();

    assert_eq!(clipboard.requested_maximum, Some(5));
    assert_eq!(editor.as_str(), "abc12345");

    let mut invalid = ClipboardFixture {
        value: Some(Arc::from("x")),
        ..ClipboardFixture::default()
    };
    assert_eq!(
        editor.paste_from(&mut invalid),
        Err(ChatPasteError::AdapterExceededBound {
            maximum: 0,
            actual: 1,
        })
    );
    assert_eq!(editor.as_str(), "abc12345");
}

#[test]
fn history_is_bounded_and_consecutive_duplicates_coalesce() {
    let mut history = ChatHistory::default();
    history.push(Arc::from("same"));
    history.push(Arc::from("same"));
    for index in 0..MAX_CHAT_HISTORY + 5 {
        history.push(Arc::from(format!("message-{index}")));
    }
    assert_eq!(history.entries().len(), MAX_CHAT_HISTORY);
    assert_eq!(history.entries().back().unwrap().as_ref(), "message-132");
    assert_eq!(history.older().unwrap().as_ref(), "message-132");
    assert_eq!(history.newer(), None);
}

#[test]
fn sends_preserve_fifo_rate_limit_and_backpressure() {
    let mut queue = ChatSendQueue::new(4, ChatRateLimit::new(2, 1_000).unwrap()).unwrap();
    assert_eq!(queue.push(7, "one", 100).unwrap().sequence, 0);
    assert_eq!(queue.push(7, "two", 100).unwrap().sequence, 1);
    assert_eq!(queue.push(7, "three", 100), Err(ChatSendError::RateLimited));
    assert_eq!(queue.pending().len(), 2);
    assert_eq!(queue.pending()[0].message.as_ref(), "one");

    assert!(!queue.confirm_front(1));
    assert_eq!(queue.pending()[0].sequence, 0);
    assert!(queue.confirm_front(0));
    assert_eq!(queue.pending()[0].sequence, 1);

    assert_eq!(queue.push(7, "three", 1_100).unwrap().sequence, 2);
}

#[test]
fn session_change_drops_old_unsent_messages_and_resets_sequence() {
    let mut queue = ChatSendQueue::new(4, ChatRateLimit::new(4, 1_000).unwrap()).unwrap();
    queue.push(1, "old-a", 0).unwrap();
    queue.push(1, "old-b", 0).unwrap();

    assert_eq!(queue.begin_session(2), 2);
    assert!(queue.pending().is_empty());
    assert_eq!(queue.push(2, "new", 0).unwrap().sequence, 0);
}

#[test]
fn autocomplete_applies_only_current_input_and_stays_bounded() {
    let mut state = ChatAutocompleteState::default();
    let stale = state.begin_input(3, 1, "/gi", 3).unwrap().unwrap();
    let current = state.begin_input(3, 2, "/give", 5).unwrap().unwrap();
    assert_eq!(current.input.as_ref(), "/give");
    assert_eq!(state.begin_input(3, 2, "/give", 5).unwrap(), None);
    let update = ChatAutocompleteDelta {
        enum_name: Arc::from("commands"),
        action: ChatAutocompleteAction::Replace,
        suggestions: Arc::from([Arc::from("/give"), Arc::from("/gamerule")]),
    };

    assert_eq!(
        state.apply(stale, update.clone()).unwrap(),
        ChatAutocompleteApply::IgnoredStaleInput
    );
    assert_eq!(
        state.apply(current, update).unwrap(),
        ChatAutocompleteApply::Applied
    );
    assert_eq!(
        state
            .suggestions()
            .iter()
            .map(AsRef::as_ref)
            .collect::<Vec<&str>>(),
        ["/give", "/gamerule"]
    );
    assert!(state.suggestions().len() <= MAX_CHAT_AUTOCOMPLETE);
    assert!(state.retained_bytes() <= MAX_CHAT_AUTOCOMPLETE_BYTES);
}

#[test]
fn autocomplete_selection_is_driven_by_semantic_ui_actions() {
    let mut state = ChatAutocompleteState::default();
    let request = state.begin_input(7, 1, "/g", 2).unwrap().unwrap();
    state
        .apply(
            request,
            ChatAutocompleteDelta {
                enum_name: Arc::from("commands"),
                action: ChatAutocompleteAction::Replace,
                suggestions: Arc::from([Arc::from("/give"), Arc::from("/gamemode")]),
            },
        )
        .unwrap();

    assert_eq!(state.selected_index(), Some(0));
    assert_eq!(state.handle_action(UiAction::Navigate([0, 1])), None);
    assert_eq!(state.selected_index(), Some(1));
    assert_eq!(
        state.handle_action(UiAction::Accept).as_deref(),
        Some("/gamemode")
    );
    assert_eq!(state.handle_action(UiAction::TabNext), None);
    assert_eq!(state.selected_index(), Some(0));
}

#[test]
fn autocomplete_session_replacement_clears_request_and_suggestions() {
    let mut state = ChatAutocompleteState::default();
    let request = state.begin_input(7, 1, "/g", 2).unwrap().unwrap();
    state
        .apply(
            request,
            ChatAutocompleteDelta {
                enum_name: Arc::from("commands"),
                action: ChatAutocompleteAction::Replace,
                suggestions: Arc::from([Arc::from("/give")]),
            },
        )
        .unwrap();

    state.begin_session(8);

    assert!(state.active_request().is_none());
    assert!(state.suggestions().is_empty());
    assert!(state.begin_input(8, 1, "/help", 5).unwrap().is_some());
}

#[test]
fn autocomplete_response_requires_exact_request_and_input_revision() {
    let mut state = ChatAutocompleteState::default();
    let request = state.begin_input(7, 4, "/g", 2).unwrap().unwrap();
    let response = ChatAutocompleteResponse {
        session: request.session,
        input_revision: request.input_revision,
        request_id: request.request_id,
        catalog_revision: 9,
        suggestions: Arc::from([Arc::from("/give")]),
    };
    let mut stale = response.clone();
    stale.request_id += 1;

    assert_eq!(
        state.apply_response(stale).unwrap(),
        ChatAutocompleteApply::IgnoredStaleInput
    );
    assert!(state.suggestions().is_empty());
    assert_eq!(
        state.apply_response(response).unwrap(),
        ChatAutocompleteApply::Applied
    );
    assert_eq!(state.suggestions(), [Arc::from("/give")]);
}
