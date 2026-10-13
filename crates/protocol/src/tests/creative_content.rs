use bedrock_protocol_1_26_51::packets;
use bytes::Bytes;

// Regenerated for protocol 2168 from gophertunnel at commit
// be6713da4dc051a4197f897d04835e89e9c54321:
//
//   packet.CreativeContent{Groups: []protocol.CreativeGroup{
//       {Category: protocol.CreativeCategoryConstruction},
//   }}
//
// One anonymous creative group whose icon is the empty item stack. Protocol
// 1001 short-circuited an empty icon after the network ID (prismarine modelled
// the icon as `ItemLegacy { network_id, content: Option<..> }` and skipped the
// body for network ID 0 *and* -1). gophertunnel's `Writer.Item`
// (minecraft/protocol/writer.go) always writes count, metadata, block runtime
// ID and a user-data byte slice, so the empty icon is now six bytes, not one.
// SHA-256: d534e5320e044ac17c37f455df953e0be2d8c7eb1c99ee13e546d653a42dfad7
const GOPHERTUNNEL_EMPTY_ICON_CREATIVE_CONTENT: &[u8] = &[
    0xfe, 0x0c, 0x91, 0x01, 0x01, 0x01, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00,
];

// Regenerated for protocol 2168 from gophertunnel at commit
// be6713da4dc051a4197f897d04835e89e9c54321:
//
//   packet.CreativeContent{Groups: []protocol.CreativeGroup{{
//       Category: protocol.CreativeCategoryItems,
//       Name:     "itemGroup.name.enchantedBook",
//       Icon: protocol.ItemStack{
//           ItemType: protocol.ItemType{NetworkID: 560},
//           Count:    1,
//           NBTData:  map[string]any{"ench": []any{map[string]any{
//               "id": int16(0), "lvl": int16(1)}}},
//       },
//   }}}
//
// The icon carries fixed-width little-endian item NBT inside the opaque
// user-data byte slice.
// SHA-256: fbca5c01ed0a485e193a35a3f4e2b027c282a49db173e9bf1a4a597e06c26ad1
const GOPHERTUNNEL_ENCHANTED_BOOK_CREATIVE_CONTENT: &[u8] = &[
    0xfe, 0x54, 0x91, 0x01, 0x01, 0x04, 0x1c, 0x69, 0x74, 0x65, 0x6d, 0x47, 0x72, 0x6f, 0x75, 0x70,
    0x2e, 0x6e, 0x61, 0x6d, 0x65, 0x2e, 0x65, 0x6e, 0x63, 0x68, 0x61, 0x6e, 0x74, 0x65, 0x64, 0x42,
    0x6f, 0x6f, 0x6b, 0xe0, 0x08, 0x01, 0x00, 0x00, 0x00, 0x2b, 0xff, 0xff, 0x01, 0x0a, 0x00, 0x00,
    0x09, 0x04, 0x00, 0x65, 0x6e, 0x63, 0x68, 0x0a, 0x01, 0x00, 0x00, 0x00, 0x02, 0x02, 0x00, 0x69,
    0x64, 0x00, 0x00, 0x02, 0x03, 0x00, 0x6c, 0x76, 0x6c, 0x01, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00,
    0x00, 0x00, 0x00, 0x00, 0x00, 0x00,
];

/// The exact item NBT the enchanted-book icon carries, as gophertunnel writes it.
///
/// Protocol 1001 exposed this through a typed `content` union, so the test could
/// assert `icon_item.content.is_some()`. 1.26.40 keeps the item user data as an
/// opaque buffer, so we pin the bytes instead - a strictly tighter assertion.
const ENCHANTED_BOOK_ICON_USER_DATA: &[u8] = &[
    0xff, 0xff, 0x01, 0x0a, 0x00, 0x00, 0x09, 0x04, 0x00, 0x65, 0x6e, 0x63, 0x68, 0x0a, 0x01, 0x00,
    0x00, 0x00, 0x02, 0x02, 0x00, 0x69, 0x64, 0x00, 0x00, 0x02, 0x03, 0x00, 0x6c, 0x76, 0x6c, 0x01,
    0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00,
];

fn raw_creative_content() -> crate::raw_packet::RawPacket {
    raw_creative_content_fixture(GOPHERTUNNEL_EMPTY_ICON_CREATIVE_CONTENT)
}

fn raw_creative_content_fixture(fixture: &'static [u8]) -> crate::raw_packet::RawPacket {
    crate::raw_packet::decode_batch(Bytes::from_static(fixture), 1024)
        .expect("raw batch decode")
        .into_iter()
        .next()
        .expect("one packet")
}

fn assert_enchanted_book_payload(data: &packets::Packet) {
    let packets::Packet::CreativeContent(content) = data else {
        panic!("expected CreativeContent, got {:?}", data.id());
    };
    assert_eq!(content.groups.len(), 1);
    assert_eq!(content.groups[0].name, "itemGroup.name.enchantedBook");
    assert_eq!(content.groups[0].group_icon_item.id.0, 560);
    assert_eq!(content.groups[0].group_icon_item.stack_size.0, 1);
    assert_eq!(
        content.groups[0].group_icon_item.user_data_buffer,
        ENCHANTED_BOOK_ICON_USER_DATA
    );
    // gophertunnel calls `entries` `Items`.
    assert!(content.entries.is_empty());
}

#[test]
fn pinned_gophertunnel_enchanted_book_nbt_decodes_and_round_trips_exactly() {
    let packet = raw_creative_content_fixture(GOPHERTUNNEL_ENCHANTED_BOOK_CREATIVE_CONTENT)
        .decode()
        .expect("owned enchanted-book CreativeContent decode");
    assert_enchanted_book_payload(&packet.data);

    let encoded = crate::encode(&packet).expect("re-encode");
    assert_eq!(
        encoded.as_ref(),
        GOPHERTUNNEL_ENCHANTED_BOOK_CREATIVE_CONTENT
    );
}

fn assert_empty_icon_payload(data: &packets::Packet) {
    let packets::Packet::CreativeContent(packet) = data else {
        panic!("expected CreativeContent, got {:?}", data.id());
    };
    assert_eq!(packet.groups.len(), 1);
    // The empty item stack is network ID 0 with an empty user-data buffer. The
    // protocol-1001 `-1` sentinel was a prismarine modelling artefact of
    // `ItemLegacy`, which skipped the item body for both 0 and -1; gophertunnel
    // only treats `NetworkID == 0` as absent (`Writer.Item` ->
    // `itemUserData(..., present: x.NetworkID != 0, ...)`).
    assert_eq!(packet.groups[0].group_icon_item.id.0, 0);
    assert_eq!(packet.groups[0].group_icon_item.stack_size.0, 0);
    assert!(packet.groups[0].group_icon_item.user_data_buffer.is_empty());
    assert!(packet.entries.is_empty());
}

#[test]
fn pinned_gophertunnel_empty_icon_decodes_and_round_trips_exactly() {
    let packet = raw_creative_content()
        .decode()
        .expect("owned CreativeContent decode");
    assert_empty_icon_payload(&packet.data);
    let encoded = crate::encode(&packet).expect("re-encode");
    assert_eq!(encoded.as_ref(), GOPHERTUNNEL_EMPTY_ICON_CREATIVE_CONTENT);
}
