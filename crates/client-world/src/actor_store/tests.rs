use std::sync::Arc;

use protocol::{
    ActorAttribute, ActorAttributesUpdateEvent, ActorEvent, ActorGameMode,
    ActorGameModeUpdateEvent, ActorKind, ActorMetadata, ActorMetadataUpdateEvent,
    ActorMetadataValue, ActorMoveEvent, ActorPositionOrigin, ActorProperty, ActorRemoveEvent,
    ActorSpawnEvent, DefaultActorGameModeEvent, PLAYER_NETWORK_OFFSET, PlayerListEntry,
    PlayerListUpdateEvent, PlayerSkin, PlayerSkinUnavailable, StandardSkin,
};

use super::{ActorApplyResult, ActorStore};

fn spawn(runtime_id: u64, unique_id: i64) -> ActorEvent {
    ActorEvent::Spawn(ActorSpawnEvent {
        dimension: 0,
        unique_id,
        runtime_id,
        kind: ActorKind::Entity {
            identifier: "minecraft:bee".into(),
        },
        game_mode: None,
        position: [1.0, 2.0, 3.0],
        velocity: [0.0; 3],
        pitch: 0.0,
        yaw: 0.0,
        head_yaw: 0.0,
        body_yaw: 0.0,
        held_item: Default::default(),
        metadata: Arc::from([]),
        attributes: Arc::from([]),
        properties: Arc::from([]),
    })
}

fn player_spawn(runtime_id: u64, unique_id: i64, x: f32) -> ActorEvent {
    let ActorEvent::Spawn(mut spawn) = spawn(runtime_id, unique_id) else {
        unreachable!();
    };
    spawn.kind = ActorKind::Player {
        uuid: [runtime_id as u8; 16],
        username: format!("player-{runtime_id}").into(),
    };
    spawn.game_mode = Some(ActorGameMode::Survival);
    spawn.position = [x, 64.0, 0.0];
    ActorEvent::Spawn(spawn)
}

fn entity_spawn(
    runtime_id: u64,
    unique_id: i64,
    identifier: &str,
    metadata: Arc<[ActorMetadata]>,
) -> ActorEvent {
    let ActorEvent::Spawn(mut spawn) = spawn(runtime_id, unique_id) else {
        unreachable!();
    };
    spawn.kind = ActorKind::Entity {
        identifier: identifier.into(),
    };
    spawn.position = [0.0, 64.0, 0.0];
    spawn.metadata = metadata;
    ActorEvent::Spawn(spawn)
}

fn network_move(runtime_id: u64, y: f32, teleported: bool) -> ActorEvent {
    ActorEvent::Move(ActorMoveEvent {
        dimension: 0,
        runtime_id,
        position: [None, Some(y), None],
        position_origin: ActorPositionOrigin::NetworkOffset,
        pitch: None,
        yaw: None,
        head_yaw: None,
        on_ground: Some(true),
        teleported,
        snap: teleported,
        player_mode: None,
        source_tick: None,
    })
}

fn player_move(runtime_id: u64, x: f32, teleported: bool) -> ActorEvent {
    ActorEvent::Move(ActorMoveEvent {
        dimension: 0,
        runtime_id,
        position: [Some(x), None, None],
        position_origin: ActorPositionOrigin::Feet,
        pitch: Some(0.0),
        yaw: Some(0.0),
        head_yaw: Some(0.0),
        on_ground: Some(true),
        teleported,
        snap: teleported,
        player_mode: Some(if teleported {
            protocol::MovePlayerMode::Teleport
        } else {
            protocol::MovePlayerMode::Normal
        }),
        source_tick: Some(10),
    })
}

#[test]
fn actor_display_names_use_player_username_and_entity_nametag_authority() {
    let mut store = ActorStore::new(1, 0);
    assert_eq!(
        store.apply(1, 1, player_spawn(7, 70, 0.0)),
        ActorApplyResult::Inserted
    );
    assert_eq!(
        store.apply(
            1,
            2,
            entity_spawn(
                8,
                80,
                "minecraft:bee",
                Arc::from([ActorMetadata {
                    key: 4,
                    value: ActorMetadataValue::String(Arc::from("Beeatrice")),
                }]),
            ),
        ),
        ActorApplyResult::Inserted
    );
    assert_eq!(
        store.apply(1, 3, entity_spawn(9, 90, "minecraft:bee", Arc::from([]))),
        ActorApplyResult::Inserted
    );

    assert_eq!(store.actor_display_name(70), Some(Arc::from("player-7")));
    assert_eq!(store.actor_display_name(80), Some(Arc::from("Beeatrice")));
    assert_eq!(store.actor_display_name(90), None);
}

#[test]
fn player_network_position_is_normalized_to_spawn_feet_space() {
    assert_eq!(PLAYER_NETWORK_OFFSET, 1.62001);
    let mut store = ActorStore::new(1, 0);
    store.apply(1, 1, player_spawn(42, -7, 0.0));
    store.apply(
        1,
        2,
        ActorEvent::Move(ActorMoveEvent {
            dimension: 0,
            runtime_id: 42,
            position: [Some(1.0), Some(64.0 + PLAYER_NETWORK_OFFSET), Some(2.0)],
            position_origin: ActorPositionOrigin::NetworkOffset,
            pitch: None,
            yaw: None,
            head_yaw: None,
            on_ground: Some(true),
            teleported: false,
            snap: false,
            player_mode: None,
            source_tick: None,
        }),
    );

    let actor = store.get(42).expect("stored player");
    assert!((actor.received_pose.position[1] - 64.0).abs() < 1e-5);
}

#[test]
fn sleeping_player_network_position_uses_explicit_sleeping_metadata() {
    const PLAYER_FLAGS_KEY: i32 = 26;
    const PLAYER_SLEEPING_BIT: i8 = 1 << 1;

    let mut store = ActorStore::new(1, 0);
    store.apply(1, 1, player_spawn(42, -7, 0.0));
    store.apply(
        1,
        2,
        ActorEvent::Metadata(ActorMetadataUpdateEvent {
            dimension: 0,
            runtime_id: 42,
            metadata: Arc::from([ActorMetadata {
                key: PLAYER_FLAGS_KEY,
                value: ActorMetadataValue::Byte(PLAYER_SLEEPING_BIT),
            }]),
            properties: Arc::from([]),
            tick: 2,
        }),
    );
    store.apply(1, 3, network_move(42, 64.2, true));

    assert!((store.get(42).unwrap().position[1] - 64.0).abs() < 1e-5);
}

#[test]
fn sleeping_player_network_position_accepts_the_extended_sleeping_flag() {
    const EXTENDED_FLAGS_KEY: i32 = 92;
    const EXTENDED_SLEEPING_BIT: u64 = 1 << 11;

    let mut store = ActorStore::new(1, 0);
    let ActorEvent::Spawn(mut spawn) = player_spawn(42, -7, 0.0) else {
        unreachable!();
    };
    spawn.metadata = Arc::from([ActorMetadata {
        key: EXTENDED_FLAGS_KEY,
        value: ActorMetadataValue::FlagsExtended(EXTENDED_SLEEPING_BIT),
    }]);
    store.apply(1, 1, ActorEvent::Spawn(spawn));
    store.apply(1, 2, network_move(42, 64.2, true));

    assert!((store.get(42).unwrap().position[1] - 64.0).abs() < 1e-5);
}

#[test]
fn spawn_and_partial_positions_never_apply_network_offsets() {
    let mut store = ActorStore::new(1, 0);
    store.apply(1, 1, entity_spawn(42, -7, "minecraft:boat", Arc::from([])));
    assert_eq!(store.get(42).unwrap().position[1], 64.0);

    store.apply(
        1,
        2,
        ActorEvent::Move(ActorMoveEvent {
            dimension: 0,
            runtime_id: 42,
            position: [None, Some(64.375), None],
            position_origin: ActorPositionOrigin::Feet,
            pitch: None,
            yaw: None,
            head_yaw: None,
            on_ground: Some(true),
            teleported: true,
            snap: true,
            player_mode: None,
            source_tick: None,
        }),
    );
    assert_eq!(store.get(42).unwrap().position[1], 64.375);
}

#[test]
fn absolute_network_positions_use_supported_entity_kind_offsets() {
    let cases = [
        ("minecraft:item", 0.5),
        ("minecraft:falling_block", 0.5),
        ("minecraft:minecart", 0.5),
        ("minecraft:chest_minecart", 0.5),
        ("minecraft:hopper_minecart", 0.5),
        ("minecraft:tnt_minecart", 0.5),
        ("minecraft:command_block_minecart", 0.5),
        ("minecraft:boat", 0.375),
    ];

    for (index, (identifier, offset)) in cases.into_iter().enumerate() {
        let runtime_id = index as u64 + 40;
        let mut store = ActorStore::new(1, 0);
        store.apply(
            1,
            1,
            entity_spawn(runtime_id, -(runtime_id as i64), identifier, Arc::from([])),
        );
        store.apply(1, 2, network_move(runtime_id, 64.0 + offset, true));

        assert!(
            (store.get(runtime_id).unwrap().position[1] - 64.0).abs() < 1e-5,
            "{identifier} retained a network-space Y"
        );
    }
}

#[test]
fn unknown_minecraft_suffixes_do_not_inherit_reviewed_offsets() {
    for (index, identifier) in ["minecraft:display_boat", "minecraft:custom_minecart"]
        .into_iter()
        .enumerate()
    {
        let runtime_id = index as u64 + 40;
        let mut store = ActorStore::new(1, 0);
        store.apply(
            1,
            1,
            entity_spawn(runtime_id, -(runtime_id as i64), identifier, Arc::from([])),
        );
        store.apply(1, 2, network_move(runtime_id, 64.75, true));

        assert_eq!(
            store.get(runtime_id).unwrap().position[1],
            64.75,
            "{identifier} inherited an offset without a reviewed mapping"
        );
    }
}

#[test]
fn primed_tnt_network_offset_uses_retained_bounding_box_height() {
    const BOUNDING_BOX_HEIGHT_KEY: i32 = 54;

    let mut store = ActorStore::new(1, 0);
    store.apply(
        1,
        1,
        entity_spawn(
            42,
            -7,
            "minecraft:tnt",
            Arc::from([ActorMetadata {
                key: BOUNDING_BOX_HEIGHT_KEY,
                value: ActorMetadataValue::Float(1.2),
            }]),
        ),
    );
    store.apply(1, 2, network_move(42, 64.6, true));

    assert!((store.get(42).unwrap().position[1] - 64.0).abs() < 1e-5);
}

#[test]
fn primed_tnt_without_height_uses_the_vanilla_default_offset() {
    let mut store = ActorStore::new(1, 0);
    store.apply(1, 1, entity_spawn(42, -7, "minecraft:tnt", Arc::from([])));
    store.apply(1, 2, network_move(42, 64.49, true));

    assert!((store.get(42).unwrap().position[1] - 64.0).abs() < 1e-5);
}

#[test]
fn player_delta_y_is_not_shifted_like_a_network_position() {
    let mut store = ActorStore::new(1, 0);
    store.apply(1, 1, player_spawn(42, -7, 0.0));
    store.apply(
        1,
        2,
        ActorEvent::Move(ActorMoveEvent {
            dimension: 0,
            runtime_id: 42,
            position: [None, Some(64.5), None],
            position_origin: ActorPositionOrigin::Feet,
            pitch: None,
            yaw: None,
            head_yaw: None,
            on_ground: Some(false),
            teleported: true,
            snap: true,
            player_mode: None,
            source_tick: None,
        }),
    );

    assert_eq!(store.get(42).expect("stored player").position[1], 64.5);
}

#[test]
fn non_player_network_position_is_unchanged_without_entity_offset_metadata() {
    let mut store = ActorStore::new(1, 0);
    store.apply(1, 1, spawn(42, -7));
    store.apply(
        1,
        2,
        ActorEvent::Move(ActorMoveEvent {
            dimension: 0,
            runtime_id: 42,
            position: [None, Some(10.621), None],
            position_origin: ActorPositionOrigin::NetworkOffset,
            pitch: None,
            yaw: None,
            head_yaw: None,
            on_ground: Some(true),
            teleported: true,
            snap: true,
            player_mode: None,
            source_tick: None,
        }),
    );

    assert_eq!(store.get(42).expect("stored entity").position[1], 10.621);
}

#[test]
fn player_network_teleport_snaps_to_feet_space() {
    let mut store = ActorStore::new(1, 0);
    store.apply(1, 1, player_spawn(42, -7, 0.0));
    store.apply(
        1,
        2,
        ActorEvent::Move(ActorMoveEvent {
            dimension: 0,
            runtime_id: 42,
            position: [Some(3.0), Some(100.0 + PLAYER_NETWORK_OFFSET), Some(4.0)],
            position_origin: ActorPositionOrigin::NetworkOffset,
            pitch: None,
            yaw: None,
            head_yaw: None,
            on_ground: Some(true),
            teleported: true,
            snap: true,
            player_mode: None,
            source_tick: None,
        }),
    );

    let actor = store.get(42).expect("stored player");
    assert_eq!(actor.previous_pose.position, [3.0, 100.0, 4.0]);
    assert_eq!(actor.position, [3.0, 100.0, 4.0]);
    assert_eq!(actor.received_pose.position, [3.0, 100.0, 4.0]);
    assert_eq!(actor.interpolation_ticks_remaining, 0);
}

#[test]
fn remote_player_converges_to_received_position_in_three_ticks() {
    let mut store = ActorStore::new(1, 0);
    store.apply(1, 1, player_spawn(42, -7, 0.0));
    store.apply(1, 2, player_move(42, 9.0, false));

    let actor = store.get(42).unwrap();
    assert_eq!(actor.position[0], 0.0);
    assert_eq!(actor.received_pose.position[0], 9.0);
    assert_eq!(actor.interpolation_ticks_remaining, 3);

    store.advance_interpolation_ticks(1);
    assert_eq!(store.get(42).unwrap().position[0], 3.0);
    store.advance_interpolation_ticks(1);
    assert_eq!(store.get(42).unwrap().position[0], 6.0);
    store.advance_interpolation_ticks(1);
    let actor = store.get(42).unwrap();
    assert_eq!(actor.position[0], 9.0);
    assert_eq!(actor.interpolation_ticks_remaining, 0);
}

#[test]
fn new_target_restarts_three_ticks_from_current_smoothed_position() {
    let mut store = ActorStore::new(1, 0);
    store.apply(1, 1, player_spawn(42, -7, 0.0));
    store.apply(1, 2, player_move(42, 9.0, false));
    store.advance_interpolation_ticks(1);
    store.apply(1, 3, player_move(42, 12.0, false));

    for expected in [6.0, 9.0, 12.0] {
        store.advance_interpolation_ticks(1);
        assert_eq!(store.get(42).unwrap().position[0], expected);
    }
}

#[test]
fn multiple_packets_before_one_tick_use_only_the_latest_target() {
    let mut store = ActorStore::new(1, 0);
    store.apply(1, 1, player_spawn(42, -7, 0.0));
    store.apply(1, 2, player_move(42, 9.0, false));
    store.apply(1, 3, player_move(42, 12.0, false));
    store.advance_interpolation_ticks(1);

    assert_eq!(store.get(42).unwrap().position[0], 4.0);
}

#[test]
fn teleport_snaps_received_previous_and_current_positions() {
    let mut store = ActorStore::new(1, 0);
    store.apply(1, 1, player_spawn(42, -7, 0.0));
    store.apply(1, 2, player_move(42, 9.0, false));
    store.advance_interpolation_ticks(1);
    store.apply(1, 3, player_move(42, 100.0, true));

    let actor = store.get(42).unwrap();
    assert_eq!(actor.previous_pose.position[0], 100.0);
    assert_eq!(actor.position[0], 100.0);
    assert_eq!(actor.received_pose.position[0], 100.0);
    assert_eq!(actor.interpolation_ticks_remaining, 0);
}

#[test]
fn same_runtime_replacement_discards_pending_player_motion() {
    let mut store = ActorStore::new(1, 0);
    store.apply(1, 1, player_spawn(42, -7, 0.0));
    store.apply(1, 2, player_move(42, 9.0, false));
    assert_eq!(
        store.apply(1, 3, player_spawn(42, -8, 100.0)),
        ActorApplyResult::Replaced
    );

    let actor = store.get(42).unwrap();
    assert_eq!(actor.previous_pose.position[0], 100.0);
    assert_eq!(actor.position[0], 100.0);
    assert_eq!(actor.received_pose.position[0], 100.0);
    assert_eq!(actor.interpolation_ticks_remaining, 0);
}

#[test]
fn actor_lifecycle_applies_fifo_patches_and_removes_by_unique_id() {
    let mut store = ActorStore::new(11, 0);
    assert_eq!(
        store.apply(11, 1, spawn(42, -7)),
        ActorApplyResult::Inserted
    );
    assert_eq!(
        store.apply(
            11,
            2,
            ActorEvent::Move(ActorMoveEvent {
                dimension: 0,
                runtime_id: 42,
                position: [Some(9.0), None, Some(8.0)],
                position_origin: ActorPositionOrigin::Feet,
                pitch: Some(10.0),
                yaw: None,
                head_yaw: None,
                on_ground: Some(true),
                teleported: false,
                snap: false,
                player_mode: None,
                source_tick: None,
            }),
        ),
        ActorApplyResult::Updated
    );
    assert_eq!(
        store.apply(
            11,
            3,
            ActorEvent::Metadata(ActorMetadataUpdateEvent {
                dimension: 0,
                runtime_id: 42,
                metadata: Arc::from([ActorMetadata {
                    key: 4,
                    value: ActorMetadataValue::String("Beeatrice".into()),
                }]),
                properties: Arc::from([ActorProperty::Int { index: 2, value: 5 }]),
                tick: 10,
            }),
        ),
        ActorApplyResult::Updated
    );
    assert_eq!(
        store.apply(
            11,
            4,
            ActorEvent::Attributes(ActorAttributesUpdateEvent {
                dimension: 0,
                runtime_id: 42,
                attributes: Arc::from([ActorAttribute {
                    name: "minecraft:health".into(),
                    min: 0.0,
                    max: 20.0,
                    current: 17.0,
                    default: Some(20.0),
                    modifiers: Arc::from([]),
                }]),
                tick: 11,
            }),
        ),
        ActorApplyResult::Updated
    );

    let actor = store.get(42).expect("stored actor");
    assert_eq!(actor.movement_revision, 2);
    assert_eq!(actor.position, [9.0, 2.0, 8.0]);
    assert_eq!(actor.pitch, 10.0);
    assert_eq!(actor.on_ground, Some(true));
    assert_eq!(
        actor.metadata[&4],
        ActorMetadataValue::String("Beeatrice".into())
    );
    assert_eq!(actor.attributes["minecraft:health"].current, 17.0);

    assert_eq!(
        store.apply(
            11,
            5,
            ActorEvent::Remove(ActorRemoveEvent {
                dimension: 0,
                unique_id: -7,
            }),
        ),
        ActorApplyResult::Removed
    );
    assert!(store.get(42).is_none());
}

#[test]
fn consecutive_teleport_packets_retain_distinct_movement_revisions() {
    let mut store = ActorStore::new(1, 0);
    store.apply(1, 1, spawn(42, -7));
    let teleport = |x| {
        ActorEvent::Move(ActorMoveEvent {
            dimension: 0,
            runtime_id: 42,
            position: [Some(x), None, None],
            position_origin: ActorPositionOrigin::Feet,
            pitch: None,
            yaw: None,
            head_yaw: None,
            on_ground: None,
            teleported: true,
            snap: true,
            player_mode: None,
            source_tick: None,
        })
    };

    assert_eq!(
        store.apply(1, 2, teleport(100.0)),
        ActorApplyResult::Updated
    );
    assert_eq!(store.get(42).unwrap().movement_revision, 2);
    assert_eq!(
        store.apply(1, 3, teleport(200.0)),
        ActorApplyResult::Updated
    );
    let actor = store.get(42).unwrap();
    assert_eq!(actor.movement_revision, 3);
    assert_eq!(actor.position[0], 200.0);
    assert!(actor.teleported);
}

#[test]
fn duplicate_runtime_or_unique_ids_replace_atomically() {
    let mut store = ActorStore::new(1, 0);
    store.apply(1, 1, spawn(10, 20));
    assert_eq!(store.get(10).unwrap().spawn_revision, 1);
    assert_eq!(store.apply(1, 2, spawn(10, 21)), ActorApplyResult::Replaced);
    assert_eq!(store.len(), 1);
    assert_eq!(store.get(10).unwrap().spawn_revision, 2);
    assert_eq!(store.get(10).unwrap().unique_id, 21);

    assert_eq!(store.apply(1, 3, spawn(11, 21)), ActorApplyResult::Replaced);
    assert_eq!(store.len(), 1);
    assert!(store.get(10).is_none());
    assert_eq!(store.get(11).unwrap().unique_id, 21);
}

#[test]
fn remove_and_readd_of_the_same_actor_gets_a_new_spawn_revision() {
    let mut store = ActorStore::new(1, 0);
    store.apply(1, 1, spawn(10, 20));
    store.apply(
        1,
        2,
        ActorEvent::Remove(ActorRemoveEvent {
            dimension: 0,
            unique_id: 20,
        }),
    );
    store.apply(1, 3, spawn(10, 20));

    let actor = store.get(10).unwrap();
    assert_eq!(actor.unique_id, 20);
    assert_eq!(actor.spawn_revision, 3);
}

#[test]
fn capacity_is_bounded_but_existing_actor_replacement_is_allowed() {
    let mut store = ActorStore::with_capacity(1, 0, 2, 2);
    assert_eq!(store.apply(1, 1, spawn(1, 1)), ActorApplyResult::Inserted);
    assert_eq!(store.apply(1, 2, spawn(2, 2)), ActorApplyResult::Inserted);
    assert_eq!(
        store.apply(1, 3, spawn(3, 3)),
        ActorApplyResult::CapacityRejected
    );
    assert_eq!(store.apply(1, 4, spawn(2, 22)), ActorApplyResult::Replaced);
    assert_eq!(store.len(), 2);
}

#[test]
fn cumulative_actor_patches_cannot_exceed_retained_collection_bounds() {
    use protocol::{MAX_ACTOR_ATTRIBUTES, MAX_ACTOR_METADATA_ENTRIES, MAX_ACTOR_PROPERTIES};

    let mut store = ActorStore::new(1, 0);
    store.apply(1, 1, spawn(1, 1));

    let metadata = (0..MAX_ACTOR_METADATA_ENTRIES)
        .map(|key| ActorMetadata {
            key: i32::try_from(key).unwrap(),
            value: ActorMetadataValue::Int(i32::try_from(key).unwrap()),
        })
        .collect::<Vec<_>>();
    assert_eq!(
        store.apply(
            1,
            2,
            ActorEvent::Metadata(ActorMetadataUpdateEvent {
                dimension: 0,
                runtime_id: 1,
                metadata: metadata.into(),
                properties: Arc::from([]),
                tick: 1,
            }),
        ),
        ActorApplyResult::Updated
    );
    assert_eq!(
        store.apply(
            1,
            3,
            ActorEvent::Metadata(ActorMetadataUpdateEvent {
                dimension: 0,
                runtime_id: 1,
                metadata: Arc::from([ActorMetadata {
                    key: i32::try_from(MAX_ACTOR_METADATA_ENTRIES).unwrap(),
                    value: ActorMetadataValue::Int(1),
                }]),
                properties: Arc::from([]),
                tick: 2,
            }),
        ),
        ActorApplyResult::CapacityRejected
    );
    assert_eq!(
        store.get(1).unwrap().metadata.len(),
        MAX_ACTOR_METADATA_ENTRIES
    );

    let attributes = (0..MAX_ACTOR_ATTRIBUTES)
        .map(|index| ActorAttribute {
            name: format!("attribute.{index}").into(),
            min: 0.0,
            max: 1.0,
            current: 1.0,
            default: None,
            modifiers: Arc::from([]),
        })
        .collect::<Vec<_>>();
    store.apply(
        1,
        4,
        ActorEvent::Attributes(ActorAttributesUpdateEvent {
            dimension: 0,
            runtime_id: 1,
            attributes: attributes.into(),
            tick: 3,
        }),
    );
    assert_eq!(
        store.apply(
            1,
            5,
            ActorEvent::Attributes(ActorAttributesUpdateEvent {
                dimension: 0,
                runtime_id: 1,
                attributes: Arc::from([ActorAttribute {
                    name: "attribute.overflow".into(),
                    min: 0.0,
                    max: 1.0,
                    current: 1.0,
                    default: None,
                    modifiers: Arc::from([]),
                }]),
                tick: 4,
            }),
        ),
        ActorApplyResult::CapacityRejected
    );
    assert_eq!(store.get(1).unwrap().attributes.len(), MAX_ACTOR_ATTRIBUTES);

    let properties = (0..MAX_ACTOR_PROPERTIES)
        .map(|index| ActorProperty::Int {
            index: i32::try_from(index).unwrap(),
            value: 1,
        })
        .collect::<Vec<_>>();
    store.apply(
        1,
        6,
        ActorEvent::Metadata(ActorMetadataUpdateEvent {
            dimension: 0,
            runtime_id: 1,
            metadata: Arc::from([]),
            properties: properties.into(),
            tick: 5,
        }),
    );
    assert_eq!(
        store.apply(
            1,
            7,
            ActorEvent::Metadata(ActorMetadataUpdateEvent {
                dimension: 0,
                runtime_id: 1,
                metadata: Arc::from([]),
                properties: Arc::from([ActorProperty::Float {
                    index: i32::try_from(MAX_ACTOR_PROPERTIES).unwrap(),
                    value: 1.0,
                }]),
                tick: 6,
            }),
        ),
        ActorApplyResult::CapacityRejected
    );
    let actor = store.get(1).unwrap();
    assert_eq!(
        actor.int_properties.len() + actor.float_properties.len(),
        MAX_ACTOR_PROPERTIES
    );
}

#[test]
fn stale_session_sequence_and_dimension_are_rejected() {
    let mut store = ActorStore::new(5, 0);
    assert_eq!(
        store.apply(4, 1, spawn(1, 1)),
        ActorApplyResult::StaleSession
    );
    assert_eq!(store.apply(5, 2, spawn(1, 1)), ActorApplyResult::Inserted);
    assert_eq!(
        store.apply(5, 2, spawn(2, 2)),
        ActorApplyResult::StaleSequence
    );
    let mut wrong_dimension = spawn(3, 3);
    let ActorEvent::Spawn(spawn) = &mut wrong_dimension else {
        unreachable!()
    };
    spawn.dimension = 1;
    assert_eq!(
        store.apply(5, 3, wrong_dimension),
        ActorApplyResult::StaleDimension
    );
    assert_eq!(store.len(), 1);
}

#[test]
fn dimension_reset_clears_actors_and_session_reset_also_clears_roster() {
    let mut store = ActorStore::new(1, 0);
    store.apply(1, 1, spawn(1, 1));
    store.apply(
        1,
        2,
        ActorEvent::PlayerList(PlayerListUpdateEvent {
            entries: Arc::from([PlayerListEntry::Add {
                uuid: [7; 16],
                unique_id: 1,
                username: "Alex".into(),
                verified: true,
                skin: protocol::PlayerSkin::Unavailable(
                    protocol::PlayerSkinUnavailable::InvalidDimensions,
                ),
            }]),
        }),
    );
    assert_eq!(store.player_count(), 1);
    assert!(store.equipment(1).is_some());

    assert_eq!(store.reset_dimension(1, 3, 2), ActorApplyResult::Reset);
    assert!(store.is_empty());
    assert_eq!(store.player_count(), 1);
    assert!(store.equipment(1).is_none());

    let mut replacement = spawn(2, 2);
    let ActorEvent::Spawn(replacement_spawn) = &mut replacement else {
        unreachable!()
    };
    replacement_spawn.dimension = 2;
    assert_eq!(store.apply(1, 4, replacement), ActorApplyResult::Inserted);
    assert!(store.equipment(2).is_some());

    store.begin_session(2, 0);
    assert!(store.is_empty());
    assert_eq!(store.player_count(), 0);
    assert!(store.equipment(2).is_none());
    assert_eq!(
        store.apply(1, 5, spawn(3, 3)),
        ActorApplyResult::StaleSession
    );
}

#[test]
fn render_players_join_roster_skins_and_sort_by_runtime_id() {
    let skin = protocol::PlayerSkin::Standard(protocol::StandardSkin {
        width: 64,
        height: 64,
        rgba8: vec![9; 64 * 64 * 4].into(),
    });
    let mut store = ActorStore::new(1, 0);
    for (sequence, runtime_id, unique_id, uuid) in [(1, 20, 2, [2; 16]), (2, 10, 1, [1; 16])] {
        let mut event = spawn(runtime_id, unique_id);
        let ActorEvent::Spawn(spawn) = &mut event else {
            unreachable!()
        };
        spawn.kind = ActorKind::Player {
            uuid,
            username: format!("player-{runtime_id}").into(),
        };
        store.apply(1, sequence, event);
    }
    store.apply(
        1,
        3,
        ActorEvent::PlayerList(PlayerListUpdateEvent {
            entries: Arc::from([PlayerListEntry::Add {
                uuid: [1; 16],
                unique_id: 1,
                username: "player-10".into(),
                verified: true,
                skin: skin.clone(),
            }]),
        }),
    );

    let players = store.render_players(None);
    assert_eq!(
        players
            .iter()
            .map(|(actor, _)| actor.runtime_id)
            .collect::<Vec<_>>(),
        [10, 20]
    );
    assert_eq!(players[0].1.map(|profile| &profile.skin), Some(&skin));
    assert!(players[1].1.is_none());
    assert_eq!(
        store.player_profile(10).map(|profile| &profile.skin),
        Some(&skin),
        "the exact roster profile remains addressable even when remote publication excludes it",
    );

    let remote_players = store.render_players(Some(10));
    assert_eq!(remote_players.len(), 1);
    assert_eq!(remote_players[0].0.runtime_id, 20);
}

#[test]
fn incremental_player_lists_cannot_exceed_the_store_skin_byte_budget() {
    let skin_bytes = 64 * 64 * 4;
    let skin = |value| {
        PlayerSkin::Standard(StandardSkin {
            width: 64,
            height: 64,
            rgba8: vec![value; skin_bytes].into(),
        })
    };
    let add = |uuid, unique_id, skin| {
        ActorEvent::PlayerList(PlayerListUpdateEvent {
            entries: Arc::from([PlayerListEntry::Add {
                uuid,
                unique_id,
                username: format!("player-{unique_id}").into(),
                verified: true,
                skin,
            }]),
        })
    };
    let mut store = ActorStore::with_limits(1, 0, 2, 2, skin_bytes);

    assert_eq!(
        store.apply(1, 1, add([1; 16], 1, skin(1))),
        ActorApplyResult::Updated
    );
    assert_eq!(
        store.apply(1, 2, add([2; 16], 2, skin(2))),
        ActorApplyResult::Updated
    );
    assert_eq!(store.retained_player_skin_bytes, skin_bytes);
    assert_eq!(
        store.players[&[2; 16]].skin,
        PlayerSkin::Unavailable(PlayerSkinUnavailable::RetainedBudgetExceeded)
    );

    let oversized_replacement = PlayerSkin::Standard(StandardSkin {
        width: 128,
        height: 128,
        rgba8: vec![9; 128 * 128 * 4].into(),
    });
    store.apply(1, 3, add([1; 16], 10, oversized_replacement));
    assert_eq!(store.retained_player_skin_bytes, skin_bytes);
    assert_eq!(store.players[&[1; 16]].unique_id, 10);
    assert_eq!(store.players[&[1; 16]].skin, skin(1));

    store.apply(
        1,
        4,
        ActorEvent::PlayerList(PlayerListUpdateEvent {
            entries: Arc::from([PlayerListEntry::Remove { uuid: [1; 16] }]),
        }),
    );
    assert_eq!(store.retained_player_skin_bytes, 0);
    store.apply(1, 5, add([2; 16], 2, skin(3)));
    assert_eq!(store.retained_player_skin_bytes, skin_bytes);
    assert!(matches!(
        store.players[&[2; 16]].skin,
        PlayerSkin::Standard(_)
    ));
}

#[test]
fn actor_snapshot_retains_add_player_game_mode() {
    let mut store = ActorStore::new(1, 0);
    let ActorEvent::Spawn(mut spawn) = player_spawn(42, -7, 0.0) else {
        unreachable!();
    };
    spawn.game_mode = Some(ActorGameMode::Creative);
    store.apply(1, 1, ActorEvent::Spawn(spawn));

    assert_eq!(
        store.get(42).expect("stored player").game_mode,
        Some(ActorGameMode::Creative)
    );
}

#[test]
fn player_invisibility_is_bound_to_the_typed_flags_metadata_bit() {
    const ENTITY_FLAGS_KEY: i32 = 0;
    const INVISIBLE_BIT: u64 = 1 << 5;

    let mut store = ActorStore::new(1, 0);
    store.apply(1, 1, player_spawn(42, -7, 0.0));
    assert!(store.get(42).unwrap().is_render_eligible());

    store.apply(
        1,
        2,
        ActorEvent::Metadata(ActorMetadataUpdateEvent {
            dimension: 0,
            runtime_id: 42,
            metadata: Arc::from([ActorMetadata {
                key: ENTITY_FLAGS_KEY,
                value: ActorMetadataValue::Flags(INVISIBLE_BIT),
            }]),
            properties: Arc::from([]),
            tick: 2,
        }),
    );
    assert!(!store.get(42).unwrap().is_render_eligible());

    store.apply(
        1,
        3,
        ActorEvent::Metadata(ActorMetadataUpdateEvent {
            dimension: 0,
            runtime_id: 42,
            metadata: Arc::from([ActorMetadata {
                key: ENTITY_FLAGS_KEY,
                value: ActorMetadataValue::Flags(0),
            }]),
            properties: Arc::from([]),
            tick: 3,
        }),
    );
    assert!(store.get(42).unwrap().is_render_eligible());

    store.apply(
        1,
        4,
        ActorEvent::Metadata(ActorMetadataUpdateEvent {
            dimension: 0,
            runtime_id: 42,
            metadata: Arc::from([ActorMetadata {
                key: ENTITY_FLAGS_KEY,
                value: ActorMetadataValue::Byte(INVISIBLE_BIT as i8),
            }]),
            properties: Arc::from([]),
            tick: 4,
        }),
    );
    assert!(
        store.get(42).unwrap().is_render_eligible(),
        "the numeric value must not escape the bounded Flags metadata type"
    );
}

#[test]
fn all_add_player_spectator_modes_are_not_render_eligible() {
    for (index, game_mode) in [
        ActorGameMode::SurvivalSpectator,
        ActorGameMode::CreativeSpectator,
        ActorGameMode::Spectator,
    ]
    .into_iter()
    .enumerate()
    {
        let mut store = ActorStore::new(1, 0);
        let ActorEvent::Spawn(mut spawn) = player_spawn(42, -7, 0.0) else {
            unreachable!();
        };
        spawn.game_mode = Some(game_mode);
        store.apply(1, 1, ActorEvent::Spawn(spawn));

        assert!(
            !store.get(42).unwrap().is_render_eligible(),
            "spectator mode {index} remained render eligible"
        );
    }
}

#[test]
fn game_mode_updates_target_unique_identity_and_transition_visibility() {
    let mut store = ActorStore::new(1, 0);
    store.apply(1, 1, player_spawn(42, -7, 0.0));

    assert_eq!(
        store.apply(
            1,
            2,
            ActorEvent::GameMode(ActorGameModeUpdateEvent {
                unique_id: -7,
                game_mode: ActorGameMode::Spectator,
                tick: 20,
            }),
        ),
        ActorApplyResult::Updated
    );
    let actor = store.get(42).unwrap();
    assert_eq!(actor.game_mode, Some(ActorGameMode::Spectator));
    assert_eq!(actor.resolved_game_mode, Some(ActorGameMode::Spectator));
    assert_eq!(actor.game_mode_tick, Some(20));
    assert!(!actor.is_render_eligible());

    assert_eq!(
        store.apply(
            1,
            3,
            ActorEvent::GameMode(ActorGameModeUpdateEvent {
                unique_id: 42,
                game_mode: ActorGameMode::Creative,
                tick: 21,
            }),
        ),
        ActorApplyResult::MissingActor,
        "a runtime ID must not be accepted as the packet's unique-ID authority"
    );
    assert!(!store.get(42).unwrap().is_render_eligible());

    assert_eq!(
        store.apply(
            1,
            4,
            ActorEvent::GameMode(ActorGameModeUpdateEvent {
                unique_id: -7,
                game_mode: ActorGameMode::Creative,
                tick: 22,
            }),
        ),
        ActorApplyResult::Updated
    );
    let actor = store.get(42).unwrap();
    assert_eq!(actor.game_mode, Some(ActorGameMode::Creative));
    assert_eq!(actor.resolved_game_mode, Some(ActorGameMode::Creative));
    assert_eq!(actor.game_mode_tick, Some(22));
    assert!(actor.is_render_eligible());

    assert_eq!(
        store.apply(1, 5, player_spawn(99, -7, 1.0)),
        ActorApplyResult::Replaced
    );
    assert_eq!(
        store.apply(
            1,
            6,
            ActorEvent::GameMode(ActorGameModeUpdateEvent {
                unique_id: -7,
                game_mode: ActorGameMode::Spectator,
                tick: 23,
            }),
        ),
        ActorApplyResult::Updated
    );
    assert!(store.get(42).is_none());
    let replacement = store.get(99).unwrap();
    assert_eq!(replacement.game_mode_tick, Some(23));
    assert!(!replacement.is_render_eligible());
}

#[test]
fn fallback_visibility_tracks_authoritative_default_without_losing_raw_mode() {
    let mut store = ActorStore::new(1, 0);
    let ActorEvent::Spawn(mut fallback) = player_spawn(42, -7, 0.0) else {
        unreachable!();
    };
    fallback.game_mode = Some(ActorGameMode::Fallback);
    store.apply(1, 1, ActorEvent::Spawn(fallback));
    store.apply(1, 2, player_spawn(43, -8, 0.0));
    assert!(store.get(42).unwrap().is_render_eligible());

    assert_eq!(
        store.apply(
            1,
            3,
            ActorEvent::DefaultGameMode(DefaultActorGameModeEvent {
                game_mode: ActorGameMode::Spectator,
            }),
        ),
        ActorApplyResult::Updated
    );
    assert_eq!(
        store.get(42).unwrap().game_mode,
        Some(ActorGameMode::Fallback),
        "fallback resolution must not erase packet attribution"
    );
    assert!(!store.get(42).unwrap().is_render_eligible());
    assert!(
        store.get(43).unwrap().is_render_eligible(),
        "an explicit survival player must not inherit the world default"
    );

    store.apply(
        1,
        4,
        ActorEvent::DefaultGameMode(DefaultActorGameModeEvent {
            game_mode: ActorGameMode::Creative,
        }),
    );
    assert!(store.get(42).unwrap().is_render_eligible());

    store.begin_session(2, 0);
    let ActorEvent::Spawn(mut fallback) = player_spawn(44, -9, 0.0) else {
        unreachable!();
    };
    fallback.game_mode = Some(ActorGameMode::Fallback);
    store.apply(2, 1, ActorEvent::Spawn(fallback));
    assert_eq!(
        store.get(44).unwrap().resolved_game_mode,
        Some(ActorGameMode::Survival),
        "the previous session's default must not leak into a replacement session"
    );
}

#[test]
fn forced_actor_move_snaps_without_teleport_attribution() {
    let mut store = ActorStore::new(1, 0);
    store.apply(1, 1, player_spawn(42, -7, 0.0));
    store.apply(
        1,
        2,
        ActorEvent::Move(ActorMoveEvent {
            dimension: 0,
            runtime_id: 42,
            position: [Some(100.0), Some(72.0), Some(-4.0)],
            position_origin: ActorPositionOrigin::Feet,
            pitch: Some(10.0),
            yaw: Some(20.0),
            head_yaw: Some(30.0),
            on_ground: Some(true),
            teleported: false,
            snap: true,
            player_mode: None,
            source_tick: Some(10),
        }),
    );

    let actor = store.get(42).unwrap();
    assert_eq!(actor.position, [100.0, 72.0, -4.0]);
    assert_eq!(actor.previous_pose, actor.received_pose);
    assert_eq!(actor.previous_pose, actor.current_pose());
    assert_eq!(actor.interpolation_ticks_remaining, 0);
    assert_eq!(actor.velocity, [0.0; 3]);
    assert!(!actor.teleported);
}
