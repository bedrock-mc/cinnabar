use protocol::{ChatPacketError, chat_text_packet};
use valentine::bedrock::version::v1_26_30::{
    McpePacketData, TextPacketCategory, TextPacketContent, TextPacketType,
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
}
