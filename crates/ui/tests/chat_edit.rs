use std::sync::Arc;

use ui::{
    ChatAutocompleteAction, ChatAutocompleteApply, ChatAutocompleteDelta, ChatAutocompleteState,
    ChatEditor, ChatEditorError, ChatHistory, ChatRateLimit, ChatSendError, ChatSendQueue,
    MAX_CHAT_AUTOCOMPLETE, MAX_CHAT_AUTOCOMPLETE_BYTES, MAX_CHAT_HISTORY, MAX_CHAT_INPUT_BYTES,
};

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
    let stale = state.begin_input(3, 1, "/gi", 3).unwrap();
    let current = state.begin_input(3, 2, "/give", 5).unwrap();
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
