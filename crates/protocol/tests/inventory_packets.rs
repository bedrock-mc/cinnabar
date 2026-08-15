use bytes::Bytes;
use protocol::{
    decode_batch, encode, into_world_event, BedrockSession, NetworkItemStack, WorldEvent,
};
use protocol::{
    normalize_authority, normalize_container_close, normalize_container_data,
    normalize_container_open, normalize_content, normalize_hotbar, normalize_response,
    normalize_slot, validate_item_nbt_size, ContainerIdentity, InventoryAuthority, InventoryEvent,
    InventoryPacketError, VerifiedNetworkItemStack, MAX_CONTAINER_SLOTS, MAX_ITEM_NBT_BYTES,
    MAX_RESPONSE_CONTAINERS, MAX_STACK_RESPONSES,
};
use sha2::{Digest, Sha256};
use std::sync::Arc;
use valentine::bedrock::version::v1_26_40::{
    ActorUniqueId, BedrockSafetyRedactableString, BedrockSafetyRedactableStringView, BlockPos,
    CerealizerNetworkItemStackDescriptorSerializedData as ItemStackDescriptor,
    ContainerClosePacket, ContainerOpenPacket, ContainerSetDataPacket,
    EnumsContainerEnumName as FullContainerNameContainerName,
    EnumsItemStackNetResult as ItemStackResponseInfoResult, FullContainerName,
    InventoryContentPacket, InventorySlotPacket, ItemStackResponseContainerInfo,
    ItemStackResponseInfo, ItemStackResponsePacket, ItemStackResponseSlotInfo, McpePacketData,
    PlayerHotbarPacket, StructureEditorData, StructureEditorDataView,
    TypedClientNetIdstructItemStackRequestIdTagint32T0,
    TypedServerNetIdstructItemStackNetIdTagint32T0,
};
use valentine::bedrock::{codec::BedrockCodec, error::DecodeError};

const CONTENT_FIXTURE: &[u8] = include_bytes!("../fixtures/inventory_content.bin");
const SLOT_FIXTURE: &[u8] = include_bytes!("../fixtures/inventory_slot.bin");
const HOTBAR_FIXTURE: &[u8] = include_bytes!("../fixtures/player_hotbar.bin");
const RESPONSE_FIXTURE: &[u8] = include_bytes!("../fixtures/item_stack_response.bin");

/// `CONTAINER_ID_INVENTORY`. 1.26.40 carries raw container IDs rather than the
/// named `WindowId` / `WindowIdVarint` enums protocol 1001 modelled.
const INVENTORY_CONTAINER: u8 = 0;
/// `CONTAINER_ID_FIRST`, the first server-assigned container window.
const FIRST_CONTAINER: u8 = 1;
/// `ContainerType::Container`, the wire code the named `WindowType` enum used.
const CONTAINER_TYPE: u8 = 0;

/// Builds an item user-data buffer that carries no NBT compound.
///
/// The three protocol-1001 item encodings collapse into one descriptor whose
/// trailing user data is an opaque length-prefixed buffer, so fixtures spell the
/// layout out instead of naming interior fields. gophertunnel
/// be6713da4dc051a4197f897d04835e89e9c54321 `minecraft/protocol/writer.go`
/// `Writer.itemUserData`: an `int16` of 0 means "no compound", then `canPlaceOn`
/// and `canBreak` as `uint32`-counted lists of `StringUTF` (an `int16` length
/// prefix). The shield blocking tick is written only for shield items.
fn item_user_data(can_place_on: &[&str]) -> Vec<u8> {
    let mut buffer = 0i16.to_le_bytes().to_vec();
    buffer.extend((can_place_on.len() as u32).to_le_bytes());
    for identifier in can_place_on {
        buffer.extend((identifier.len() as i16).to_le_bytes());
        buffer.extend_from_slice(identifier.as_bytes());
    }
    buffer.extend(0u32.to_le_bytes());
    buffer
}

/// One item descriptor. Protocol 1001 needed an `ItemV4` and an `ItemNew`
/// builder here because the schema modelled the content and slot encodings
/// separately; 1.26.40 puts the same descriptor in both places.
fn item(id: i16, stacksize: u16, stack_network_id: i32, user_data: Vec<u8>) -> ItemStackDescriptor {
    ItemStackDescriptor {
        id,
        stacksize,
        auxvalue: u32::from_ne_bytes((-2i32).to_ne_bytes()),
        net_id_variant: Some(stack_network_id),
        block_runtime_id: 91,
        user_data_buffer: user_data,
    }
}

fn full_container(
    container_name: FullContainerNameContainerName,
    dynamic_id: Option<u32>,
) -> FullContainerName {
    FullContainerName {
        container_name,
        dynamic_id,
    }
}

/// One response slot. `constant_3` is the always-true outer flag of the
/// double-optional the stack net ID is written behind: gophertunnel
/// be6713da4dc051a4197f897d04835e89e9c54321 `minecraft/protocol/io.go`
/// `DoubleOptionalFunc` writes `outer := true` and then the inner presence bool.
/// The two protocol-1001 name fields are now the unredacted and redacted halves
/// of one redactable string (`minecraft/protocol/item_stack.go` writes
/// `CustomName` then `FilteredCustomName`).
fn response_slot(
    slot: u8,
    amount: u8,
    item_stack_id: i32,
    custom_name: &str,
    filtered_custom_name: &str,
    durability_correction: i32,
) -> ItemStackResponseSlotInfo {
    ItemStackResponseSlotInfo {
        requested_slot: slot,
        slot,
        amount,
        item_stack_net_id: Some(Some(TypedServerNetIdstructItemStackNetIdTagint32T0 {
            id: item_stack_id,
        })),
        custom_name: BedrockSafetyRedactableString {
            unredacted: custom_name.to_owned(),
            redacted: filtered_custom_name.to_owned(),
        },
        durability_correction,
    }
}

/// One accepted response. `constant_2` is the outer flag of the same
/// double-optional wrapping the container list.
fn accepted_response(
    request_id: i32,
    containers: Vec<ItemStackResponseContainerInfo>,
) -> ItemStackResponseInfo {
    ItemStackResponseInfo {
        result: ItemStackResponseInfoResult::Success,
        client_request_id: TypedClientNetIdstructItemStackRequestIdTagint32T0 { id: request_id },
        containers: Some(Some(containers)),
    }
}

fn decode_fixture(bytes: &'static [u8]) -> protocol::Packet {
    let mut packets = decode_batch(bytes.into(), &BedrockSession { shield_item_id: 0 })
        .expect("decode pinned inventory fixture");
    assert_eq!(packets.len(), 1);
    packets.pop().unwrap()
}

#[test]
fn pinned_gophertunnel_inventory_fixtures_normalize_without_vendor_types() {
    let content = match decode_fixture(CONTENT_FIXTURE).data {
        McpePacketData::InventoryContentPacket(packet) => normalize_content(*packet).unwrap(),
        other => panic!("expected InventoryContent, got {other:?}"),
    };
    assert!(matches!(content, InventoryEvent::Content(_)));

    let slot = match decode_fixture(SLOT_FIXTURE).data {
        McpePacketData::InventorySlotPacket(packet) => normalize_slot(*packet).unwrap(),
        other => panic!("expected InventorySlot, got {other:?}"),
    };
    assert!(matches!(slot, InventoryEvent::Slot(_)));

    let hotbar = match decode_fixture(HOTBAR_FIXTURE).data {
        McpePacketData::PlayerHotbarPacket(packet) => normalize_hotbar(packet).unwrap(),
        other => panic!("expected PlayerHotbar, got {other:?}"),
    };
    assert!(matches!(hotbar, InventoryEvent::SelectedSlot(_)));

    let response = match decode_fixture(RESPONSE_FIXTURE).data {
        McpePacketData::ItemStackResponsePacket(packet) => normalize_response(packet).unwrap(),
        other => panic!("expected ItemStackResponse, got {other:?}"),
    };
    assert!(matches!(response, InventoryEvent::Response(_)));
}

#[test]
fn inventory_packets_dispatch_through_the_public_world_event_surface() {
    for bytes in [
        CONTENT_FIXTURE,
        SLOT_FIXTURE,
        HOTBAR_FIXTURE,
        RESPONSE_FIXTURE,
    ] {
        let event = into_world_event(decode_fixture(bytes), 0)
            .expect("normalize inventory world event")
            .expect("inventory packet must be allowlisted");
        assert!(matches!(event, WorldEvent::Inventory(_)));
    }
}

/// Pins gophertunnel's two adjacent strings for the generated redactable type.
///
/// gophertunnel's `StackResponseSlotInfo.Marshal`
/// (`minecraft/protocol/item_stack.go` @ 9f42f3679a573fc4b51104569cc4f422036e28ec)
/// writes `CustomName` and `FilteredCustomName` as two ordinary adjacent strings.
#[test]
fn item_stack_response_fixture_decodes_and_round_trips_exactly() {
    let packet = decode_fixture(RESPONSE_FIXTURE);
    let McpePacketData::ItemStackResponsePacket(response) = &packet.data else {
        panic!("expected ItemStackResponse")
    };
    let slot = &response.responses[0]
        .containers
        .as_ref()
        .unwrap()
        .as_ref()
        .unwrap()[0]
        .slots[0];
    assert_eq!(slot.custom_name.unredacted, "Fixture item");
    assert_eq!(slot.custom_name.redacted, "Fixture item");

    let encoded = encode(&packet, &BedrockSession { shield_item_id: 0 }).unwrap();
    assert_eq!(encoded.as_ref(), RESPONSE_FIXTURE);
}

/// The only other generated use must carry the same two-string wire shape, and
/// a malicious declared length must fail before allocating or reading past the
/// available bytes.
#[test]
fn structure_editor_redactable_name_uses_two_bounded_adjacent_strings() {
    let structure = StructureEditorData {
        structure_name: BedrockSafetyRedactableString {
            unredacted: "structure".into(),
            redacted: "filtered".into(),
        },
        data_field: "payload".into(),
        ..Default::default()
    };
    let mut encoded = Vec::new();
    structure.encode(&mut encoded).unwrap();
    assert_eq!(&encoded[..20], b"\x09structure\x08filtered\x07");

    let mut body = Bytes::from(encoded.clone());
    let decoded = StructureEditorData::decode(&mut body, ()).unwrap();
    assert_eq!(decoded, structure);
    assert!(body.is_empty());
    let mut reencoded = Vec::new();
    decoded.encode(&mut reencoded).unwrap();
    assert_eq!(reencoded, encoded);

    let mut borrowed_body = Bytes::from(encoded);
    let borrowed = StructureEditorDataView::decode(&mut borrowed_body).unwrap();
    assert_eq!(borrowed.structure_name.unredacted.as_bytes(), b"structure");
    assert_eq!(borrowed.structure_name.redacted.as_bytes(), b"filtered");
    assert!(borrowed_body.is_empty());

    let empty = BedrockSafetyRedactableString {
        unredacted: String::new(),
        redacted: String::new(),
    };
    let mut empty_wire = Vec::new();
    empty.encode(&mut empty_wire).unwrap();
    assert_eq!(empty_wire, [0, 0]);
    let mut empty_wire = Bytes::from(empty_wire);
    assert_eq!(
        BedrockSafetyRedactableString::decode(&mut empty_wire, ()).unwrap(),
        empty
    );
    let mut borrowed_empty_wire = Bytes::from_static(&[0, 0]);
    let borrowed_empty =
        BedrockSafetyRedactableStringView::decode(&mut borrowed_empty_wire).unwrap();
    assert!(borrowed_empty.redacted.as_bytes().is_empty());

    let mut malformed = Bytes::from_static(&[0, 5, b'x']);
    assert!(matches!(
        BedrockSafetyRedactableString::decode(&mut malformed, ()),
        Err(DecodeError::StringLengthExceeded {
            declared: 5,
            available: 1
        })
    ));
}

#[test]
fn content_slot_hotbar_response_and_container_packets_normalize_in_wire_order() {
    let first_user_data = item_user_data(&["minecraft:stone"]);
    let content = InventoryContentPacket {
        container_id: u32::from(INVENTORY_CONTAINER),
        slots: vec![
            item(5, 2, 11, first_user_data.clone()),
            item(6, 3, 12, item_user_data(&["minecraft:dirt"])),
        ],
        full_container_name: full_container(
            FullContainerNameContainerName::CombinedHotbarAndInventoryContainer,
            Some(7),
        ),
        storage_item: ItemStackDescriptor::default(),
    };
    let InventoryEvent::Content(content) = normalize_content(content).unwrap() else {
        panic!("expected content event")
    };
    assert_eq!(content.container.window_id, Some(0));
    assert_eq!(content.container.slot_type, Some(12));
    assert_eq!(content.container.dynamic_id, Some(7));
    assert_eq!(content.slots[0].network_id, 5);
    assert_eq!(content.slots[1].network_id, 6);
    assert_eq!(content.slots[0].metadata, u32::MAX - 1);
    let first_digest: [u8; 32] = Sha256::digest(&first_user_data).into();
    assert_eq!(content.slots[0].nbt_digest, first_digest);

    let slot = InventorySlotPacket {
        container_id: INVENTORY_CONTAINER,
        slot: 8,
        full_container_name: Some(full_container(
            FullContainerNameContainerName::InventoryContainer,
            None,
        )),
        storage_item: None,
        item: item(7, 4, 13, Vec::new()),
    };
    let InventoryEvent::Slot(slot) = normalize_slot(slot).unwrap() else {
        panic!("expected slot event")
    };
    assert_eq!(slot.identity.slot, 8);
    assert_eq!(slot.stack.network_id, 7);
    assert_eq!(slot.stack.stack_network_id, 13);

    let hotbar = PlayerHotbarPacket {
        selected_slot: 4,
        container_id: INVENTORY_CONTAINER,
        shouldselectslot: true,
    };
    let InventoryEvent::SelectedSlot(selected) = normalize_hotbar(hotbar).unwrap() else {
        panic!("expected selected-slot event")
    };
    assert_eq!(selected.slot, 4);
    assert!(selected.select_slot);

    let response = ItemStackResponsePacket {
        responses: vec![accepted_response(
            44,
            vec![ItemStackResponseContainerInfo {
                full_container_name: full_container(
                    FullContainerNameContainerName::HotbarContainer,
                    Some(9),
                ),
                slots: vec![response_slot(2, 5, 13, "named", "filtered", -3)],
            }],
        )],
    };
    let InventoryEvent::Response(response) = normalize_response(response).unwrap() else {
        panic!("expected response event")
    };
    assert_eq!(response.responses[0].request_id, 44);
    assert_eq!(
        response.responses[0].containers[0].slots[0].item_stack_id,
        13
    );
    assert_eq!(
        response.responses[0].containers[0].slots[0]
            .custom_name
            .as_ref(),
        "named"
    );
    assert_eq!(
        response.responses[0].containers[0].slots[0]
            .filtered_custom_name
            .as_ref(),
        "filtered"
    );

    let open = ContainerOpenPacket {
        container_id: FIRST_CONTAINER,
        container_type: CONTAINER_TYPE,
        position: BlockPos { x: 1, y: 64, z: -2 },
        target_actor_id: ActorUniqueId {
            actor_unique_id: -77,
        },
    };
    let InventoryEvent::Open(open) = normalize_container_open(open).unwrap() else {
        panic!("expected open event")
    };
    assert_eq!(open.container, ContainerIdentity::window(1));
    assert_eq!(open.window_type, 0);
    assert_eq!(open.runtime_entity_id, -77);

    let close = ContainerClosePacket {
        container_id: FIRST_CONTAINER,
        container_type: CONTAINER_TYPE,
        server_initiated_close: true,
    };
    assert!(matches!(
        normalize_container_close(close).unwrap(),
        InventoryEvent::Close(_)
    ));
    let data = ContainerSetDataPacket {
        container_id: FIRST_CONTAINER,
        id: -4,
        value: 99,
    };
    assert!(matches!(
        normalize_container_data(data).unwrap(),
        InventoryEvent::Data(_)
    ));
}

#[test]
fn authority_and_identity_preserve_start_game_and_container_discriminants() {
    assert_eq!(
        normalize_authority(true),
        InventoryEvent::Authority(InventoryAuthority::Server)
    );
    assert_eq!(
        normalize_authority(false),
        InventoryEvent::Authority(InventoryAuthority::Client)
    );

    let unknown = InventoryContentPacket {
        container_id: u32::from_ne_bytes((-777i32).to_ne_bytes()),
        slots: Vec::new(),
        full_container_name: full_container(
            FullContainerNameContainerName::Unknown(211),
            Some(u32::MAX),
        ),
        storage_item: ItemStackDescriptor::default(),
    };
    let InventoryEvent::Content(content) = normalize_content(unknown).unwrap() else {
        panic!("expected content event")
    };
    assert_eq!(content.container.window_id, Some(-777));
    assert_eq!(content.container.slot_type, Some(211));
    assert_eq!(content.container.dynamic_id, Some(u32::MAX));

    let negative_item_id = InventoryContentPacket {
        container_id: u32::from(INVENTORY_CONTAINER),
        slots: vec![item(-5, 1, 1, Vec::new())],
        full_container_name: FullContainerName::default(),
        storage_item: ItemStackDescriptor::default(),
    };
    let InventoryEvent::Content(content) = normalize_content(negative_item_id).unwrap() else {
        panic!("expected content event")
    };
    assert_eq!(content.slots[0].network_id, -5);
}

#[test]
fn invalid_slots_items_and_collection_sizes_fail_closed() {
    let invalid_slot = InventorySlotPacket {
        container_id: INVENTORY_CONTAINER,
        slot: u32::MAX,
        full_container_name: None,
        storage_item: None,
        item: item(1, 1, 1, Vec::new()),
    };
    assert_eq!(
        normalize_slot(invalid_slot).unwrap_err(),
        InventoryPacketError::InvalidSlot(-1)
    );

    let oversized = InventoryContentPacket {
        container_id: u32::from(INVENTORY_CONTAINER),
        slots: vec![ItemStackDescriptor::default(); MAX_CONTAINER_SLOTS + 1],
        full_container_name: FullContainerName::default(),
        storage_item: ItemStackDescriptor::default(),
    };
    assert_eq!(
        normalize_content(oversized).unwrap_err(),
        InventoryPacketError::TooManySlots {
            count: MAX_CONTAINER_SLOTS + 1,
            max: MAX_CONTAINER_SLOTS
        }
    );

    let bad_extra = InventoryContentPacket {
        container_id: u32::from(INVENTORY_CONTAINER),
        slots: vec![item(1, 1, 1, vec![0; protocol::MAX_ITEM_EXTRA_BYTES + 1])],
        full_container_name: FullContainerName::default(),
        storage_item: ItemStackDescriptor::default(),
    };
    assert!(matches!(
        normalize_content(bad_extra),
        Err(InventoryPacketError::ItemExtraTooLarge { .. })
    ));

    assert_eq!(
        validate_item_nbt_size(MAX_ITEM_NBT_BYTES + 1).unwrap_err(),
        InventoryPacketError::ItemNbtTooLarge {
            bytes: MAX_ITEM_NBT_BYTES + 1,
            max: MAX_ITEM_NBT_BYTES
        }
    );
}

#[test]
fn response_nested_collection_bounds_are_checked_before_retention() {
    let too_many_responses = ItemStackResponsePacket {
        responses: vec![ItemStackResponseInfo::default(); MAX_STACK_RESPONSES + 1],
    };
    assert_eq!(
        normalize_response(too_many_responses).unwrap_err(),
        InventoryPacketError::TooManyResponses {
            count: MAX_STACK_RESPONSES + 1,
            max: MAX_STACK_RESPONSES
        }
    );

    let response = accepted_response(
        1,
        vec![ItemStackResponseContainerInfo::default(); MAX_RESPONSE_CONTAINERS + 1],
    );
    assert_eq!(
        normalize_response(ItemStackResponsePacket {
            responses: vec![response]
        })
        .unwrap_err(),
        InventoryPacketError::TooManyResponseContainers {
            count: MAX_RESPONSE_CONTAINERS + 1,
            max: MAX_RESPONSE_CONTAINERS,
        }
    );
}

#[test]
fn accepted_response_preserves_zero_stack_id_for_a_newly_empty_slot() {
    let response = accepted_response(
        2,
        vec![ItemStackResponseContainerInfo {
            full_container_name: full_container(
                FullContainerNameContainerName::HotbarContainer,
                None,
            ),
            slots: vec![response_slot(3, 0, 0, "", "", 0)],
        }],
    );
    let InventoryEvent::Response(event) = normalize_response(ItemStackResponsePacket {
        responses: vec![response],
    })
    .unwrap() else {
        panic!("expected response event")
    };
    let slot = &event.responses[0].containers[0].slots[0];
    assert_eq!(slot.item_stack_id, 0);
    assert_eq!(slot.count, 0);
}

#[test]
fn accepted_response_rejects_negative_stack_ids() {
    let response = accepted_response(
        3,
        vec![ItemStackResponseContainerInfo {
            full_container_name: full_container(
                FullContainerNameContainerName::HotbarContainer,
                None,
            ),
            slots: vec![ItemStackResponseSlotInfo {
                item_stack_net_id: Some(Some(TypedServerNetIdstructItemStackNetIdTagint32T0 {
                    id: -1,
                })),
                ..Default::default()
            }],
        }],
    );
    assert_eq!(
        normalize_response(ItemStackResponsePacket {
            responses: vec![response]
        })
        .unwrap_err(),
        InventoryPacketError::InvalidStackNetworkId(-1)
    );
}

#[test]
fn verified_network_stack_requires_retained_bytes_and_both_digests_to_match() {
    let digest: [u8; 32] = Sha256::digest(b"exact").into();
    let stack = NetworkItemStack {
        network_id: 5,
        metadata: 3,
        stack_network_id: 9,
        count: 2,
        nbt_digest: digest,
        block_runtime_id: 7,
        extra_data: Arc::from(&b"exact"[..]),
    };
    let verified = VerifiedNetworkItemStack::try_new(stack.clone(), digest).unwrap();
    assert_eq!(verified.network_id(), 5);
    assert_eq!(verified.metadata(), 3);
    assert_eq!(verified.stack_network_id(), 9);
    assert_eq!(verified.count(), 2);
    assert_eq!(verified.nbt_digest(), digest);
    assert_eq!(verified.block_runtime_id(), 7);
    assert_eq!(verified.extra_data(), b"exact");

    let mut wrong_retained = stack.clone();
    wrong_retained.nbt_digest = [1; 32];
    assert_eq!(
        VerifiedNetworkItemStack::try_new(wrong_retained, digest).unwrap_err(),
        InventoryPacketError::DigestMismatch
    );
    assert_eq!(
        VerifiedNetworkItemStack::try_new(stack, [2; 32]).unwrap_err(),
        InventoryPacketError::DigestMismatch
    );
}

// Regression: an InventorySlot whose items carry a zero-length user-data blob (air / empty
// items) must decode rather than fail. gophertunnel's writer emits a zero-length ByteSlice
// for an absent stack (`Writer.itemUserData`, `minecraft/protocol/writer.go`, the `!present`
// branch) and its reader returns without parsing when that blob is empty
// (`Reader.itemUserData`); valentine previously always read a 2-byte discriminant from the
// empty sub-buffer, producing "unexpected end of buffer: needed 2 bytes, had 0" on a real
// Lifeboat/sm3 join.
//
// 1.26.40 removes the shield-ID-discriminated extra-data union that made that read
// conditional at all: the user data is one opaque length-prefixed buffer, so `shield_item_id`
// is no longer a decode argument. The same wire body is kept because it still exercises the
// case the bug was found on — two descriptors whose user-data length prefix is zero.
#[test]
fn inventory_slot_with_empty_extra_items_decodes_and_round_trips() {
    use valentine::bedrock::codec::BedrockCodec;

    // Exact 22-byte InventorySlot body observed on the wire.
    let body: [u8; 22] = [
        0x7c, 0x00, 0x01, 0x01, 0x00, 0x01, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00,
        0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00,
    ];

    let mut buf: &[u8] = &body;
    let packet = InventorySlotPacket::decode(&mut buf, ())
        .expect("empty-user-data items must decode instead of erroring");
    assert!(buf.is_empty(), "entire body consumed, no trailing bytes");

    let storage = packet.storage_item.as_ref().expect("storage item present");
    assert_eq!(storage.id, 0, "storage item is air");
    assert!(storage.user_data_buffer.is_empty());
    assert_eq!(packet.item.id, 0, "new item is air");
    assert!(packet.item.user_data_buffer.is_empty());

    // Air items re-encode to a zero-length user-data blob, reproducing the original bytes exactly.
    let mut out = Vec::new();
    packet.encode(&mut out).expect("re-encode");
    assert_eq!(out, body, "round-trips back to the original wire bytes");
}
