use std::sync::Arc;

use protocol::{RawTextEvent, TextCategory, TextEvent, TextKind, UiEvent, parse_raw_text};

/// Builds a raw-text event for the requested packet surface.
pub(super) fn literal_raw_text(kind: TextKind, json: &str) -> UiEvent {
    let document = parse_raw_text(json).unwrap();
    UiEvent::RawText(RawTextEvent {
        text: TextEvent {
            category: TextCategory::MessageOnly,
            kind,
            needs_translation: false,
            source: None,
            message: Arc::from(document.literal_text()),
            parameters: Arc::from([]),
            xuid: Arc::from(""),
            platform_chat_id: Arc::from(""),
            filtered_message: None,
        },
        document,
    })
}

/// Builds a chat message from a named player.
pub(super) fn text(message: &str) -> UiEvent {
    UiEvent::Text(TextEvent {
        category: TextCategory::MessageOnly,
        kind: TextKind::Chat,
        needs_translation: false,
        source: Some(Arc::from("player")),
        message: Arc::from(message),
        parameters: Arc::from([]),
        xuid: Arc::from(""),
        platform_chat_id: Arc::from(""),
        filtered_message: None,
    })
}
