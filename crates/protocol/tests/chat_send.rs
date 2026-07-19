use std::sync::Arc;

use protocol::{
    BedrockSession, ChatAutocompleteAction, ChatAutocompleteCatalog, ChatAutocompleteEvent,
    ChatPacketError, chat_input_packet, chat_text_packet, decode_batch, encode,
};
use valentine::bedrock::version::v1_26_30::{
    McpePacketData, McpePacketName, TextPacketCategory, TextPacketContent, TextPacketType,
};

#[test]
fn outbound_chat_uses_exact_authored_text_shape() {
    let packet = chat_text_packet("RustMCBE", "1234", "hello server").unwrap();
    let McpePacketData::PacketText(packet) = packet.data else {
        panic!("expected text packet")
    };
    assert!(!packet.needs_translation);
    assert_eq!(packet.category, TextPacketCategory::Authored);
    assert_eq!(packet.type_, TextPacketType::Chat);
    let Some(TextPacketContent::Chat(content)) = packet.content else {
        panic!("expected authored chat content")
    };
    assert_eq!(content.source_name, "RustMCBE");
    assert_eq!(content.message, "hello server");
    assert_eq!(packet.xuid, "1234");
    assert!(packet.platform_chat_id.is_empty());
    assert!(packet.filtered_message.is_none());
}

#[test]
fn outbound_chat_rejects_empty_and_oversized_fields() {
    assert_eq!(
        chat_text_packet("player", "", ""),
        Err(ChatPacketError::EmptyMessage)
    );
    assert!(matches!(
        chat_text_packet("player", "", &"x".repeat(513)),
        Err(ChatPacketError::MessageTooLong { .. })
    ));
    assert!(matches!(
        chat_input_packet(&"x".repeat(16_385), "", "/transfer sm3"),
        Err(ChatPacketError::IdentityTooLong {
            field: "source_name",
            ..
        })
    ));
}

#[test]
fn slash_input_round_trips_as_vanilla_player_command_request() {
    let session = BedrockSession { shield_item_id: 0 };
    let built = chat_input_packet("RustMCBE", "1234", "/transfer sm3").unwrap();

    assert_eq!(built.header.id, McpePacketName::PacketCommandRequest);
    let encoded = encode(&built, &session).expect("encode command request");
    let mut decoded = decode_batch(encoded, &session).expect("decode command request");
    assert_eq!(decoded.len(), 1);
    let packet = decoded.pop().unwrap();
    assert_eq!(packet.header.id, McpePacketName::PacketCommandRequest);
    let McpePacketData::PacketCommandRequest(packet) = packet.data else {
        panic!("expected command request packet")
    };
    assert_eq!(packet.command, "/transfer sm3");
    assert_eq!(packet.origin.type_, "player");
    assert!(!packet.origin.uuid.is_nil());
    assert!(packet.origin.request_id.is_empty());
    assert_eq!(packet.origin.player_entity_id, 0);
    assert!(!packet.internal);
    assert_eq!(packet.version, "latest");
}

#[test]
fn command_requests_receive_fresh_origin_uuids() {
    let first = chat_input_packet("RustMCBE", "1234", "/transfer sm3").unwrap();
    let second = chat_input_packet("RustMCBE", "1234", "/transfer sm3").unwrap();
    let McpePacketData::PacketCommandRequest(first) = first.data else {
        panic!("expected first command request packet")
    };
    let McpePacketData::PacketCommandRequest(second) = second.data else {
        panic!("expected second command request packet")
    };

    assert_ne!(first.origin.uuid, second.origin.uuid);
}

#[test]
fn ordinary_chat_input_remains_an_authored_text_packet() {
    let packet = chat_input_packet("RustMCBE", "1234", "hello server").unwrap();
    let text_packet = chat_text_packet("RustMCBE", "1234", "hello server").unwrap();

    assert_eq!(packet.header.id, McpePacketName::PacketText);
    assert_eq!(packet, text_packet);
    let McpePacketData::PacketText(packet) = packet.data else {
        panic!("expected text packet")
    };
    let Some(TextPacketContent::Chat(content)) = packet.content else {
        panic!("expected authored chat content")
    };
    assert_eq!(content.source_name, "RustMCBE");
    assert_eq!(content.message, "hello server");
    assert_eq!(packet.xuid, "1234");
}

#[test]
fn autocomplete_catalog_is_revisioned_and_queries_an_immutable_snapshot() {
    let mut catalog = ChatAutocompleteCatalog::default();
    let revision = catalog
        .apply(ChatAutocompleteEvent {
            enum_name: Arc::from("commands"),
            action: ChatAutocompleteAction::Replace,
            suggestions: Arc::from([Arc::from("/give"), Arc::from("/gamerule")]),
        })
        .unwrap();

    let completion = catalog.complete("/gi", 3).unwrap();

    assert_eq!(completion.catalog_revision, revision);
    assert_eq!(completion.suggestions.as_ref(), [Arc::from("/give")]);
}

#[test]
fn unrelated_soft_enum_updates_do_not_pretend_to_be_editor_responses() {
    let mut catalog = ChatAutocompleteCatalog::default();
    catalog
        .apply(ChatAutocompleteEvent {
            enum_name: Arc::from("colors"),
            action: ChatAutocompleteAction::Replace,
            suggestions: Arc::from([Arc::from("green")]),
        })
        .unwrap();

    let completion = catalog.complete("/gi", 3).unwrap();

    assert!(completion.suggestions.is_empty());
}
