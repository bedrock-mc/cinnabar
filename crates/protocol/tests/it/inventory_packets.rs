use bedrock_protocol_1_26_51::enums::{
    ContainerEnumName as FullContainerNameContainerName,
    ItemStackNetResult as ItemStackResponseInfoResult,
};
use bedrock_protocol_1_26_51::packets::{
    self, ContainerClose as ContainerClosePacket, ContainerOpen as ContainerOpenPacket,
    ContainerSetData as ContainerSetDataPacket, InventoryContent as InventoryContentPacket,
    InventorySlot as InventorySlotPacket, ItemStackResponse as ItemStackResponsePacket,
    PlayerHotbar as PlayerHotbarPacket,
};
use bedrock_protocol_1_26_51::types::{
    ActorUniqueID, BedrockSafetyRedactableString, BlockPos, FullContainerName, ItemStackNetID,
    ItemStackRequestID, ItemStackResponseContainerInfo, ItemStackResponseInfo,
    ItemStackResponseSlotInfo, NetworkItemStackDescriptorSerializedData as ItemStackDescriptor,
    StructureEditorData,
};
use bedrock_protocol_1_26_51::wire::{self, Decode, Encode};
use bytes::Bytes;
use protocol::inventory::{
    normalize_container_close, normalize_container_data, normalize_container_open,
    normalize_content, normalize_hotbar, normalize_response, normalize_slot,
};
use protocol::{
    CanonicalCell, ContainerIdentity, InventoryAuthority, InventoryEvent, InventoryPacketError,
    MAX_CONTAINER_SLOTS, MAX_ITEM_NBT_BYTES, MAX_RESPONSE_CONTAINERS, MAX_STACK_RESPONSES,
    VerifiedNetworkItemStack, normalize_authority, project_container_cell, validate_item_nbt_size,
};
use protocol::{NetworkItemStack, WorldEvent, decode_batch, encode, into_world_event};
use sha2::{Digest, Sha256};
use std::sync::Arc;

const CONTENT_FIXTURE: &[u8] = include_bytes!("../../fixtures/inventory_content.bin");
const SLOT_FIXTURE: &[u8] = include_bytes!("../../fixtures/inventory_slot.bin");
/// A zeqa.net hotbar item: window 0 whose container name is `AnvilMaterial` (1).
const LIVE_ZEQA_SLOT_FIXTURE: &[u8] = include_bytes!("../../fixtures/inventory_slot_live_zeqa.bin");
/// zeqa.net's spawn armor: window 120 whose container name is `AnvilMaterial` (1).
const LIVE_ZEQA_ARMOR_FIXTURE: &[u8] =
    include_bytes!("../../fixtures/inventory_content_armor_live_zeqa.bin");
const HOTBAR_FIXTURE: &[u8] = include_bytes!("../../fixtures/player_hotbar.bin");
const RESPONSE_FIXTURE: &[u8] = include_bytes!("../../fixtures/item_stack_response.bin");

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
        id: wire::I16LE(id),
        stack_size: wire::U16LE(stacksize),
        aux_value: wire::VarUInt(u32::from_ne_bytes((-2i32).to_ne_bytes())),
        net_id_variant: Some(wire::ZigZag32(stack_network_id)),
        block_runtime_id: wire::VarUInt(91),
        user_data_buffer: user_data.into(),
    }
}

fn full_container(
    container_name: FullContainerNameContainerName,
    dynamic_id: Option<u32>,
) -> FullContainerName {
    FullContainerName {
        container_name,
        dynamic_id: dynamic_id.map(wire::U32LE),
    }
}

fn default_full_container() -> FullContainerName {
    FullContainerName::default()
}

/// One response slot. The custom name is followed by an optional filtered
/// name, as encoded by Gophertunnel's item-stack response codec.
fn response_slot(
    slot: u8,
    amount: u8,
    item_stack_id: i32,
    custom_name: &str,
    filtered_custom_name: &str,
    durability_correction: i32,
) -> ItemStackResponseSlotInfo {
    ItemStackResponseSlotInfo {
        requested_slot: wire::U8(slot),
        slot: wire::U8(slot),
        amount: wire::U8(amount),
        item_stack_net_id: Some(ItemStackNetID(item_stack_id)),
        custom_name: BedrockSafetyRedactableString {
            unredacted: custom_name.into(),
            redacted: Some(filtered_custom_name.into()),
        },
        durability_correction: wire::ZigZag32(durability_correction),
    }
}

/// One accepted response.
fn accepted_response(
    request_id: i32,
    containers: Vec<ItemStackResponseContainerInfo>,
) -> ItemStackResponseInfo {
    ItemStackResponseInfo {
        result: ItemStackResponseInfoResult::Success,
        client_request_id: ItemStackRequestID(request_id),
        containers: Some(containers),
    }
}

fn decode_fixture(bytes: &'static [u8]) -> protocol::Packet {
    let mut packets = decode_batch(bytes.into()).expect("decode pinned inventory fixture");
    assert_eq!(packets.len(), 1);
    packets.pop().unwrap()
}

#[test]
fn pinned_gophertunnel_inventory_fixtures_normalize_without_vendor_types() {
    let content = match *decode_fixture(CONTENT_FIXTURE).data {
        packets::Packet::InventoryContent(packet) => normalize_content(packet).unwrap(),
        other => panic!("expected InventoryContent, got {other:?}"),
    };
    assert!(matches!(content, InventoryEvent::Content(_)));

    let slot = match *decode_fixture(SLOT_FIXTURE).data {
        packets::Packet::InventorySlot(packet) => normalize_slot(packet).unwrap(),
        other => panic!("expected InventorySlot, got {other:?}"),
    };
    assert!(matches!(slot, InventoryEvent::Slot(_)));

    let hotbar = match *decode_fixture(HOTBAR_FIXTURE).data {
        packets::Packet::PlayerHotbar(packet) => normalize_hotbar(packet).unwrap(),
        other => panic!("expected PlayerHotbar, got {other:?}"),
    };
    assert!(matches!(hotbar, InventoryEvent::SelectedSlot(_)));

    let response = match *decode_fixture(RESPONSE_FIXTURE).data {
        packets::Packet::ItemStackResponse(packet) => normalize_response(packet).unwrap(),
        other => panic!("expected ItemStackResponse, got {other:?}"),
    };
    assert!(matches!(response, InventoryEvent::Response(_)));
}

/// The pinned InventorySlot fixture carries window id 0 plus a full
/// container name whose byte encodes `ContainerEnumName::
/// InventoryContainer` — the exact shape live servers send for ordinary
/// player-inventory slot updates. The canonical address projection must
/// route that shape onto player-inventory cell 4 exactly like the unnamed
/// window-0 shape, restoring prior admission until live evidence says
/// otherwise.
#[test]
fn pinned_inventory_slot_fixture_projects_through_the_canonical_address() {
    let slot = match *decode_fixture(SLOT_FIXTURE).data {
        packets::Packet::InventorySlot(packet) => normalize_slot(packet).unwrap(),
        other => panic!("expected InventorySlot, got {other:?}"),
    };
    let InventoryEvent::Slot(slot) = slot else {
        panic!("expected a Slot event")
    };
    assert_eq!(slot.identity.slot, 4);
    assert_eq!(
        slot.identity.container,
        ContainerIdentity {
            window_id: Some(0),
            slot_type: Some(29),
            dynamic_id: None,
        },
        "the pinned fixture decodes to the named InventoryContainer alias on window 0"
    );
    assert_eq!(
        project_container_cell(&slot.identity.container, slot.identity.slot),
        Some(CanonicalCell::PlayerInventory(4)),
    );
}

/// A live server's arbitrary container name on window 0 still lands in the player hotbar.
#[test]
fn live_window_zero_slot_routes_by_window_despite_a_foreign_container_name() {
    let InventoryEvent::Slot(slot) = (match *decode_fixture(LIVE_ZEQA_SLOT_FIXTURE).data {
        packets::Packet::InventorySlot(packet) => normalize_slot(packet).unwrap(),
        other => panic!("expected InventorySlot, got {other:?}"),
    }) else {
        panic!("expected a Slot event")
    };
    assert_eq!(slot.identity.container.window_id, Some(0));
    assert_eq!(slot.identity.container.slot_type, Some(1));
    assert_eq!(slot.stack.network_id, 20_329);
    assert_eq!(slot.stack.count, 1);
    assert_eq!(
        project_container_cell(&slot.identity.container, slot.identity.slot),
        Some(CanonicalCell::PlayerInventory(0)),
    );
}

/// A live server's armor content on window 120 lands in the armor cells whatever its name.
#[test]
fn live_window_120_content_routes_to_the_armor_cells() {
    let InventoryEvent::Content(content) = (match *decode_fixture(LIVE_ZEQA_ARMOR_FIXTURE).data {
        packets::Packet::InventoryContent(packet) => normalize_content(packet).unwrap(),
        other => panic!("expected InventoryContent, got {other:?}"),
    }) else {
        panic!("expected a Content event")
    };
    assert_eq!(content.container.window_id, Some(120));
    assert_eq!(content.slots.len(), 4);
    assert_eq!(
        project_container_cell(&content.container, 0),
        Some(CanonicalCell::Armor(0)),
    );
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

/// Pins the optional filtered name from Gophertunnel commit 283a5a97.
#[test]
fn item_stack_response_fixture_decodes_and_round_trips_exactly() {
    let packet = decode_fixture(RESPONSE_FIXTURE);
    let packets::Packet::ItemStackResponse(response) = &*packet.data else {
        panic!("expected ItemStackResponse")
    };
    let slot = &response.responses[0].containers.as_ref().unwrap()[0].slots[0];
    assert_eq!(slot.custom_name.unredacted, "Fixture item");
    assert_eq!(
        slot.custom_name.redacted,
        Some(wire::Str::from("Fixture item"))
    );

    let encoded = encode(&packet).unwrap();
    assert_eq!(encoded.as_ref(), RESPONSE_FIXTURE);
}

/// Checks presence, empty values and truncation of filtered names.
#[test]
fn response_filtered_name_preserves_optional_wire_shape() {
    for filtered in [None, Some(""), Some("filtered")] {
        let slot = ItemStackResponseSlotInfo {
            custom_name: BedrockSafetyRedactableString {
                unredacted: "original".into(),
                redacted: filtered.map(wire::Str::from),
            },
            durability_correction: wire::ZigZag32(-3),
            ..Default::default()
        };
        let wire = slot.encode_to_vec().unwrap();
        let owned = ItemStackResponseSlotInfo::decode_exact(&wire).unwrap();
        assert_eq!(owned, slot);
        assert_eq!(owned.encode_to_vec().unwrap(), wire);
        for end in 0..wire.len() {
            assert!(ItemStackResponseSlotInfo::decode_exact(&wire[..end]).is_err());
        }
    }
}

/// Structure editor names retain their two adjacent strings, and a malicious
/// declared length must fail before allocating or reading past the available
/// bytes.
#[test]
fn structure_editor_name_uses_two_bounded_adjacent_strings() {
    let structure = StructureEditorData {
        structure_name: "structure".into(),
        filtered_structure_name: "filtered".into(),
        data_field: "payload".into(),
        ..Default::default()
    };
    let encoded = structure.encode_to_vec().unwrap();
    assert_eq!(&encoded[..20], b"\x09structure\x08filtered\x07");

    let shared = Bytes::from(encoded.clone());
    let decoded = StructureEditorData::decode_shared(&shared, &wire::Limits::default()).unwrap();
    assert_eq!(decoded, structure);
    assert_eq!(decoded.structure_name.as_bytes(), b"structure");
    assert_eq!(decoded.filtered_structure_name.as_bytes(), b"filtered");
    assert_eq!(decoded.encode_to_vec().unwrap(), encoded);

    assert!(matches!(
        StructureEditorData::decode(&mut wire::Reader::new(&[0, 5, b'x'])),
        Err(wire::DecodeError::LengthNotRepresentable {
            field: bedrock_protocol_1_26_51::fields::STRUCTURE_EDITOR_DATA_FILTERED_STRUCTURE_NAME,
            needed: 5,
            remaining: 1
        })
    ));
}

/// The redactable string is an unredacted string plus an optional redacted one.
#[test]
fn redactable_string_redacted_half_is_optional() {
    let empty = BedrockSafetyRedactableString::default();
    let empty_wire = empty.encode_to_vec().unwrap();
    assert_eq!(empty_wire, [0, 0]);
    let decoded = BedrockSafetyRedactableString::decode_exact(&empty_wire).unwrap();
    assert_eq!(decoded, empty);
    assert!(decoded.redacted.is_none());
}

#[test]
fn content_slot_hotbar_response_and_container_packets_normalize_in_wire_order() {
    let first_user_data = item_user_data(&["minecraft:stone"]);
    let content = InventoryContentPacket {
        container_id: wire::VarUInt(u32::from(INVENTORY_CONTAINER)),
        slots: vec![
            item(5, 2, 11, first_user_data.clone()),
            item(6, 3, 12, item_user_data(&["minecraft:dirt"])),
        ],
        full_container_name: full_container(
            FullContainerNameContainerName::Combinedhotbarandinventorycontainer,
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
        container_id: wire::U8(INVENTORY_CONTAINER),
        slot: wire::VarUInt(8),
        full_container_name: Some(full_container(
            FullContainerNameContainerName::Inventorycontainer,
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
        selected_slot: wire::VarUInt(4),
        container_id: wire::U8(INVENTORY_CONTAINER),
        should_select_slot: true,
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
                    FullContainerNameContainerName::Hotbarcontainer,
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
    // The server-corrected damage rides the same slot payload; dropping it
    // would leave presentation on a stale NBT-only durability estimate.
    assert_eq!(
        response.responses[0].containers[0].slots[0].durability_correction,
        -3
    );

    let open = ContainerOpenPacket {
        container_id: wire::U8(FIRST_CONTAINER),
        container_type: wire::U8(CONTAINER_TYPE),
        position: BlockPos {
            x: wire::ZigZag32(1),
            y: wire::ZigZag32(64),
            z: wire::ZigZag32(-2),
        },
        target_actor_id: ActorUniqueID(-77),
    };
    let InventoryEvent::Open(open) = normalize_container_open(open).unwrap() else {
        panic!("expected open event")
    };
    assert_eq!(open.container, ContainerIdentity::window(1));
    assert_eq!(open.window_type, 0);
    assert_eq!(open.runtime_entity_id, -77);

    let close = ContainerClosePacket {
        container_id: wire::U8(FIRST_CONTAINER),
        container_type: wire::U8(CONTAINER_TYPE),
        server_initiated_close: true,
    };
    assert!(matches!(
        normalize_container_close(close).unwrap(),
        InventoryEvent::Close(_)
    ));
    let data = ContainerSetDataPacket {
        container_id: wire::U8(FIRST_CONTAINER),
        id: wire::ZigZag32(-4),
        value: wire::ZigZag32(99),
    };
    assert!(matches!(
        normalize_container_data(data).unwrap(),
        InventoryEvent::Data(_)
    ));
}

#[test]
fn default_full_container_descriptor_uses_legacy_player_window_identity_only() {
    let slots = vec![ItemStackDescriptor::default(); 36];
    let InventoryEvent::Content(content) = normalize_content(InventoryContentPacket {
        container_id: wire::VarUInt(u32::from(INVENTORY_CONTAINER)),
        slots,
        full_container_name: default_full_container(),
        storage_item: ItemStackDescriptor::default(),
    })
    .unwrap() else {
        panic!("expected content event")
    };
    assert_eq!(
        content.container,
        ContainerIdentity {
            window_id: Some(0),
            slot_type: None,
            dynamic_id: None,
        }
    );
    assert_eq!(
        project_container_cell(&content.container, 0),
        Some(CanonicalCell::PlayerInventory(0))
    );

    let InventoryEvent::Slot(slot) = normalize_slot(InventorySlotPacket {
        container_id: wire::U8(INVENTORY_CONTAINER),
        slot: wire::VarUInt(4),
        full_container_name: Some(default_full_container()),
        storage_item: None,
        item: item(7, 1, 13, Vec::new()),
    })
    .unwrap() else {
        panic!("expected slot event")
    };
    assert_eq!(slot.identity.container, content.container);
    assert_eq!(
        project_container_cell(&slot.identity.container, slot.identity.slot),
        Some(CanonicalCell::PlayerInventory(4))
    );

    let InventoryEvent::Content(offhand) = normalize_content(InventoryContentPacket {
        container_id: wire::VarUInt(u32::from_ne_bytes((-119i32).to_ne_bytes())),
        slots: vec![ItemStackDescriptor::default()],
        full_container_name: default_full_container(),
        storage_item: ItemStackDescriptor::default(),
    })
    .unwrap() else {
        panic!("expected content event")
    };
    assert_eq!(
        offhand.container,
        ContainerIdentity {
            window_id: Some(-119),
            slot_type: Some(0),
            dynamic_id: None,
        },
        "a foreign negative window is not the legacy offhand window"
    );

    let InventoryEvent::Content(offhand) = normalize_content(InventoryContentPacket {
        container_id: wire::VarUInt(119),
        slots: vec![ItemStackDescriptor::default()],
        full_container_name: default_full_container(),
        storage_item: ItemStackDescriptor::default(),
    })
    .unwrap() else {
        panic!("expected content event")
    };
    assert_eq!(offhand.container.slot_type, None);
    assert_eq!(
        project_container_cell(&offhand.container, 0),
        Some(CanonicalCell::Offhand)
    );
}

/// Legacy windows route by window id whatever name rides them; a windowless default name stays unrouted.
#[test]
fn legacy_windows_ignore_their_container_name_and_windowless_defaults_stay_unrouted() {
    let default_foreign = normalize_content(InventoryContentPacket {
        container_id: wire::VarUInt(120),
        slots: vec![ItemStackDescriptor::default(); 5],
        full_container_name: default_full_container(),
        storage_item: ItemStackDescriptor::default(),
    })
    .unwrap();
    let InventoryEvent::Content(default_foreign) = default_foreign else {
        panic!("expected content event")
    };
    assert_eq!(default_foreign.container.slot_type, Some(0));
    assert_eq!(
        project_container_cell(&default_foreign.container, 0),
        Some(CanonicalCell::Armor(0))
    );

    for dynamic_id in [0, 7] {
        let dynamic_default = normalize_content(InventoryContentPacket {
            container_id: wire::VarUInt(u32::from(INVENTORY_CONTAINER)),
            slots: vec![ItemStackDescriptor::default()],
            full_container_name: FullContainerName {
                dynamic_id: Some(wire::U32LE(dynamic_id)),
                ..default_full_container()
            },
            storage_item: ItemStackDescriptor::default(),
        })
        .unwrap();
        let InventoryEvent::Content(dynamic_default) = dynamic_default else {
            panic!("expected content event")
        };
        assert_eq!(dynamic_default.container.slot_type, Some(0));
        assert_eq!(dynamic_default.container.dynamic_id, Some(dynamic_id));
        assert_eq!(
            project_container_cell(&dynamic_default.container, 0),
            Some(CanonicalCell::PlayerInventory(0))
        );
    }

    for name in [
        FullContainerNameContainerName::Craftinginputcontainer,
        FullContainerNameContainerName::Unknown(211),
    ] {
        let event = normalize_content(InventoryContentPacket {
            container_id: wire::VarUInt(u32::from(INVENTORY_CONTAINER)),
            slots: vec![ItemStackDescriptor::default()],
            full_container_name: full_container(name, None),
            storage_item: ItemStackDescriptor::default(),
        })
        .unwrap();
        let InventoryEvent::Content(content) = event else {
            panic!("expected content event")
        };
        assert!(content.container.slot_type.is_some());
        assert_eq!(
            project_container_cell(&content.container, 0),
            Some(CanonicalCell::PlayerInventory(0))
        );
    }

    let cursor = normalize_content(InventoryContentPacket {
        container_id: wire::VarUInt(u32::from(INVENTORY_CONTAINER)),
        slots: vec![ItemStackDescriptor::default()],
        full_container_name: full_container(FullContainerNameContainerName::Cursorcontainer, None),
        storage_item: ItemStackDescriptor::default(),
    })
    .unwrap();
    let InventoryEvent::Content(cursor) = cursor else {
        panic!("expected content event")
    };
    assert_eq!(
        project_container_cell(&cursor.container, 0),
        Some(CanonicalCell::PlayerInventory(0))
    );

    let response = normalize_response(ItemStackResponsePacket {
        responses: vec![accepted_response(
            44,
            vec![ItemStackResponseContainerInfo {
                full_container_name: default_full_container(),
                slots: vec![response_slot(0, 1, 13, "", "", 0)],
            }],
        )],
    })
    .unwrap();
    let InventoryEvent::Response(response) = response else {
        panic!("expected response event")
    };
    let container = response.responses[0].containers[0].container;
    assert_eq!(container.window_id, None);
    assert_eq!(container.slot_type, Some(0));
    assert_eq!(container.dynamic_id, None);
    assert_eq!(project_container_cell(&container, 0), None);
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
        container_id: wire::VarUInt(u32::from_ne_bytes((-777i32).to_ne_bytes())),
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
        container_id: wire::VarUInt(u32::from(INVENTORY_CONTAINER)),
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
        container_id: wire::U8(INVENTORY_CONTAINER),
        slot: wire::VarUInt(u32::MAX),
        full_container_name: None,
        storage_item: None,
        item: item(1, 1, 1, Vec::new()),
    };
    assert_eq!(
        normalize_slot(invalid_slot).unwrap_err(),
        InventoryPacketError::InvalidSlot(-1)
    );

    let oversized = InventoryContentPacket {
        container_id: wire::VarUInt(u32::from(INVENTORY_CONTAINER)),
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
        container_id: wire::VarUInt(u32::from(INVENTORY_CONTAINER)),
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
                FullContainerNameContainerName::Hotbarcontainer,
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
fn accepted_response_retains_odd_stack_ids_for_lenient_sparse_validation() {
    let response = accepted_response(
        3,
        vec![ItemStackResponseContainerInfo {
            full_container_name: full_container(
                FullContainerNameContainerName::Hotbarcontainer,
                None,
            ),
            slots: vec![ItemStackResponseSlotInfo {
                item_stack_net_id: Some(ItemStackNetID(-1)),
                ..Default::default()
            }],
        }],
    );
    let InventoryEvent::Response(event) = normalize_response(ItemStackResponsePacket {
        responses: vec![response],
    })
    .unwrap() else {
        panic!("response")
    };
    assert_eq!(event.responses[0].containers[0].slots[0].item_stack_id, -1);
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
// (`Reader.itemUserData`); the old codec always read a 2-byte discriminant from the
// empty sub-buffer, producing "unexpected end of buffer: needed 2 bytes, had 0" on a real
// Lifeboat/sm3 join.
//
// 1.26.40 removes the shield-ID-discriminated extra-data union that made that read
// conditional at all: the user data is one opaque length-prefixed buffer, so `shield_item_id`
// is no longer a decode argument. The same wire body is kept because it still exercises the
// case the bug was found on — two descriptors whose user-data length prefix is zero.
#[test]
fn inventory_slot_with_empty_extra_items_decodes_and_round_trips() {
    // Exact 22-byte InventorySlot body observed on the wire.
    let body: [u8; 22] = [
        0x7c, 0x00, 0x01, 0x01, 0x00, 0x01, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00,
        0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00,
    ];

    let packet = InventorySlotPacket::decode_exact(&body)
        .expect("empty-user-data items must decode instead of erroring, with no trailing bytes");

    let storage = packet.storage_item.as_ref().expect("storage item present");
    assert_eq!(storage.id.0, 0, "storage item is air");
    assert!(storage.user_data_buffer.is_empty());
    assert_eq!(packet.item.id.0, 0, "new item is air");
    assert!(packet.item.user_data_buffer.is_empty());

    // Air items re-encode to a zero-length user-data blob, reproducing the original bytes exactly.
    let out = packet.encode_to_vec().expect("re-encode");
    assert_eq!(out, body, "round-trips back to the original wire bytes");
}
