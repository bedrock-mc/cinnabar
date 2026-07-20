use std::sync::Arc;

use ui::{
    ChatApplyResult, ChatMessage, ChatMessageKind, ChatStore, MAX_CHAT_MESSAGES,
    MAX_CHAT_RETAINED_BYTES,
};

fn message(sequence: u64, text: impl Into<Arc<str>>) -> ChatMessage {
    ChatMessage {
        fifo_sequence: sequence,
        received_millis: sequence * 10,
        kind: ChatMessageKind::Chat,
        source: Some(Arc::from("player")),
        message: text.into(),
        parameters: Arc::from([]),
    }
}

#[test]
fn chat_retention_is_bounded_by_count_bytes_and_fifo_identity() {
    let mut store = ChatStore::default();
    for sequence in 1..=(MAX_CHAT_MESSAGES as u64 + 1) {
        assert!(matches!(
            store.push(message(sequence, "hello")),
            ChatApplyResult::Applied { .. }
        ));
    }
    assert_eq!(store.messages().len(), MAX_CHAT_MESSAGES);
    assert_eq!(store.messages().front().unwrap().fifo_sequence, 2);
    assert_eq!(
        store.push(message(MAX_CHAT_MESSAGES as u64 + 1, "duplicate")),
        ChatApplyResult::RejectedStaleSequence
    );

    let mut byte_capped = ChatStore::default();
    let oversized: Arc<str> = Arc::from("x".repeat(MAX_CHAT_RETAINED_BYTES));
    assert_eq!(
        byte_capped.push(message(1, oversized)),
        ChatApplyResult::RejectedTooLarge
    );
    assert!(byte_capped.messages().is_empty());
    assert_eq!(byte_capped.retained_bytes(), 0);
}

#[test]
fn chat_snapshots_are_deterministic_and_do_not_mutate_the_store() {
    let mut store = ChatStore::default();
    store.push(message(4, "first"));
    store.push(message(9, "second"));

    let first = store.view_nodes();
    let second = store.view_nodes();

    assert_eq!(first, second);
    assert_eq!(first[0].ordinal, 0);
    assert_eq!(first[0].source_sequence, 4);
    assert_eq!(first[1].text.as_ref(), "second");
    assert_eq!(store.messages().len(), 2);
}

#[test]
fn clear_releases_messages_bytes_and_sequence_identity() {
    let mut store = ChatStore::default();
    store.push(message(20, "old session"));
    store.clear();

    assert!(store.messages().is_empty());
    assert_eq!(store.retained_bytes(), 0);
    assert_eq!(store.last_sequence(), None);
    assert!(matches!(
        store.push(message(1, "new session")),
        ChatApplyResult::Applied { .. }
    ));
}
