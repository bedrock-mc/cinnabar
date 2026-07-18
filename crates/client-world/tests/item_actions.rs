use std::sync::Arc;

use assets::{
    CompiledEntityAssets, EntityAnimationClip, EntityAnimationLoop, EntityAssetKind,
    EntityAssetSource, EntityAssetSymbol, EntityGeometryScalar, ItemActionPhase,
    ItemDisplayTransform, ItemTextureReference, ItemVisualAlias, ItemVisualDefinition,
    ItemVisualDefinitionRoute, ItemVisualId, ItemVisualKey, ItemVisualRoute, RuntimeAssets,
    RuntimeEntityAssets, encode_entity_blob,
};
use client_world::{
    ActorSourceTick, MAX_ACTION_EVENTS_PER_TICK, MAX_ACTIONS_PER_ACTOR, MAX_ITEM_REGISTRY_RECORDS,
    MAX_PENDING_ITEM_RESOLUTIONS, RemoteActionFallback, WorldStream,
};
use protocol::{
    ActorActionEvent, ActorActionKind, ActorEvent, ActorHandedness, ActorKind, ActorMoveEvent,
    ActorPositionOrigin, ActorRemoveEvent, ActorSpawnEvent, ChangeDimensionEvent, EquipmentEvent,
    ItemActorEvent, ItemRegistryEntry, ItemRegistryEvent, ItemRegistryVersion, NetworkItemStack,
    WorldBootstrap, WorldEvent,
};
use sha2::{Digest, Sha256};

fn item_assets() -> Arc<RuntimeEntityAssets> {
    let compiled = CompiledEntityAssets {
        source_manifest_sha256: [0x31; 32],
        block_visual_count: 1,
        sources: vec![
            EntityAssetSource {
                path: "animations/empty.animation.json".into(),
                source_bytes: 1,
                source_sha256: [0x30; 32],
            },
            EntityAssetSource {
                path: "entity/item_frame.entity.json".into(),
                source_bytes: 1,
                source_sha256: [0x31; 32],
            },
            EntityAssetSource {
                path: "textures/item_texture.json".into(),
                source_bytes: 1,
                source_sha256: [0x32; 32],
            },
            EntityAssetSource {
                path: "textures/items/apple.png".into(),
                source_bytes: 1,
                source_sha256: [0x33; 32],
            },
        ]
        .into_boxed_slice(),
        symbols: vec![
            EntityAssetSymbol {
                kind: EntityAssetKind::Entity,
                identifier: "minecraft:item_frame".into(),
                source_index: 1,
                dependencies: Box::new([]),
            },
            EntityAssetSymbol {
                kind: EntityAssetKind::Animation,
                identifier: "animation.catalog_only".into(),
                source_index: 0,
                dependencies: Box::new([]),
            },
        ]
        .into_boxed_slice(),
        geometries: Box::new([]),
        animation_clips: vec![EntityAnimationClip {
            symbol: 1,
            length_seconds: EntityGeometryScalar::new(1.0).unwrap(),
            loop_mode: EntityAnimationLoop::Once,
            first_channel: 0,
            channel_count: 0,
            source: 0,
        }]
        .into_boxed_slice(),
        animation_channels: Box::new([]),
        animation_keyframes: Box::new([]),
        molang_symbols: Box::new([]),
        molang_expressions: Box::new([]),
        molang_ops: Box::new([]),
        molang_collections: Box::new([]),
        molang_collection_items: Box::new([]),
        controllers: Box::new([]),
        controller_states: Box::new([]),
        controller_animations: Box::new([]),
        controller_transitions: Box::new([]),
        rig_bindings: Box::new([]),
        rig_geometries: Box::new([]),
        rig_animations: Box::new([]),
        rig_controllers: Box::new([]),
        item_visuals: vec![ItemVisualDefinition {
            key: ItemVisualKey {
                identifier: "minecraft:apple".into(),
                metadata: 0,
            },
            source: 2,
            route: ItemVisualDefinitionRoute::Sprite {
                texture: ItemTextureReference {
                    source: 3,
                    variant: 0,
                },
            },
            first_person: ItemDisplayTransform::identity(),
            third_person: ItemDisplayTransform::identity(),
            dropped: ItemDisplayTransform::identity(),
        }]
        .into_boxed_slice(),
        item_visual_aliases: vec![ItemVisualAlias {
            key: ItemVisualKey {
                identifier: "minecraft:red_apple".into(),
                metadata: 0,
            },
            visual: ItemVisualId(0),
        }]
        .into_boxed_slice(),
    };
    let bytes = encode_entity_blob(&compiled).unwrap();
    Arc::new(RuntimeEntityAssets::decode(&bytes).unwrap())
}

fn stream() -> WorldStream {
    WorldStream::new_with_asset_sets(
        WorldBootstrap {
            dimension: 0,
            local_player_runtime_id: 1,
            player_position: [0.0, 64.0, 0.0],
            world_spawn_position: [0, 64, 0],
            air_network_id: 0,
            block_network_ids_are_hashes: false,
        },
        Arc::new(RuntimeAssets::diagnostic()),
        item_assets(),
        [0.0, 64.0, 0.0],
        None,
    )
}

fn stack(network_id: i32, count: u16, extra: &[u8]) -> NetworkItemStack {
    NetworkItemStack {
        network_id,
        metadata: 0,
        stack_network_id: if count == 0 { -1 } else { 91 },
        count,
        nbt_digest: Sha256::digest(extra).into(),
        block_runtime_id: 0,
        extra_data: Arc::from(extra),
    }
}

fn spawn(runtime_id: u64, unique_id: i64, held_item: NetworkItemStack) -> WorldEvent {
    WorldEvent::Actor(ActorEvent::Spawn(ActorSpawnEvent {
        dimension: 0,
        unique_id,
        runtime_id,
        kind: ActorKind::Player {
            uuid: [runtime_id as u8; 16],
            username: format!("player-{runtime_id}").into(),
        },
        game_mode: Some(protocol::ActorGameMode::Survival),
        position: [0.0, 64.0, 0.0],
        velocity: [0.0; 3],
        pitch: 0.0,
        yaw: 0.0,
        head_yaw: 0.0,
        body_yaw: 0.0,
        held_item,
        metadata: Arc::from([]),
        attributes: Arc::from([]),
        properties: Arc::from([]),
    }))
}

fn registry(network_id: i32, identifier: &str) -> WorldEvent {
    WorldEvent::ItemActor(ItemActorEvent::Registry(ItemRegistryEvent {
        entries: Arc::from([ItemRegistryEntry {
            identifier: Arc::from(identifier),
            network_id,
            component_based: false,
            version: ItemRegistryVersion::Legacy,
            component_digest: [7; 32],
        }]),
    }))
}

fn equipment(
    runtime_id: u64,
    item: NetworkItemStack,
    handedness: Option<ActorHandedness>,
) -> WorldEvent {
    WorldEvent::Equipment(EquipmentEvent {
        actor_runtime_id: runtime_id,
        stack: item,
        inventory_slot: 4,
        selected_slot: 4,
        window_id: 0,
        handedness,
    })
}

fn action(runtime_ids: &[u64], kind: ActorActionKind) -> WorldEvent {
    action_with_details(runtime_ids, kind, 0.0, None)
}

fn action_with_details(
    runtime_ids: &[u64],
    kind: ActorActionKind,
    data: f32,
    swing_source: Option<&str>,
) -> WorldEvent {
    WorldEvent::ItemActor(ItemActorEvent::Action(ActorActionEvent {
        actor_runtime_ids: Arc::from(runtime_ids),
        kind,
        data,
        swing_source: swing_source.map(Arc::from),
    }))
}

#[test]
fn retained_item_action_bounds_are_exact() {
    assert_eq!(MAX_ITEM_REGISTRY_RECORDS, 16_384);
    assert_eq!(MAX_PENDING_ITEM_RESOLUTIONS, 1_024);
    assert_eq!(MAX_ACTIONS_PER_ACTOR, 32);
    assert_eq!(MAX_ACTION_EVENTS_PER_TICK, 4_096);
}

#[test]
fn spawn_equipment_resolves_after_registry_without_mutating_stack_identity() {
    let mut stream = stream();
    let held = stack(5, 1, b"exact-extra");
    stream.submit(1, spawn(42, -42, held.clone())).unwrap();

    let unresolved = stream.actor_equipment(42).unwrap().clone();
    assert_eq!(unresolved.item.identifier, None);
    assert_eq!(unresolved.item.visual, ItemVisualRoute::Missing);
    assert_eq!(unresolved.hand, ActorHandedness::Right);
    assert!(unresolved.hand_defaulted);
    assert_eq!(unresolved.event.ingress_sequence, 1);
    assert_eq!(unresolved.event.actor_lifetime, 1);

    stream
        .submit(2, registry(5, "minecraft:red_apple"))
        .unwrap();
    let resolved = stream.actor_equipment(42).unwrap();
    assert_eq!(resolved.item.identity, unresolved.item.identity);
    assert_eq!(
        resolved.item.identifier.as_deref(),
        Some("minecraft:red_apple")
    );
    assert_eq!(
        resolved.item.visual,
        ItemVisualRoute::Compiled(ItemVisualId(0))
    );
    assert_eq!(resolved.event, unresolved.event);
    assert_eq!(stream.pending_item_resolution_count(), 0);
}

#[test]
fn registry_first_and_equipment_replacement_preserve_slots_and_handedness() {
    let mut stream = stream();
    stream.submit(1, registry(5, "minecraft:apple")).unwrap();
    stream
        .submit(2, spawn(42, -42, NetworkItemStack::empty()))
        .unwrap();
    stream
        .submit(
            3,
            equipment(42, stack(5, 1, b"one"), Some(ActorHandedness::Left)),
        )
        .unwrap();
    let left = stream.actor_equipment(42).unwrap();
    assert_eq!(left.hand, ActorHandedness::Left);
    assert!(!left.hand_defaulted);
    assert_eq!(left.inventory_slot, 4);
    assert_eq!(left.selected_slot, 4);
    assert_eq!(left.window_id, 0);
    assert_eq!(left.item.identifier.as_deref(), Some("minecraft:apple"));

    stream
        .submit(
            4,
            equipment(42, stack(5, 2, b"two"), Some(ActorHandedness::Right)),
        )
        .unwrap();
    let right = stream.actor_equipment(42).unwrap();
    assert_eq!(right.hand, ActorHandedness::Right);
    assert!(!right.hand_defaulted);
    assert_eq!(right.item.identity.count, 2);
    assert_eq!(right.event.ingress_sequence, 4);
}

#[test]
fn latest_registry_re_resolves_live_equipment_without_changing_identity() {
    let mut stream = stream();
    stream.submit(1, registry(5, "minecraft:apple")).unwrap();
    stream
        .submit(2, spawn(42, -42, stack(5, 1, b"same")))
        .unwrap();
    let original = stream.actor_equipment(42).unwrap().clone();
    assert_eq!(original.item.identifier.as_deref(), Some("minecraft:apple"));

    stream
        .submit(3, registry(5, "minecraft:red_apple"))
        .unwrap();
    let replaced = stream.actor_equipment(42).unwrap();
    assert_eq!(replaced.item.identity, original.item.identity);
    assert_eq!(
        replaced.item.identifier.as_deref(),
        Some("minecraft:red_apple")
    );
    assert_eq!(
        replaced.item.visual,
        ItemVisualRoute::Compiled(ItemVisualId(0))
    );
    assert_eq!(replaced.event, original.event);
}

#[test]
fn pending_resolution_queue_is_bounded_and_registry_still_resolves_overflow() {
    let mut stream = stream();
    for offset in 0..=MAX_PENDING_ITEM_RESOLUTIONS {
        let runtime_id = 100 + offset as u64;
        stream
            .submit(
                offset as u64 + 1,
                spawn(runtime_id, -(runtime_id as i64), stack(99, 1, b"same")),
            )
            .unwrap();
    }
    assert_eq!(
        stream.pending_item_resolution_count(),
        MAX_PENDING_ITEM_RESOLUTIONS
    );

    stream
        .submit(
            MAX_PENDING_ITEM_RESOLUTIONS as u64 + 2,
            registry(99, "minecraft:apple"),
        )
        .unwrap();
    assert_eq!(stream.pending_item_resolution_count(), 0);
    for runtime_id in [100, 100 + MAX_PENDING_ITEM_RESOLUTIONS as u64] {
        assert_eq!(
            stream.actor_equipment(runtime_id).unwrap().item.visual,
            ItemVisualRoute::Compiled(ItemVisualId(0))
        );
    }
}

#[test]
fn registry_record_bound_accepts_exact_limit_and_rejects_limit_plus_one_atomically() {
    let registry_with_count = |count: usize, first_identifier: &str| {
        WorldEvent::ItemActor(ItemActorEvent::Registry(ItemRegistryEvent {
            entries: (0..count)
                .map(|index| ItemRegistryEntry {
                    identifier: if index == 0 {
                        Arc::from(first_identifier)
                    } else {
                        Arc::from(format!("minecraft:test_item_{index}"))
                    },
                    network_id: index as i32 + 1,
                    component_based: false,
                    version: ItemRegistryVersion::Legacy,
                    component_digest: [index as u8; 32],
                })
                .collect::<Vec<_>>()
                .into(),
        }))
    };

    let mut stream = stream();
    stream
        .submit(1, spawn(42, -42, stack(1, 1, b"same")))
        .unwrap();
    stream
        .submit(
            2,
            registry_with_count(MAX_ITEM_REGISTRY_RECORDS, "minecraft:apple"),
        )
        .unwrap();
    let accepted = stream.actor_equipment(42).unwrap().clone();
    assert_eq!(accepted.item.identifier.as_deref(), Some("minecraft:apple"));

    stream
        .submit(
            3,
            registry_with_count(MAX_ITEM_REGISTRY_RECORDS + 1, "minecraft:red_apple"),
        )
        .unwrap();
    assert_eq!(stream.actor_equipment(42), Some(&accepted));
}

#[test]
fn bad_digest_and_unknown_actor_equipment_are_not_retained() {
    let mut stream = stream();
    stream
        .submit(1, spawn(42, -42, NetworkItemStack::empty()))
        .unwrap();
    let before = stream.actor_equipment(42).unwrap().clone();
    let mut corrupt = stack(5, 1, b"bytes");
    corrupt.nbt_digest = [0xff; 32];
    stream
        .submit(2, equipment(42, corrupt, Some(ActorHandedness::Right)))
        .unwrap();
    assert_eq!(stream.actor_equipment(42), Some(&before));

    stream
        .submit(3, equipment(999, stack(5, 1, b"bytes"), None))
        .unwrap();
    assert!(stream.actor_equipment(999).is_none());
}

#[test]
fn replacement_remove_and_dimension_reset_drop_lifetime_item_state() {
    let mut stream = stream();
    stream.submit(1, spawn(42, -42, stack(5, 1, b"a"))).unwrap();
    let first = stream.actor_equipment(42).unwrap().actor;

    stream.submit(2, spawn(42, -43, stack(5, 1, b"b"))).unwrap();
    let second = stream.actor_equipment(42).unwrap().actor;
    assert_ne!(first, second);
    assert_eq!(second.spawn_revision, 2);

    stream
        .submit(
            3,
            WorldEvent::Actor(ActorEvent::Remove(ActorRemoveEvent {
                dimension: 0,
                unique_id: -43,
            })),
        )
        .unwrap();
    assert!(stream.actor_equipment(42).is_none());

    stream.submit(4, spawn(43, -44, stack(5, 1, b"c"))).unwrap();
    stream
        .submit(
            5,
            WorldEvent::ChangeDimension(ChangeDimensionEvent {
                dimension: 1,
                position: [0.0, 80.0, 0.0],
            }),
        )
        .unwrap();
    assert!(stream.actor_equipment(43).is_none());
}

#[test]
fn session_item_registry_survives_dimension_actor_state_reset() {
    let mut stream = stream();
    stream.submit(1, registry(5, "minecraft:apple")).unwrap();
    stream
        .submit(2, spawn(42, -42, stack(5, 1, b"old")))
        .unwrap();
    stream
        .submit(
            3,
            WorldEvent::ChangeDimension(ChangeDimensionEvent {
                dimension: 1,
                position: [0.0, 80.0, 0.0],
            }),
        )
        .unwrap();

    let mut next_dimension_spawn = spawn(43, -43, stack(5, 1, b"new"));
    let WorldEvent::Actor(ActorEvent::Spawn(spawn)) = &mut next_dimension_spawn else {
        unreachable!()
    };
    spawn.dimension = 1;
    stream.submit(4, next_dimension_spawn).unwrap();

    let equipment = stream.actor_equipment(43).unwrap();
    assert_eq!(equipment.actor.dimension, 1);
    assert_eq!(
        equipment.item.identifier.as_deref(),
        Some("minecraft:apple")
    );
    assert_eq!(
        equipment.item.visual,
        ItemVisualRoute::Compiled(ItemVisualId(0))
    );
}

#[test]
fn unknown_item_stays_missing_and_duplicate_sequence_is_rejected() {
    let mut stream = stream();
    stream
        .submit(1, spawn(42, -42, stack(99, 1, b"a")))
        .unwrap();
    let item = &stream.actor_equipment(42).unwrap().item;
    assert_eq!(item.identifier, None);
    assert_eq!(item.visual, ItemVisualRoute::Missing);
    assert!(stream.submit(1, registry(99, "minecraft:apple")).is_err());
}

#[test]
fn local_actor_is_excluded_from_remote_equipment_and_action_state() {
    let mut stream = stream();
    stream
        .submit(1, spawn(1, -1, stack(5, 1, b"local")))
        .unwrap();
    stream
        .submit(2, spawn(42, -42, NetworkItemStack::empty()))
        .unwrap();

    assert!(stream.actor_equipment(1).is_none());
    assert!(stream.actor_equipment(42).is_some());

    stream
        .submit(3, equipment(1, stack(5, 2, b"replacement"), None))
        .unwrap();
    stream
        .submit(4, action(&[1, 42], ActorActionKind::SwingArm))
        .unwrap();

    assert!(stream.actor_equipment(1).is_none());
    assert!(stream.actor_action(1).is_none());
    assert!(stream.actor_action(42).is_some());
}

#[test]
fn actions_are_fifo_bounded_and_later_ingress_restarts_windup() {
    let mut stream = stream();
    stream
        .submit(1, spawn(42, -42, NetworkItemStack::empty()))
        .unwrap();
    stream
        .submit(
            2,
            WorldEvent::Actor(ActorEvent::Move(ActorMoveEvent {
                dimension: 0,
                runtime_id: 42,
                position: [None; 3],
                position_origin: ActorPositionOrigin::Feet,
                pitch: None,
                yaw: None,
                head_yaw: None,
                on_ground: None,
                teleported: false,
                player_mode: None,
                source_tick: Some(10),
            })),
        )
        .unwrap();
    stream
        .submit(3, action(&[42, 42], ActorActionKind::SwingArm))
        .unwrap();
    let first = stream.actor_action(42).unwrap().clone();
    assert_eq!(stream.actor_action_history(42).len(), 1);
    assert_eq!(first.event.source_tick, ActorSourceTick::IngressSequence(3));
    assert_eq!(first.phase, ItemActionPhase::Windup { elapsed_ticks: 0 });

    stream.advance_actor_interpolation_ticks(1);
    assert_ne!(stream.actor_action(42).unwrap().phase, first.phase);
    stream
        .submit(
            4,
            WorldEvent::Actor(ActorEvent::Move(ActorMoveEvent {
                dimension: 0,
                runtime_id: 42,
                position: [None; 3],
                position_origin: ActorPositionOrigin::Feet,
                pitch: None,
                yaw: None,
                head_yaw: None,
                on_ground: None,
                teleported: false,
                player_mode: None,
                source_tick: Some(11),
            })),
        )
        .unwrap();
    stream
        .submit(5, action(&[42], ActorActionKind::SwingArm))
        .unwrap();
    let restarted = stream.actor_action(42).unwrap();
    assert_eq!(
        restarted.event.source_tick,
        ActorSourceTick::IngressSequence(5)
    );
    assert_eq!(
        restarted.phase,
        ItemActionPhase::Windup { elapsed_ticks: 0 }
    );

    for sequence in 6..46 {
        let kind = if sequence % 2 == 0 {
            ActorActionKind::CriticalHit
        } else {
            ActorActionKind::MagicCriticalHit
        };
        stream.submit(sequence, action(&[42], kind)).unwrap();
    }
    let history = stream.actor_action_history(42);
    assert_eq!(history.len(), MAX_ACTIONS_PER_ACTOR);
    assert_eq!(history.last(), stream.actor_action(42));
}

#[test]
fn action_admission_is_exactly_bounded_per_completed_tick() {
    let mut stream = stream();
    stream
        .submit(1, spawn(42, -42, NetworkItemStack::empty()))
        .unwrap();
    stream
        .submit(2, spawn(43, -43, NetworkItemStack::empty()))
        .unwrap();
    for sequence in 3..=MAX_ACTION_EVENTS_PER_TICK as u64 + 1 {
        stream
            .submit(sequence, action(&[42], ActorActionKind::SwingArm))
            .unwrap();
    }
    assert_eq!(
        stream.actor_action(42).unwrap().event.ingress_sequence,
        MAX_ACTION_EVENTS_PER_TICK as u64 + 1
    );

    stream
        .submit(
            MAX_ACTION_EVENTS_PER_TICK as u64 + 2,
            action(&[42, 43], ActorActionKind::CriticalHit),
        )
        .unwrap();
    assert_eq!(
        stream.actor_action(42).unwrap().kind,
        ActorActionKind::SwingArm
    );
    assert!(stream.actor_action(43).is_none());

    stream
        .submit(
            MAX_ACTION_EVENTS_PER_TICK as u64 + 3,
            action(&[43], ActorActionKind::CriticalHit),
        )
        .unwrap();
    assert_eq!(
        stream.actor_action(43).unwrap().kind,
        ActorActionKind::CriticalHit
    );

    stream.advance_actor_interpolation_ticks(1);
    stream
        .submit(
            MAX_ACTION_EVENTS_PER_TICK as u64 + 4,
            action(&[42], ActorActionKind::CriticalHit),
        )
        .unwrap();
    assert_eq!(
        stream.actor_action(42).unwrap().kind,
        ActorActionKind::CriticalHit
    );
}

#[test]
fn custom_actions_replace_in_fifo_and_teleport_cancels_the_current_lifetime() {
    let mut stream = stream();
    stream
        .submit(1, spawn(42, -42, NetworkItemStack::empty()))
        .unwrap();
    let custom = ActorActionKind::Custom {
        animation: "animation.missing.wave".into(),
        controller: "controller.animation.missing".into(),
    };
    stream
        .submit(2, action_with_details(&[42], custom.clone(), 0.4, None))
        .unwrap();
    let first = stream.actor_action(42).unwrap();
    assert_eq!(first.kind, custom);
    assert_eq!(first.data, 0.4);
    assert_eq!(first.swing_source, None);
    assert_eq!(first.phase, ItemActionPhase::Active { elapsed_ticks: 0 });
    assert_eq!(first.fallback, RemoteActionFallback::StaticPose);

    let catalog_only = ActorActionKind::Custom {
        animation: "animation.catalog_only".into(),
        controller: "".into(),
    };
    stream.submit(3, action(&[42], catalog_only)).unwrap();
    assert_eq!(
        stream.actor_action(42).unwrap().fallback,
        RemoteActionFallback::StaticPose
    );
    assert_eq!(stream.actor_action_stats().static_fallbacks, 2);

    stream
        .submit(
            4,
            action_with_details(&[42], ActorActionKind::RowLeft, 0.25, Some("paddle.left")),
        )
        .unwrap();
    let rowing = stream.actor_action(42).unwrap();
    assert_eq!(rowing.kind, ActorActionKind::RowLeft);
    assert_eq!(rowing.data, 0.25);
    assert_eq!(rowing.swing_source.as_deref(), Some("paddle.left"));
    assert_eq!(
        rowing.phase,
        ItemActionPhase::UseHeld {
            elapsed_ticks: 0,
            duration_ticks: 5,
        }
    );
    assert_eq!(stream.actor_action_history(42).len(), 3);

    stream
        .submit(
            5,
            WorldEvent::Actor(ActorEvent::Move(ActorMoveEvent {
                dimension: 0,
                runtime_id: 42,
                position: [Some(1.0), Some(64.0), Some(1.0)],
                position_origin: ActorPositionOrigin::Feet,
                pitch: None,
                yaw: None,
                head_yaw: None,
                on_ground: Some(true),
                teleported: true,
                player_mode: None,
                source_tick: Some(12),
            })),
        )
        .unwrap();
    assert_eq!(
        stream.actor_action(42).unwrap().phase,
        ItemActionPhase::Cancelled
    );
    assert_eq!(stream.actor_action_history(42).len(), 1);

    stream
        .submit(6, spawn(42, -43, NetworkItemStack::empty()))
        .unwrap();
    assert!(stream.actor_action(42).is_none());
    assert!(stream.actor_action_history(42).is_empty());
}
