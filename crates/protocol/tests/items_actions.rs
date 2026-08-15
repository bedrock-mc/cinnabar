use bytes::Bytes;
use protocol::{
    into_world_event, ActorActionKind, ActorEvent, ActorHandedness, EquipmentEvent, ItemActorEvent,
    NetworkItemStack, WorldEvent, MAX_ACTION_IDENTIFIER_BYTES, MAX_ANIMATE_ENTITY_IDS,
    MAX_ANIMATION_IDENTIFIER_BYTES, MAX_ITEM_EXTRA_BYTES, MAX_ITEM_REGISTRY_ENTRIES,
};
use sha2::{Digest, Sha256};
use valentine::bedrock::codec::Nbt;
use valentine::bedrock::version::v1_26_40::{
    ActorRuntimeId, AddActorPacket, AddPlayerPacket, AnimateEntityPacket, AnimatePacket,
    CerealizerNetworkItemStackDescriptorSerializedData as ItemStackDescriptor,
    EnumsAnimatePacketPayloadAction as AnimatePacketAction, ItemData, ItemRegistryPacket,
    MobEquipmentPacket, NetworkSettingsPacket,
};

/// The inventory container. 1.26.40 carries the raw container ID rather than a
/// named `WindowId` enum (gophertunnel `MobEquipment.WindowID`, a plain byte).
const INVENTORY_CONTAINER: u8 = 0;
/// `CONTAINER_ID_OFFHAND`, the container that implies a left-handed equip.
const OFFHAND_CONTAINER: u8 = 119;
/// `CONTAINER_ID_NONE` (-1) as it appears in the unsigned byte on the wire.
const NO_CONTAINER: u8 = u8::MAX;

/// Builds an item user-data buffer that carries no NBT compound.
///
/// 1.26.40 hands the trailing item user data over as one opaque length-prefixed
/// buffer instead of modelling its interior, so fixtures have to spell the
/// layout out. gophertunnel be6713da4dc051a4197f897d04835e89e9c54321
/// `minecraft/protocol/writer.go` `Writer.itemUserData`: an `int16` of 0 means
/// "no compound", then `canPlaceOn` and `canBreak` as `uint32`-counted lists of
/// `StringUTF` (an `int16` length prefix). The shield blocking tick is written
/// only for shield items and so is absent here.
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

fn stack_item() -> ItemStackDescriptor {
    ItemStackDescriptor {
        id: 5,
        stacksize: 2,
        auxvalue: 3,
        net_id_variant: Some(7),
        block_runtime_id: 9,
        user_data_buffer: item_user_data(&["minecraft:stone"]),
    }
}

fn runtime_id(actor_runtime_id: i64) -> ActorRuntimeId {
    ActorRuntimeId {
        actor_runtime_id: u64::from_ne_bytes(actor_runtime_id.to_ne_bytes()),
    }
}

#[test]
fn reviewed_packet_bounds_are_exact() {
    assert_eq!(MAX_ITEM_REGISTRY_ENTRIES, 16_384);
    assert_eq!(MAX_ITEM_EXTRA_BYTES, 64 * 1024);
    assert_eq!(MAX_ANIMATE_ENTITY_IDS, 256);
    assert_eq!(MAX_ACTION_IDENTIFIER_BYTES, 256);
    assert_eq!(MAX_ANIMATION_IDENTIFIER_BYTES, 256);
}

#[test]
fn item_registry_and_add_player_held_item_are_vendor_neutral() {
    let registry = ItemRegistryPacket {
        item_data: vec![ItemData {
            item_name: "minecraft:stick".into(),
            item_id: 5,
            ..Default::default()
        }],
    };
    let WorldEvent::ItemActor(ItemActorEvent::Registry(registry)) =
        into_world_event(registry.into(), 0).unwrap().unwrap()
    else {
        panic!("expected item registry")
    };
    assert_eq!(registry.entries.len(), 1);
    assert_eq!(registry.entries[0].identifier.as_ref(), "minecraft:stick");
    assert_eq!(registry.entries[0].network_id, 5);

    let player = AddPlayerPacket {
        target_runtime_id: runtime_id(42),
        carried_item: stack_item(),
        ..Default::default()
    };
    let WorldEvent::Actor(ActorEvent::Spawn(spawn)) =
        into_world_event(player.into(), 0).unwrap().unwrap()
    else {
        panic!("expected player spawn")
    };
    assert_eq!(spawn.held_item.network_id, 5);
    assert_eq!(spawn.held_item.metadata, 3);
    assert_eq!(spawn.held_item.stack_network_id, 7);
    assert_eq!(spawn.held_item.count, 2);
    assert_eq!(spawn.held_item.block_runtime_id, 9);
    assert!(!spawn.held_item.extra_data.is_empty());
    assert!(spawn.held_item.extra_data.len() <= MAX_ITEM_EXTRA_BYTES);
    assert_eq!(
        spawn.held_item.nbt_digest,
        Sha256::digest(&spawn.held_item.extra_data).as_slice()
    );
}

#[test]
fn mob_equipment_retains_slots_and_canonical_stack_identity() {
    let packet = MobEquipmentPacket {
        target_runtime_id: runtime_id(42),
        item: ItemStackDescriptor {
            id: 5,
            stacksize: 4,
            auxvalue: 6,
            net_id_variant: Some(8),
            block_runtime_id: 10,
            user_data_buffer: item_user_data(&[]),
        },
        slot: 2,
        selected_slot: 2,
        container_id: INVENTORY_CONTAINER,
    };
    let WorldEvent::Equipment(EquipmentEvent {
        actor_runtime_id,
        stack,
        inventory_slot,
        selected_slot,
        window_id,
        handedness,
    }) = into_world_event(packet.into(), 0).unwrap().unwrap()
    else {
        panic!("expected equipment")
    };
    assert_eq!(actor_runtime_id, 42);
    assert_eq!(stack.network_id, 5);
    assert_eq!(stack.metadata, 6);
    assert_eq!(stack.stack_network_id, 8);
    assert_eq!(stack.count, 4);
    assert_eq!(inventory_slot, 2);
    assert_eq!(selected_slot, 2);
    assert_eq!(window_id, 0);
    assert_eq!(handedness, Some(ActorHandedness::Right));

    let offhand = MobEquipmentPacket {
        target_runtime_id: runtime_id(42),
        item: ItemStackDescriptor {
            id: 5,
            stacksize: 1,
            ..Default::default()
        },
        slot: 0,
        selected_slot: 0,
        container_id: OFFHAND_CONTAINER,
    };
    let WorldEvent::Equipment(offhand) = into_world_event(offhand.into(), 0).unwrap().unwrap()
    else {
        panic!("expected offhand equipment")
    };
    assert_eq!(offhand.handedness, Some(ActorHandedness::Left));
}

#[test]
fn animate_known_row_and_unknown_actions_are_attributed() {
    for (action, expected) in [
        (AnimatePacketAction::Swing, ActorActionKind::SwingArm),
        (AnimatePacketAction::WakeUp, ActorActionKind::Wake),
        (
            AnimatePacketAction::CriticalHit,
            ActorActionKind::CriticalHit,
        ),
        (
            AnimatePacketAction::MagicCriticalHit,
            ActorActionKind::MagicCriticalHit,
        ),
        (AnimatePacketAction::Unknown(128), ActorActionKind::RowRight),
        (AnimatePacketAction::Unknown(129), ActorActionKind::RowLeft),
        (
            AnimatePacketAction::Unknown(200),
            ActorActionKind::Ignored { action_id: 200 },
        ),
    ] {
        let packet = AnimatePacket {
            action,
            target_actor_runtime_id: runtime_id(42),
            data: 0.25,
            swing_source: Some("attack".into()),
        };
        let WorldEvent::ItemActor(ItemActorEvent::Action(action)) =
            into_world_event(packet.into(), 0).unwrap().unwrap()
        else {
            panic!("expected action")
        };
        assert_eq!(action.actor_runtime_ids.as_ref(), &[42]);
        assert_eq!(action.kind, expected);
        assert_eq!(action.data, 0.25);
    }
}

#[test]
fn animate_entity_retains_one_bounded_custom_action_for_all_targets() {
    let packet = AnimateEntityPacket {
        m_animation: "animation.test.attack".into(),
        m_next_state: "default".into(),
        m_stop_expression: "query.any_animation_finished".into(),
        m_stop_expression_version: 1,
        m_controller: "controller.animation.test".into(),
        m_blend_out_time: 0.1,
        m_runtime_ids: vec![runtime_id(42), runtime_id(43)],
    };
    let WorldEvent::ItemActor(ItemActorEvent::Action(action)) =
        into_world_event(packet.into(), 0).unwrap().unwrap()
    else {
        panic!("expected custom action")
    };
    assert_eq!(action.actor_runtime_ids.as_ref(), &[42, 43]);
    assert_eq!(
        action.kind,
        ActorActionKind::Custom {
            animation: "animation.test.attack".into(),
            controller: "controller.animation.test".into(),
        }
    );
}

#[test]
fn unrelated_packets_remain_ignored() {
    let packet = NetworkSettingsPacket::default();
    assert_eq!(into_world_event(packet.into(), 0).unwrap(), None);
    let _ = NetworkItemStack::default();
}

#[test]
fn non_player_spawns_receive_the_canonical_empty_stack() {
    let packet = AddActorPacket {
        target_runtime_id: runtime_id(42),
        actor_type: "minecraft:bee".into(),
        ..Default::default()
    };
    let WorldEvent::Actor(ActorEvent::Spawn(spawn)) =
        into_world_event(packet.into(), 0).unwrap().unwrap()
    else {
        panic!("expected entity spawn")
    };
    assert!(spawn.held_item.is_empty());
    assert_eq!(spawn.held_item.stack_network_id, -1);
    let empty_digest: [u8; 32] = Sha256::digest([]).into();
    assert_eq!(spawn.held_item.nbt_digest, empty_digest);
    assert!(spawn.held_item.extra_data.is_empty());
}

#[test]
fn item_stacks_reject_invalid_identity_and_unbounded_extra_bytes() {
    for item in [
        // A named item that claims to hold nothing.
        ItemStackDescriptor {
            id: 5,
            stacksize: 0,
            ..Default::default()
        },
        // A *present* stack net ID that is not a positive tracking ID. 1.26.40
        // models the net ID as a plain `Option`, so protocol 1001's
        // "present flag says no, payload says yes" shape can no longer be
        // built; this is the contradiction that survives the collapse.
        ItemStackDescriptor {
            id: 5,
            stacksize: 1,
            net_id_variant: Some(0),
            ..Default::default()
        },
    ] {
        let packet = AddPlayerPacket {
            target_runtime_id: runtime_id(42),
            carried_item: item,
            ..Default::default()
        };
        assert!(into_world_event(packet.into(), 0).is_err());
    }

    let signed_wire_fields = AddPlayerPacket {
        target_runtime_id: runtime_id(42),
        carried_item: ItemStackDescriptor {
            id: -1,
            stacksize: 1,
            auxvalue: u32::from_ne_bytes((-1i32).to_ne_bytes()),
            ..Default::default()
        },
        ..Default::default()
    };
    let WorldEvent::Actor(ActorEvent::Spawn(spawn)) =
        into_world_event(signed_wire_fields.into(), 0)
            .unwrap()
            .unwrap()
    else {
        panic!("expected signed item wire fields")
    };
    assert_eq!(spawn.held_item.network_id, -1);
    assert_eq!(spawn.held_item.metadata, u32::MAX);
    assert_eq!(spawn.held_item.stack_network_id, -1);

    let packet = AddPlayerPacket {
        target_runtime_id: runtime_id(42),
        carried_item: ItemStackDescriptor {
            id: 5,
            stacksize: 1,
            user_data_buffer: vec![0; MAX_ITEM_EXTRA_BYTES + 1],
            ..Default::default()
        },
        ..Default::default()
    };
    assert!(into_world_event(packet.into(), 0).is_err());

    // A buffer too short to even hold its own `int16` header. Protocol 1001
    // spent this case on a `canDestroy` string longer than its `int16` length
    // prefix could describe; 1.26.40 never re-encodes the interior strings, so
    // the surviving truncation check is on the header itself.
    let truncated_user_data = AddPlayerPacket {
        target_runtime_id: runtime_id(42),
        carried_item: ItemStackDescriptor {
            id: 5,
            stacksize: 1,
            user_data_buffer: vec![0xff],
            ..Default::default()
        },
        ..Default::default()
    };
    assert!(into_world_event(truncated_user_data.into(), 0).is_err());

    let invalid_nbt_version = AddPlayerPacket {
        target_runtime_id: runtime_id(42),
        carried_item: ItemStackDescriptor {
            id: 5,
            stacksize: 1,
            // int16 -1 announces a compound, then an unsupported version byte.
            user_data_buffer: vec![0xff, 0xff, 2],
            ..Default::default()
        },
        ..Default::default()
    };
    assert!(into_world_event(invalid_nbt_version.into(), 0).is_err());

    let malformed_nbt = AddPlayerPacket {
        target_runtime_id: runtime_id(42),
        carried_item: ItemStackDescriptor {
            id: 5,
            stacksize: 1,
            // Version 1 followed by a byte that is not a valid tag.
            user_data_buffer: vec![0xff, 0xff, 1, 0xff],
            ..Default::default()
        },
        ..Default::default()
    };
    assert!(into_world_event(malformed_nbt.into(), 0).is_err());
}

#[test]
fn registry_preserves_signed_ids_and_rejects_duplicate_and_oversized_records() {
    let signed_id = ItemRegistryPacket {
        item_data: vec![ItemData {
            item_name: "minecraft:signed".into(),
            item_id: -1,
            ..Default::default()
        }],
    };
    let WorldEvent::ItemActor(ItemActorEvent::Registry(signed_id)) =
        into_world_event(signed_id.into(), 0).unwrap().unwrap()
    else {
        panic!("expected signed registry ID")
    };
    assert_eq!(signed_id.entries[0].network_id, -1);

    let malformed_nbt = ItemRegistryPacket {
        item_data: vec![ItemData {
            item_name: "minecraft:malformed".into(),
            item_id: 5,
            item_component_data: Nbt(Bytes::from_static(&[0xff])),
            ..Default::default()
        }],
    };
    assert!(into_world_event(malformed_nbt.into(), 0).is_err());

    let duplicate = ItemRegistryPacket {
        item_data: vec![
            ItemData {
                item_name: "minecraft:stick".into(),
                item_id: 5,
                ..Default::default()
            },
            ItemData {
                item_name: "minecraft:stick".into(),
                item_id: 6,
                ..Default::default()
            },
        ],
    };
    assert!(into_world_event(duplicate.into(), 0).is_err());

    let long_name = ItemRegistryPacket {
        item_data: vec![ItemData {
            item_name: "x".repeat(MAX_ACTION_IDENTIFIER_BYTES + 1),
            item_id: 5,
            ..Default::default()
        }],
    };
    assert!(into_world_event(long_name.into(), 0).is_err());

    let oversized = ItemRegistryPacket {
        item_data: (0..=MAX_ITEM_REGISTRY_ENTRIES)
            .map(|index| ItemData {
                item_name: format!("test:item_{index}"),
                item_id: 1,
                ..Default::default()
            })
            .collect(),
    };
    assert!(into_world_event(oversized.into(), 0).is_err());
}

#[test]
fn equipment_rejects_invalid_runtime_and_stack_but_retains_unusual_slots() {
    let valid_item = ItemStackDescriptor {
        id: 5,
        stacksize: 1,
        ..Default::default()
    };
    // A zero runtime id and a semantically invalid item still disconnect: those
    // are genuine protocol violations.
    for packet in [
        MobEquipmentPacket {
            target_runtime_id: runtime_id(0),
            item: valid_item.clone(),
            slot: 0,
            selected_slot: 0,
            container_id: INVENTORY_CONTAINER,
        },
        MobEquipmentPacket {
            target_runtime_id: runtime_id(42),
            item: ItemStackDescriptor {
                user_data_buffer: vec![0; MAX_ITEM_EXTRA_BYTES + 1],
                ..Default::default()
            },
            slot: 0,
            selected_slot: 0,
            container_id: INVENTORY_CONTAINER,
        },
    ] {
        assert!(into_world_event(packet.into(), 0).is_err());
    }

    // Non-hotbar and mismatched slots are common on custom servers; the client
    // never reads them, so they are retained verbatim instead of disconnecting.
    for (slot, selected_slot) in [(0, 9), (1, 0), (255, 255)] {
        let packet = MobEquipmentPacket {
            target_runtime_id: runtime_id(42),
            item: valid_item.clone(),
            slot,
            selected_slot,
            container_id: INVENTORY_CONTAINER,
        };
        let WorldEvent::Equipment(event) = into_world_event(packet.into(), 0).unwrap().unwrap()
        else {
            panic!("expected retained equipment")
        };
        assert_eq!(event.selected_slot, selected_slot);
        assert_eq!(event.inventory_slot, i32::from(slot));
    }

    let signed_window = MobEquipmentPacket {
        target_runtime_id: runtime_id(42),
        item: valid_item,
        slot: 0,
        selected_slot: 0,
        container_id: NO_CONTAINER,
    };
    let WorldEvent::Equipment(signed_window) =
        into_world_event(signed_window.into(), 0).unwrap().unwrap()
    else {
        panic!("expected bit-preserved window")
    };
    assert_eq!(signed_window.window_id, u8::MAX);
    assert_eq!(signed_window.handedness, None);
}

fn custom_action(targets: Vec<i64>) -> AnimateEntityPacket {
    AnimateEntityPacket {
        m_animation: "animation.test.attack".into(),
        m_next_state: "default".into(),
        m_stop_expression: "query.any_animation_finished".into(),
        m_stop_expression_version: 1,
        m_controller: "controller.animation.test".into(),
        m_blend_out_time: 0.1,
        m_runtime_ids: targets.into_iter().map(runtime_id).collect(),
    }
}

#[test]
fn animate_entity_enforces_exact_target_and_text_bounds() {
    let maximum = custom_action((1..=MAX_ANIMATE_ENTITY_IDS as i64).collect());
    assert!(into_world_event(maximum.into(), 0).unwrap().is_some());

    for packet in [
        custom_action(vec![]),
        custom_action((1..=(MAX_ANIMATE_ENTITY_IDS + 1) as i64).collect()),
        custom_action(vec![42, 42]),
        AnimateEntityPacket {
            m_animation: "x".repeat(MAX_ANIMATION_IDENTIFIER_BYTES + 1),
            ..custom_action(vec![42])
        },
        AnimateEntityPacket {
            m_controller: "x".repeat(MAX_ACTION_IDENTIFIER_BYTES + 1),
            ..custom_action(vec![42])
        },
        AnimateEntityPacket {
            m_blend_out_time: f32::NAN,
            ..custom_action(vec![42])
        },
    ] {
        assert!(into_world_event(packet.into(), 0).is_err());
    }

    let WorldEvent::ItemActor(ItemActorEvent::Action(high_runtime_id)) =
        into_world_event(custom_action(vec![-1]).into(), 0)
            .unwrap()
            .unwrap()
    else {
        panic!("expected bit-preserved high runtime ID")
    };
    assert_eq!(high_runtime_id.actor_runtime_ids.as_ref(), &[u64::MAX]);
}

#[test]
fn animate_rejects_invalid_runtime_non_finite_data_and_oversized_source() {
    for packet in [
        AnimatePacket {
            target_actor_runtime_id: runtime_id(0),
            action: AnimatePacketAction::Swing,
            ..Default::default()
        },
        AnimatePacket {
            target_actor_runtime_id: runtime_id(42),
            action: AnimatePacketAction::Swing,
            data: f32::INFINITY,
            ..Default::default()
        },
        AnimatePacket {
            target_actor_runtime_id: runtime_id(42),
            action: AnimatePacketAction::Swing,
            swing_source: Some("x".repeat(MAX_ACTION_IDENTIFIER_BYTES + 1)),
            ..Default::default()
        },
    ] {
        assert!(into_world_event(packet.into(), 0).is_err());
    }
}
