use std::sync::Arc;

use protocol::{
    ChatAutocompleteAction, ChatAutocompleteCatalog, ChatAutocompleteEvent, ChatPacketError,
    chat_text_packet,
};
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
