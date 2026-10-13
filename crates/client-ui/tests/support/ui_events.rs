use std::sync::Arc;

use protocol::UiEvent;

use super::SequencedUiEvent;

/// Wraps a UI event in ordered session metadata.
pub(super) fn envelope(session_id: u64, sequence: u64, event: UiEvent) -> SequencedUiEvent {
    SequencedUiEvent {
        session_id,
        fifo_sequence: sequence,
        local_millis: sequence * 10,
        server_tick: None,
        event,
    }
}

/// Builds a translated text packet for the localization fixtures.
pub(super) fn raw_text_event(json: &str) -> protocol::UiEvent {
    protocol::UiEvent::RawText(protocol::RawTextEvent {
        text: protocol::TextEvent {
            category: protocol::TextCategory::MessageOnly,
            kind: protocol::TextKind::Raw,
            needs_translation: false,
            source: None,
            message: std::sync::Arc::from(""),
            parameters: std::sync::Arc::from([]),
            xuid: std::sync::Arc::from(""),
            platform_chat_id: std::sync::Arc::from(""),
            filtered_message: None,
        },
        document: protocol::parse_raw_text(json).unwrap(),
    })
}

/// Builds a command tree with enum and player-target parameters.
pub(super) fn warp_tree() -> protocol::CommandTreeEvent {
    let param = |name: &str, kind| protocol::CommandParam {
        name: Arc::from(name),
        optional: false,
        collapse_enum: false,
        kind,
    };
    let mode = protocol::CommandParamKind::Enum {
        name: Arc::from("Mode"),
        values: Arc::from([Arc::from("fast"), Arc::from("far"), Arc::from("slow")]),
    };
    protocol::CommandTreeEvent {
        commands: Arc::from([protocol::CommandSpec {
            name: Arc::from("warp"),
            aliases: Arc::from([]),
            permission: 0,
            overloads: Arc::from([Arc::from([
                param("mode", mode),
                param("who", protocol::CommandParamKind::Target),
            ])]),
        }]),
        soft_enums: Arc::from([]),
    }
}
