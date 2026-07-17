use bytes::Bytes;
use protocol::{
    ActorActionKind, ActorEvent, ActorHandedness, EquipmentEvent, ItemActorEvent,
    MAX_ACTION_IDENTIFIER_BYTES, MAX_ANIMATE_ENTITY_IDS, MAX_ANIMATION_IDENTIFIER_BYTES,
    MAX_ITEM_EXTRA_BYTES, MAX_ITEM_REGISTRY_ENTRIES, NetworkItemStack, WorldEvent,
    into_world_event,
};
use sha2::{Digest, Sha256};
use valentine::bedrock::codec::Nbt;
use valentine::bedrock::version::v1_26_30::{
    AddEntityPacket, AddPlayerPacket, AnimateEntityPacket, AnimatePacket, AnimatePacketActionId,
    Item, ItemContent, ItemContentExtra, ItemExtraDataWithoutBlockingTick,
    ItemExtraDataWithoutBlockingTickNbt, ItemNew, ItemNewExtra, ItemNewStackId, ItemRegistryPacket,
    ItemstatesItem, MobEquipmentPacket, NetworkSettingsPacket, WindowId,
};

fn stack_item() -> Item {
    Item {
        network_id: 5,
        content: Some(Box::new(ItemContent {
            count: 2,
            metadata: 3,
            has_stack_id: 1,
            stack_id: Some(7),
            block_runtime_id: 9,
            extra: ItemContentExtra::Default(Default::default()),
        })),
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
        itemstates: vec![ItemstatesItem {
            name: "minecraft:stick".into(),
            runtime_id: 5,
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
        runtime_id: 42,
        unique_id: -7,
        held_item: stack_item(),
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
        runtime_entity_id: 42,
        item: ItemNew {
            network_id: 5,
            count: 4,
            metadata: 6,
            stack_id: Some(ItemNewStackId { empty: 0, id: 8 }),
            block_runtime_id: 10,
            extra: ItemNewExtra::Default(Default::default()),
        },
        slot: 2,
        selected_slot: 2,
        window_id: WindowId::Inventory,
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
        runtime_entity_id: 42,
        item: ItemNew {
            network_id: 5,
            count: 1,
            ..Default::default()
        },
        slot: 0,
        selected_slot: 0,
        window_id: WindowId::Offhand,
    };
    let WorldEvent::Equipment(offhand) = into_world_event(offhand.into(), 0).unwrap().unwrap()
    else {
        panic!("expected offhand equipment")
    };
    assert_eq!(offhand.handedness, Some(ActorHandedness::Left));
}

#[test]
fn animate_known_row_and_unknown_actions_are_attributed() {
    for (action_id, expected) in [
        (AnimatePacketActionId::SwingArm, ActorActionKind::SwingArm),
        (AnimatePacketActionId::WakeUp, ActorActionKind::Wake),
        (
            AnimatePacketActionId::CriticalHit,
            ActorActionKind::CriticalHit,
        ),
        (
            AnimatePacketActionId::MagicCriticalHit,
            ActorActionKind::MagicCriticalHit,
        ),
        (
            AnimatePacketActionId::UnknownValue(128),
            ActorActionKind::RowRight,
        ),
        (
            AnimatePacketActionId::UnknownValue(129),
            ActorActionKind::RowLeft,
        ),
        (
            AnimatePacketActionId::UnknownValue(200),
            ActorActionKind::Ignored { action_id: 200 },
        ),
    ] {
        let packet = AnimatePacket {
            action_id,
            runtime_entity_id: 42,
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
        animation: "animation.test.attack".into(),
        next_state: "default".into(),
        stop_condition: "query.any_animation_finished".into(),
        stop_condition_version: 1,
        controller: "controller.animation.test".into(),
        blend_out_time: 0.1,
        runtime_entity_ids: vec![42, 43],
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
    let packet = AddEntityPacket {
        runtime_id: 42,
        entity_type: "minecraft:bee".into(),
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
        Item {
            network_id: 5,
            content: Some(Box::new(ItemContent {
                count: 0,
                ..Default::default()
            })),
        },
        Item {
            network_id: 5,
            content: Some(Box::new(ItemContent {
                count: 1,
                has_stack_id: 0,
                stack_id: Some(7),
                ..Default::default()
            })),
        },
    ] {
        let packet = AddPlayerPacket {
            runtime_id: 42,
            held_item: item,
            ..Default::default()
        };
        assert!(into_world_event(packet.into(), 0).is_err());
    }

    let signed_wire_fields = AddPlayerPacket {
        runtime_id: 42,
        held_item: Item {
            network_id: -1,
            content: Some(Box::new(ItemContent {
                count: 1,
                metadata: -1,
                ..Default::default()
            })),
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
        runtime_id: 42,
        held_item: Item {
            network_id: 5,
            content: Some(Box::new(ItemContent {
                count: 1,
                extra: ItemContentExtra::Default(ItemExtraDataWithoutBlockingTick {
                    can_place_on: vec!["x".repeat(MAX_ITEM_EXTRA_BYTES + 1)],
                    ..Default::default()
                }),
                ..Default::default()
            })),
        },
        ..Default::default()
    };
    assert!(into_world_event(packet.into(), 0).is_err());

    let invalid_short_string = AddPlayerPacket {
        runtime_id: 42,
        held_item: Item {
            network_id: 5,
            content: Some(Box::new(ItemContent {
                count: 1,
                extra: ItemContentExtra::Default(ItemExtraDataWithoutBlockingTick {
                    can_destroy: vec!["x".repeat(i16::MAX as usize + 1)],
                    ..Default::default()
                }),
                ..Default::default()
            })),
        },
        ..Default::default()
    };
    assert!(into_world_event(invalid_short_string.into(), 0).is_err());

    let invalid_nbt_version = AddPlayerPacket {
        runtime_id: 42,
        held_item: Item {
            network_id: 5,
            content: Some(Box::new(ItemContent {
                count: 1,
                extra: ItemContentExtra::Default(ItemExtraDataWithoutBlockingTick {
                    nbt: Some(ItemExtraDataWithoutBlockingTickNbt {
                        version: 2,
                        ..Default::default()
                    }),
                    ..Default::default()
                }),
                ..Default::default()
            })),
        },
        ..Default::default()
    };
    assert!(into_world_event(invalid_nbt_version.into(), 0).is_err());

    let malformed_nbt = AddPlayerPacket {
        runtime_id: 42,
        held_item: Item {
            network_id: 5,
            content: Some(Box::new(ItemContent {
                count: 1,
                extra: ItemContentExtra::Default(ItemExtraDataWithoutBlockingTick {
                    nbt: Some(ItemExtraDataWithoutBlockingTickNbt {
                        version: 1,
                        nbt: Nbt(Bytes::from_static(&[0xff])),
                    }),
                    ..Default::default()
                }),
                ..Default::default()
            })),
        },
        ..Default::default()
    };
    assert!(into_world_event(malformed_nbt.into(), 0).is_err());
}

#[test]
fn registry_preserves_signed_ids_and_rejects_duplicate_and_oversized_records() {
    let signed_id = ItemRegistryPacket {
        itemstates: vec![ItemstatesItem {
            name: "minecraft:signed".into(),
            runtime_id: -1,
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
        itemstates: vec![ItemstatesItem {
            name: "minecraft:malformed".into(),
            runtime_id: 5,
            nbt: Nbt(Bytes::from_static(&[0xff])),
            ..Default::default()
        }],
    };
    assert!(into_world_event(malformed_nbt.into(), 0).is_err());

    let duplicate = ItemRegistryPacket {
        itemstates: vec![
            ItemstatesItem {
                name: "minecraft:stick".into(),
                runtime_id: 5,
                ..Default::default()
            },
            ItemstatesItem {
                name: "minecraft:stick".into(),
                runtime_id: 6,
                ..Default::default()
            },
        ],
    };
    assert!(into_world_event(duplicate.into(), 0).is_err());

    let long_name = ItemRegistryPacket {
        itemstates: vec![ItemstatesItem {
            name: "x".repeat(MAX_ACTION_IDENTIFIER_BYTES + 1),
            runtime_id: 5,
            ..Default::default()
        }],
    };
    assert!(into_world_event(long_name.into(), 0).is_err());

    let oversized = ItemRegistryPacket {
        itemstates: (0..=MAX_ITEM_REGISTRY_ENTRIES)
            .map(|index| ItemstatesItem {
                name: format!("test:item_{index}"),
                runtime_id: 1,
                ..Default::default()
            })
            .collect(),
    };
    assert!(into_world_event(oversized.into(), 0).is_err());
}

#[test]
fn equipment_rejects_invalid_runtime_slots_window_and_stack() {
    let valid_item = ItemNew {
        network_id: 5,
        count: 1,
        ..Default::default()
    };
    for packet in [
        MobEquipmentPacket {
            runtime_entity_id: 0,
            item: valid_item.clone(),
            slot: 0,
            selected_slot: 0,
            window_id: WindowId::Inventory,
        },
        MobEquipmentPacket {
            runtime_entity_id: 42,
            item: valid_item.clone(),
            slot: 0,
            selected_slot: 9,
            window_id: WindowId::Inventory,
        },
        MobEquipmentPacket {
            runtime_entity_id: 42,
            item: valid_item.clone(),
            slot: 1,
            selected_slot: 0,
            window_id: WindowId::Inventory,
        },
        MobEquipmentPacket {
            runtime_entity_id: 42,
            item: ItemNew {
                extra: ItemNewExtra::Default(ItemExtraDataWithoutBlockingTick {
                    can_place_on: vec!["x".repeat(MAX_ITEM_EXTRA_BYTES + 1)],
                    ..Default::default()
                }),
                ..Default::default()
            },
            slot: 0,
            selected_slot: 0,
            window_id: WindowId::Inventory,
        },
    ] {
        assert!(into_world_event(packet.into(), 0).is_err());
    }

    let signed_window = MobEquipmentPacket {
        runtime_entity_id: 42,
        item: valid_item,
        slot: 0,
        selected_slot: 0,
        window_id: WindowId::None,
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
        animation: "animation.test.attack".into(),
        next_state: "default".into(),
        stop_condition: "query.any_animation_finished".into(),
        stop_condition_version: 1,
        controller: "controller.animation.test".into(),
        blend_out_time: 0.1,
        runtime_entity_ids: targets,
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
            animation: "x".repeat(MAX_ANIMATION_IDENTIFIER_BYTES + 1),
            ..custom_action(vec![42])
        },
        AnimateEntityPacket {
            controller: "x".repeat(MAX_ACTION_IDENTIFIER_BYTES + 1),
            ..custom_action(vec![42])
        },
        AnimateEntityPacket {
            blend_out_time: f32::NAN,
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
            runtime_entity_id: 0,
            action_id: AnimatePacketActionId::SwingArm,
            ..Default::default()
        },
        AnimatePacket {
            runtime_entity_id: 42,
            action_id: AnimatePacketActionId::SwingArm,
            data: f32::INFINITY,
            ..Default::default()
        },
        AnimatePacket {
            runtime_entity_id: 42,
            action_id: AnimatePacketActionId::SwingArm,
            swing_source: Some("x".repeat(MAX_ACTION_IDENTIFIER_BYTES + 1)),
            ..Default::default()
        },
    ] {
        assert!(into_world_event(packet.into(), 0).is_err());
    }
}
