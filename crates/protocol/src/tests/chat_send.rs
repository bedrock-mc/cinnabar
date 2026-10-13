use std::sync::Arc;

use bedrock_protocol_1_26_51::enums::TextPacketType;
use bedrock_protocol_1_26_51::packets::{self, PacketId};
use bedrock_protocol_1_26_51::types::TextData;
use protocol::{
    ChatAutocompleteAction, ChatAutocompleteCatalog, ChatAutocompleteEvent, ChatPacketError,
    chat_input_packet, chat_text_packet, decode_batch, encode,
};

#[test]
fn outbound_chat_uses_exact_authored_text_shape() {
    let packet = chat_text_packet("RustMCBE", "1234", "hello server").unwrap();
    let packets::Packet::Text(packet) = *packet.data else {
        panic!("expected text packet")
    };
    assert!(!packet.localize);
    // The union tag is the category byte gophertunnel derives from the message
    // type: TextTypeChat is written as TextCategoryAuthoredMessage.
    let TextData::AuthorAndMessage {
        message_type,
        player_name,
        message,
    } = packet.body
    else {
        panic!("expected authored chat body")
    };
    assert_eq!(message_type, TextPacketType::Chat);
    assert_eq!(player_name, "RustMCBE");
    assert_eq!(message, "hello server");
    assert_eq!(packet.sender_xuid, "1234");
    assert!(packet.platform_id.is_empty());
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
        chat_input_packet(&"x".repeat(257), "", "/transfer sm3"),
        Err(ChatPacketError::IdentityTooLong {
            field: "source_name",
            max: 256,
            ..
        })
    ));
}

/// Identity limits are the codec's own: values at the limit encode, one byte more is refused
/// before encoding.
#[test]
fn outbound_chat_identity_limits_match_the_codec() {
    let at_limit = chat_text_packet(&"n".repeat(256), &"1".repeat(64), "hi").unwrap();
    assert!(encode(&at_limit).is_ok());
    assert!(matches!(
        chat_text_packet(&"n".repeat(257), "", "hi"),
        Err(ChatPacketError::IdentityTooLong {
            field: "source_name",
            bytes: 257,
            max: 256,
        })
    ));
    assert!(matches!(
        chat_text_packet("player", &"1".repeat(65), "hi"),
        Err(ChatPacketError::IdentityTooLong {
            field: "xuid",
            bytes: 65,
            max: 64,
        })
    ));
}

#[test]
fn slash_input_round_trips_as_vanilla_player_command_request() {
    let built = chat_input_packet("RustMCBE", "1234", "/kill @s").unwrap();

    assert_eq!(built.header.id, PacketId::CommandRequest);
    let encoded = encode(&built).expect("encode command request");
    let mut decoded = decode_batch(encoded).expect("decode command request");
    assert_eq!(decoded.len(), 1);
    let packet = decoded.pop().unwrap();
    assert_eq!(packet.header.id, PacketId::CommandRequest);
    let packets::Packet::CommandRequest(packet) = *packet.data else {
        panic!("expected command request packet")
    };
    assert_eq!(packet.command, "/kill @s");
    assert_eq!(packet.origin.type_, "player");
    assert!(!packet.origin.uuid.is_nil());
    assert!(packet.origin.request_id.is_empty());
    assert_eq!(packet.origin.player_id.0, 0);
    assert!(!packet.is_internal);
    assert_eq!(packet.version, "latest");
}

#[test]
fn exact_fast_transfer_matches_the_gophertunnel_command_fixture() {
    let mut built = chat_input_packet("RustMCBE", "1234", "/transfer sm3").unwrap();
    let packets::Packet::CommandRequest(packet) = &mut *built.data else {
        panic!("expected command request packet")
    };
    packet.origin.uuid = uuid::Uuid::parse_str("00112233-4455-6677-8899-aabbccddeeff").unwrap();

    let encoded = encode(&built).expect("encode command request");
    let expected = [
        0xfe, 0x37, 0x4d, 0x0d, b'/', b't', b'r', b'a', b'n', b's', b'f', b'e', b'r', b' ', b's',
        b'm', b'3', 0x06, b'p', b'l', b'a', b'y', b'e', b'r', 0x77, 0x66, 0x55, 0x44, 0x33, 0x22,
        0x11, 0x00, 0xff, 0xee, 0xdd, 0xcc, 0xbb, 0xaa, 0x99, 0x88, 0x00, 0x00, 0x00, 0x00, 0x00,
        0x00, 0x00, 0x00, 0x00, 0x00, 0x06, b'l', b'a', b't', b'e', b's', b't',
    ];
    assert_eq!(encoded.as_ref(), expected);
}

#[test]
fn command_requests_receive_fresh_origin_uuids() {
    let first = chat_input_packet("RustMCBE", "1234", "/kill @s").unwrap();
    let second = chat_input_packet("RustMCBE", "1234", "/kill @s").unwrap();
    let packets::Packet::CommandRequest(first) = *first.data else {
        panic!("expected first command request packet")
    };
    let packets::Packet::CommandRequest(second) = *second.data else {
        panic!("expected second command request packet")
    };

    assert_ne!(first.origin.uuid, second.origin.uuid);
}

#[test]
fn every_transfer_command_spelling_uses_vanilla_command_request() {
    for input in [
        "/transfer",
        "/transfer sm3",
        "/TRANSFER sm3",
        "/TrAnSfEr\tsm3",
    ] {
        let packet = chat_input_packet("RustMCBE", "1234", input).unwrap();
        let packets::Packet::CommandRequest(packet) = *packet.data else {
            panic!("{input:?} must use CommandRequest")
        };
        assert_eq!(packet.command, input);
        assert_eq!(packet.origin.type_, "player");
        assert!(!packet.is_internal);
        assert_eq!(packet.version, "latest");
    }
}

#[test]
fn exact_fast_transfer_uses_vanilla_command_request() {
    let packet = chat_input_packet("RustMCBE", "1234", "/transfer sm3").unwrap();
    let packets::Packet::CommandRequest(packet) = *packet.data else {
        panic!("exact fast transfer must use CommandRequest")
    };
    assert_eq!(packet.command, "/transfer sm3");
    assert_eq!(packet.origin.type_, "player");
    assert!(!packet.origin.uuid.is_nil());
    assert!(!packet.is_internal);
    assert_eq!(packet.version, "latest");
}

#[test]
fn transfer_near_miss_remains_a_vanilla_command_request() {
    let packet = chat_input_packet("RustMCBE", "1234", "/transferred sm3").unwrap();
    let packets::Packet::CommandRequest(packet) = *packet.data else {
        panic!("near-miss command must remain CommandRequest")
    };
    assert_eq!(packet.command, "/transferred sm3");
}

#[test]
fn ordinary_chat_input_remains_an_authored_text_packet() {
    let packet = chat_input_packet("RustMCBE", "1234", "hello server").unwrap();
    let text_packet = chat_text_packet("RustMCBE", "1234", "hello server").unwrap();

    assert_eq!(packet.header.id, PacketId::Text);
    assert_eq!(packet, text_packet);
    let packets::Packet::Text(packet) = *packet.data else {
        panic!("expected text packet")
    };
    let TextData::AuthorAndMessage {
        message_type,
        player_name,
        message,
    } = packet.body
    else {
        panic!("expected authored chat body")
    };
    assert_eq!(message_type, TextPacketType::Chat);
    assert_eq!(player_name, "RustMCBE");
    assert_eq!(message, "hello server");
    assert_eq!(packet.sender_xuid, "1234");
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
