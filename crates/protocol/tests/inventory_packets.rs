use protocol::{BedrockSession, NetworkItemStack, WorldEvent, decode_batch, into_world_event};
use protocol::{
    ContainerIdentity, InventoryAuthority, InventoryEvent, InventoryPacketError,
    MAX_CONTAINER_SLOTS, MAX_ITEM_NBT_BYTES, MAX_RESPONSE_CONTAINERS, MAX_STACK_RESPONSES,
    VerifiedNetworkItemStack, normalize_authority, normalize_container_close,
    normalize_container_data, normalize_container_open, normalize_content, normalize_hotbar,
    normalize_response, normalize_slot, validate_item_nbt_size,
};
use sha2::{Digest, Sha256};
use std::sync::Arc;
use valentine::bedrock::version::v1_26_30::{
    ContainerClosePacket, ContainerOpenPacket, ContainerSetDataPacket, ContainerSlotType,
    FullContainerName, InventoryContentPacket, InventorySlotPacket, InventorySlotPacketArgs,
    ItemExtraDataWithoutBlockingTick, ItemNew, ItemNewExtra, ItemStackResponsePacket,
    ItemStackResponsesItem, ItemStackResponsesItemContent,
    ItemStackResponsesItemContentContainersItem,
    ItemStackResponsesItemContentContainersItemSlotsItem, ItemStackResponsesItemStatus, ItemV4,
    ItemV4NetIdVariant, ItemV4NetIdVariantType, McpePacketData, PlayerHotbarPacket, WindowId,
    WindowIdVarint, WindowType,
};

const CONTENT_FIXTURE: &[u8] = include_bytes!("../fixtures/inventory_content.bin");
const SLOT_FIXTURE: &[u8] = include_bytes!("../fixtures/inventory_slot.bin");
const HOTBAR_FIXTURE: &[u8] = include_bytes!("../fixtures/player_hotbar.bin");
const RESPONSE_FIXTURE: &[u8] = include_bytes!("../fixtures/item_stack_response.bin");

fn item_v4(network_id: i16, count: u16, stack_network_id: i32, extra: &[u8]) -> ItemV4 {
    ItemV4 {
        network_id,
        count,
        metadata: -2,
        net_id_variant: Some(ItemV4NetIdVariant {
            type_: ItemV4NetIdVariantType::ItemStackNetId,
            id: stack_network_id,
        }),
        block_runtime_id: 91,
        extra_data: extra.to_vec(),
    }
}

fn item_new(network_id: i16, count: u16, stack_network_id: i32) -> ItemNew {
    ItemNew {
        network_id,
        count,
        metadata: 3,
        stack_id: Some(valentine::bedrock::version::v1_26_30::ItemNewStackId {
            empty: 0,
            id: stack_network_id,
        }),
        block_runtime_id: 92,
        extra: ItemNewExtra::Default(ItemExtraDataWithoutBlockingTick::default()),
    }
}

fn full_container(slot_type: ContainerSlotType, dynamic_id: Option<u32>) -> FullContainerName {
    FullContainerName {
        container_id: slot_type,
        dynamic_container_id: dynamic_id,
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
        McpePacketData::PacketInventoryContent(packet) => normalize_content(*packet).unwrap(),
        other => panic!("expected InventoryContent, got {other:?}"),
    };
    assert!(matches!(content, InventoryEvent::Content(_)));

    let slot = match decode_fixture(SLOT_FIXTURE).data {
        McpePacketData::PacketInventorySlot(packet) => normalize_slot(*packet).unwrap(),
        other => panic!("expected InventorySlot, got {other:?}"),
    };
    assert!(matches!(slot, InventoryEvent::Slot(_)));

    let hotbar = match decode_fixture(HOTBAR_FIXTURE).data {
        McpePacketData::PacketPlayerHotbar(packet) => normalize_hotbar(packet).unwrap(),
        other => panic!("expected PlayerHotbar, got {other:?}"),
    };
    assert!(matches!(hotbar, InventoryEvent::SelectedSlot(_)));

    let response = match decode_fixture(RESPONSE_FIXTURE).data {
        McpePacketData::PacketItemStackResponse(packet) => normalize_response(packet).unwrap(),
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

#[test]
fn content_slot_hotbar_response_and_container_packets_normalize_in_wire_order() {
    let content = InventoryContentPacket {
        window_id: WindowIdVarint::Inventory,
        input: vec![item_v4(5, 2, 11, b"first"), item_v4(6, 3, 12, b"second")],
        container: full_container(ContainerSlotType::HotbarAndInventory, Some(7)),
        storage_item: ItemV4::default(),
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
    let first_digest: [u8; 32] = Sha256::digest(b"first").into();
    assert_eq!(content.slots[0].nbt_digest, first_digest);

    let slot = InventorySlotPacket {
        window_id: WindowIdVarint::Inventory,
        slot: 8,
        container: Some(full_container(ContainerSlotType::Inventory, None)),
        storage_item: None,
        item: item_new(7, 4, 13),
    };
    let InventoryEvent::Slot(slot) = normalize_slot(slot).unwrap() else {
        panic!("expected slot event")
    };
    assert_eq!(slot.identity.slot, 8);
    assert_eq!(slot.stack.network_id, 7);
    assert_eq!(slot.stack.stack_network_id, 13);

    let hotbar = PlayerHotbarPacket {
        selected_slot: 4,
        window_id: WindowId::Inventory,
        select_slot: true,
    };
    let InventoryEvent::SelectedSlot(selected) = normalize_hotbar(hotbar).unwrap() else {
        panic!("expected selected-slot event")
    };
    assert_eq!(selected.slot, 4);
    assert!(selected.select_slot);

    let response = ItemStackResponsePacket {
        responses: vec![ItemStackResponsesItem {
            status: ItemStackResponsesItemStatus::Ok,
            request_id: 44,
            content: Some(ItemStackResponsesItemContent {
                containers: vec![ItemStackResponsesItemContentContainersItem {
                    slot_type: full_container(ContainerSlotType::Hotbar, Some(9)),
                    slots: vec![ItemStackResponsesItemContentContainersItemSlotsItem {
                        slot: 2,
                        hotbar_slot: 2,
                        count: 5,
                        item_stack_id: 13,
                        custom_name: "named".to_owned(),
                        filtered_custom_name: "filtered".to_owned(),
                        durability_correction: -3,
                    }],
                }],
            }),
        }],
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

    let open = ContainerOpenPacket {
        window_id: WindowId::First,
        window_type: WindowType::Container,
        coordinates: valentine::bedrock::version::v1_26_30::BlockCoordinates { x: 1, y: 64, z: -2 },
        runtime_entity_id: -77,
    };
    let InventoryEvent::Open(open) = normalize_container_open(open).unwrap() else {
        panic!("expected open event")
    };
    assert_eq!(open.container, ContainerIdentity::window(1));
    assert_eq!(open.window_type, 0);
    assert_eq!(open.runtime_entity_id, -77);

    let close = ContainerClosePacket {
        window_id: WindowId::First,
        window_type: WindowType::Container,
        server: true,
    };
    assert!(matches!(
        normalize_container_close(close).unwrap(),
        InventoryEvent::Close(_)
    ));
    let data = ContainerSetDataPacket {
        window_id: WindowId::First,
        property: -4,
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
        window_id: WindowIdVarint::Unknown(-777),
        input: Vec::new(),
        container: full_container(ContainerSlotType::Unknown(211), Some(u32::MAX)),
        storage_item: ItemV4::default(),
    };
    let InventoryEvent::Content(content) = normalize_content(unknown).unwrap() else {
        panic!("expected content event")
    };
    assert_eq!(content.container.window_id, Some(-777));
    assert_eq!(content.container.slot_type, Some(211));
    assert_eq!(content.container.dynamic_id, Some(u32::MAX));

    let negative_item_id = InventoryContentPacket {
        window_id: WindowIdVarint::Inventory,
        input: vec![item_v4(-5, 1, 1, b"")],
        container: FullContainerName::default(),
        storage_item: ItemV4::default(),
    };
    let InventoryEvent::Content(content) = normalize_content(negative_item_id).unwrap() else {
        panic!("expected content event")
    };
    assert_eq!(content.slots[0].network_id, -5);
}

#[test]
fn invalid_slots_items_and_collection_sizes_fail_closed() {
    let invalid_slot = InventorySlotPacket {
        window_id: WindowIdVarint::Inventory,
        slot: -1,
        container: None,
        storage_item: None,
        item: item_new(1, 1, 1),
    };
    assert_eq!(
        normalize_slot(invalid_slot).unwrap_err(),
        InventoryPacketError::InvalidSlot(-1)
    );

    let oversized = InventoryContentPacket {
        window_id: WindowIdVarint::Inventory,
        input: vec![ItemV4::default(); MAX_CONTAINER_SLOTS + 1],
        container: FullContainerName::default(),
        storage_item: ItemV4::default(),
    };
    assert_eq!(
        normalize_content(oversized).unwrap_err(),
        InventoryPacketError::TooManySlots {
            count: MAX_CONTAINER_SLOTS + 1,
            max: MAX_CONTAINER_SLOTS
        }
    );

    let bad_extra = InventoryContentPacket {
        window_id: WindowIdVarint::Inventory,
        input: vec![item_v4(
            1,
            1,
            1,
            &vec![0; protocol::MAX_ITEM_EXTRA_BYTES + 1],
        )],
        container: FullContainerName::default(),
        storage_item: ItemV4::default(),
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
        responses: vec![ItemStackResponsesItem::default(); MAX_STACK_RESPONSES + 1],
    };
    assert_eq!(
        normalize_response(too_many_responses).unwrap_err(),
        InventoryPacketError::TooManyResponses {
            count: MAX_STACK_RESPONSES + 1,
            max: MAX_STACK_RESPONSES
        }
    );

    let response = ItemStackResponsesItem {
        status: ItemStackResponsesItemStatus::Ok,
        request_id: 1,
        content: Some(ItemStackResponsesItemContent {
            containers: vec![
                ItemStackResponsesItemContentContainersItem::default();
                MAX_RESPONSE_CONTAINERS + 1
            ],
        }),
    };
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
    let response = ItemStackResponsesItem {
        status: ItemStackResponsesItemStatus::Ok,
        request_id: 2,
        content: Some(ItemStackResponsesItemContent {
            containers: vec![ItemStackResponsesItemContentContainersItem {
                slot_type: full_container(ContainerSlotType::Hotbar, None),
                slots: vec![ItemStackResponsesItemContentContainersItemSlotsItem {
                    slot: 3,
                    hotbar_slot: 3,
                    count: 0,
                    item_stack_id: 0,
                    custom_name: String::new(),
                    filtered_custom_name: String::new(),
                    durability_correction: 0,
                }],
            }],
        }),
    };
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
    let response = ItemStackResponsesItem {
        status: ItemStackResponsesItemStatus::Ok,
        request_id: 3,
        content: Some(ItemStackResponsesItemContent {
            containers: vec![ItemStackResponsesItemContentContainersItem {
                slot_type: full_container(ContainerSlotType::Hotbar, None),
                slots: vec![ItemStackResponsesItemContentContainersItemSlotsItem {
                    item_stack_id: -1,
                    ..Default::default()
                }],
            }],
        }),
    };
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

// Regression: an InventorySlot whose items carry a zero-length "extra" blob (air / empty
// items) must decode rather than fail. Gophertunnel's ItemInstanceNew reader returns without
// parsing when the extra blob is empty; valentine previously always read a 2-byte discriminant
// from the empty sub-buffer, producing "unexpected end of buffer: needed 2 bytes, had 0" on a
// real Lifeboat/sm3 join. shield_item_id is 0 here, so air items (network_id 0) also match the
// shield branch — the guard must fire before that check.
#[test]
fn inventory_slot_with_empty_extra_items_decodes_and_round_trips() {
    use valentine::bedrock::codec::BedrockCodec;

    // Exact 22-byte InventorySlot body observed on the wire.
    let body: [u8; 22] = [
        0x7c, 0x00, 0x01, 0x01, 0x00, 0x01, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00,
        0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00,
    ];

    let mut buf: &[u8] = &body;
    let packet =
        InventorySlotPacket::decode(&mut buf, InventorySlotPacketArgs { shield_item_id: 0 })
            .expect("empty-extra items must decode instead of erroring");
    assert!(buf.is_empty(), "entire body consumed, no trailing bytes");

    let storage = packet.storage_item.as_ref().expect("storage item present");
    assert_eq!(storage.network_id, 0, "storage item is air");
    assert!(matches!(storage.extra, ItemNewExtra::Default(_)));
    assert_eq!(packet.item.network_id, 0, "new item is air");
    assert!(matches!(packet.item.extra, ItemNewExtra::Default(_)));

    // Air items re-encode to a zero-length extra blob, reproducing the original bytes exactly.
    let mut out = Vec::new();
    packet.encode(&mut out).expect("re-encode");
    assert_eq!(out, body, "round-trips back to the original wire bytes");
}
