use protocol_ids::{ActorDataKey, Attribute};
use std::sync::Arc;

use protocol::{
    ActorAttribute, ActorAttributesUpdateEvent, ActorEvent, ActorKind, ActorMetadata,
    ActorMetadataUpdateEvent, ActorMetadataValue, ActorMoveEvent, ActorPositionOrigin,
    ActorProperty, ActorRemoveEvent, ActorSpawnEvent, PLAYER_NETWORK_OFFSET, PlayerListEntry,
    PlayerListUpdateEvent, PlayerSkin, PlayerSkinUnavailable,
};
use render_api::StandardSkin;

use super::{ActorApplyResult, ActorStore};

pub(super) fn spawn(runtime_id: u64, unique_id: i64) -> ActorEvent {
    ActorEvent::Spawn(ActorSpawnEvent {
        dimension: 0,
        unique_id,
        runtime_id,
        kind: ActorKind::Entity {
            identifier: "minecraft:bee".into(),
        },
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
        links: Arc::from([]),
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
    spawn.position = [x, 64.0, 0.0];
    ActorEvent::Spawn(spawn)
}

pub(super) fn entity_spawn(
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

pub(super) fn network_move(runtime_id: u64, y: f32, teleported: bool) -> ActorEvent {
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
        force_local_move: false,
        player_mode: None,
        source_tick: None,
        interpolation: Default::default(),
    })
}

pub(super) fn player_move(runtime_id: u64, x: f32, teleported: bool) -> ActorEvent {
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
        force_local_move: false,
        player_mode: Some(if teleported {
            protocol::MovePlayerMode::Teleport
        } else {
            protocol::MovePlayerMode::Normal
        }),
        source_tick: Some(10),
        interpolation: Default::default(),
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
fn synced_player_tag_overrides_username_without_renaming_scoreboard_identity() {
    let mut store = ActorStore::new(1, 0);
    store.apply(1, 1, player_spawn(7, 70, 0.0));
    assert_eq!(store.actor_name_tag(70), Some(Arc::from("player-7")));
    for (tick, tag) in [(2, "§aRank\nPlayer"), (3, "")] {
        store.apply(
            1,
            tick,
            ActorEvent::Metadata(ActorMetadataUpdateEvent {
                dimension: 0,
                runtime_id: 7,
                metadata: Arc::from([ActorMetadata {
                    key: ActorDataKey::Name.wire_value(),
                    value: ActorMetadataValue::String(tag.into()),
                }]),
                properties: Arc::from([]),
                tick,
            }),
        );
        assert_eq!(
            store.actor_name_tag(70),
            (!tag.is_empty()).then(|| Arc::from(tag))
        );
        assert_eq!(store.actor_display_name(70), Some(Arc::from("player-7")));
    }
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
            force_local_move: false,
            player_mode: None,
            source_tick: None,
            interpolation: Default::default(),
        }),
    );

    let actor = store.get(42).expect("stored player");
    assert!((actor.received_pose.position[1] - 64.0).abs() < 1e-5);
}

#[test]
fn sleeping_player_network_position_uses_explicit_sleeping_metadata() {
    const PLAYER_FLAGS_KEY: u32 = 26;
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
    const EXTENDED_FLAGS_KEY: u32 = 92;
    // Sleeping is actor flag 76, bit 12 of the overflow word.
    const EXTENDED_SLEEPING_BIT: u64 = 1 << 12;

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

    // The neighbouring overflow bit is a shield flag, not sleeping.
    let mut store = ActorStore::new(1, 0);
    let ActorEvent::Spawn(mut spawn) = player_spawn(42, -7, 0.0) else {
        unreachable!();
    };
    spawn.metadata = Arc::from([ActorMetadata {
        key: EXTENDED_FLAGS_KEY,
        value: ActorMetadataValue::FlagsExtended(1 << 11),
    }]);
    store.apply(1, 1, ActorEvent::Spawn(spawn));
    store.apply(1, 2, network_move(42, 64.2, true));
    assert!((store.get(42).unwrap().position[1] - (64.2 - PLAYER_NETWORK_OFFSET)).abs() < 1e-5);
}

/// A boat's spawn position is its native origin, like movement; feet-origin moves are not.
#[test]
fn boat_spawns_apply_the_network_offset_and_feet_moves_do_not() {
    let mut store = ActorStore::new(1, 0);
    store.apply(1, 1, entity_spawn(42, -7, "minecraft:boat", Arc::from([])));
    assert_eq!(store.get(42).unwrap().position[1], 64.0 - 0.375);

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
            force_local_move: false,
            player_mode: None,
            source_tick: None,
            interpolation: Default::default(),
        }),
    );
    assert_eq!(store.get(42).unwrap().position[1], 64.375);
}

#[test]
fn absolute_network_positions_use_supported_entity_kind_offsets() {
    let cases = [
        ("minecraft:item", super::ITEM_ACTOR_NETWORK_OFFSET),
        ("minecraft:falling_block", 0.5),
        ("minecraft:minecart", 0.5),
        ("minecraft:chest_minecart", 0.5),
        ("minecraft:hopper_minecart", 0.5),
        ("minecraft:tnt_minecart", 0.5),
        ("minecraft:command_block_minecart", 0.5),
        ("minecraft:boat", 0.375),
        ("minecraft:chest_boat", 0.375),
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
    const BOUNDING_BOX_HEIGHT_KEY: u32 = 54;

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
fn feet_origin_y_is_not_shifted_like_a_network_position() {
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
            force_local_move: false,
            player_mode: None,
            source_tick: None,
            interpolation: Default::default(),
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
            force_local_move: false,
            player_mode: None,
            source_tick: None,
            interpolation: Default::default(),
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
            force_local_move: false,
            player_mode: None,
            source_tick: None,
            interpolation: Default::default(),
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
                force_local_move: false,
                player_mode: None,
                source_tick: None,
                interpolation: Default::default(),
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
                    name: Attribute::Health.wire_name().into(),
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
    assert_eq!(actor.received_pose.position, [9.0, 2.0, 8.0]);
    assert_eq!(actor.received_pose.pitch, 10.0);
    assert_eq!(actor.on_ground, Some(true));
    let mut interpolated = store.get(42).unwrap().clone();
    let start = interpolated.position;
    store.advance_interpolation_ticks(1);
    interpolated = store.get(42).unwrap().clone();
    assert!((interpolated.position[0] - (start[0] + (9.0 - start[0]) / 3.0)).abs() < 1.0e-5);
    store.advance_interpolation_ticks(2);
    let actor = store.get(42).expect("stored actor");
    assert_eq!(actor.position, [9.0, 2.0, 8.0]);
    assert_eq!(actor.pitch, 10.0);
    assert_eq!(
        actor.metadata[&4],
        ActorMetadataValue::String("Beeatrice".into())
    );
    assert_eq!(
        actor.attributes[Attribute::Health.wire_name()].current,
        17.0
    );

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
            force_local_move: false,
            player_mode: None,
            source_tick: None,
            interpolation: Default::default(),
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
            key: u32::try_from(key).unwrap(),
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
                    key: u32::try_from(MAX_ACTOR_METADATA_ENTRIES).unwrap(),
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
            index: u32::try_from(index).unwrap(),
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
                    index: u32::try_from(MAX_ACTOR_PROPERTIES).unwrap(),
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
                xuid: "".into(),
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
fn remote_rotation_steps_the_short_way_across_the_wrap() {
    let mut store = ActorStore::new(11, 0);
    store.apply(11, 1, spawn(42, -7));
    let turn = |sequence, yaw| {
        ActorEvent::Move(ActorMoveEvent {
            dimension: 0,
            runtime_id: 42,
            position: [None; 3],
            position_origin: ActorPositionOrigin::Feet,
            pitch: None,
            yaw: Some(yaw),
            head_yaw: Some(yaw),
            on_ground: None,
            teleported: false,
            force_local_move: false,
            player_mode: None,
            source_tick: Some(sequence),
            interpolation: Default::default(),
        })
    };
    store.apply(11, 2, turn(2, 170.0));
    store.advance_interpolation_ticks(3);
    assert_eq!(store.get(42).unwrap().yaw, 170.0);
    store.apply(11, 3, turn(3, -170.0));
    store.advance_interpolation_ticks(1);
    let yaw = store.get(42).unwrap().yaw;
    assert!((yaw - (170.0 + 20.0 / 3.0)).abs() < 1.0e-4, "{yaw}");
    store.advance_interpolation_ticks(2);
    assert_eq!(store.get(42).unwrap().head_yaw, -170.0);
}

#[test]
fn movement_flag_update_reads_both_flag_words_and_skips_absent_ones() {
    use protocol::{ActorMetadata, ActorMetadataValue};
    let primary = ActorMetadata {
        key: 0,
        value: ActorMetadataValue::Flags((1 << 1) | (1 << 32)),
    };
    let extended = ActorMetadata {
        key: 92,
        value: ActorMetadataValue::FlagsExtended(1 << (114 - 64)),
    };
    let update = crate::MovementFlagUpdate::from_metadata(&[primary.clone(), extended]).unwrap();
    assert_eq!(update.sneaking, Some(true));
    assert_eq!(update.sprinting, Some(false));
    assert_eq!(update.gliding, Some(true));
    assert_eq!(update.swimming, Some(false));
    assert_eq!(update.crawling, Some(true));

    let primary_only = crate::MovementFlagUpdate::from_metadata(&[primary]).unwrap();
    assert_eq!(primary_only.crawling, None);
    let unrelated = ActorMetadata {
        key: 4,
        value: ActorMetadataValue::String("name".into()),
    };
    assert_eq!(crate::MovementFlagUpdate::from_metadata(&[unrelated]), None);
}

/// Remote MovePlayer Rotation turns without moving and Reset snaps without a lerp.
#[test]
fn remote_move_player_rotation_and_reset_modes_follow_vanilla() {
    let mut store = ActorStore::new(1, 0);
    store.apply(1, 1, player_spawn(42, -7, 0.0));
    let before = store.get(42).unwrap().position;
    let movement = |mode, x: f32| protocol::MovePlayerEvent {
        runtime_id: 42,
        position: [x, 80.0, 0.0],
        yaw: 90.0,
        head_yaw: 90.0,
        mode,
        ..protocol::MovePlayerEvent::default()
    };
    store.apply_player_move(1, 2, 0, movement(protocol::MovePlayerMode::Rotation, 30.0));
    store.advance_interpolation_ticks(3);
    let turned = store.get(42).unwrap();
    assert_eq!(
        turned.position, before,
        "rotation mode never moves the player"
    );
    assert_eq!(turned.yaw, 90.0);

    store.apply_player_move(1, 3, 0, movement(protocol::MovePlayerMode::Reset, 30.0));
    let reset = store.get(42).unwrap();
    assert_eq!(
        reset.position[0], 30.0,
        "reset sets the position without a lerp"
    );
    assert_eq!(reset.interpolation_ticks_remaining, 0);
}

/// 1.26.50.26 samples 0.66 of the authoritative body height.
#[test]
fn brightness_sample_uses_interpolated_feet_and_body_height() {
    for height in [0.25, 1.8, 3.6] {
        let ActorEvent::Spawn(mut event) = spawn(5, 50) else {
            unreachable!()
        };
        event.metadata = Arc::from([
            ActorMetadata {
                key: 53,
                value: ActorMetadataValue::Float(0.6),
            },
            ActorMetadata {
                key: 54,
                value: ActorMetadataValue::Float(height),
            },
        ]);
        let mut store = ActorStore::new(1, 0);
        store.apply(1, 1, ActorEvent::Spawn(event));
        let feet = [-0.25, -1.5, 0.5];
        let sample = store.get(5).unwrap().brightness_sample_position(feet);
        assert_eq!([sample[0], sample[2]], [feet[0], feet[2]]);
        assert!((sample[1] - (feet[1] + 0.66 * height)).abs() < 1e-6);
    }
}

mod bounds;
mod local_server_swings;
mod player_list;

#[test]
fn always_show_name_tag_sentinel_preserves_the_previous_effective_value() {
    let mut store = ActorStore::new(1, 0);
    store.apply(1, 1, player_spawn(7, 70, 0.0));
    let key = ActorDataKey::AlwaysShowNameTag.wire_value();
    for (index, (value, expected)) in [
        (-1, None),
        (1, Some(1)),
        (-1, Some(1)),
        (0, Some(0)),
        (-1, Some(0)),
    ]
    .into_iter()
    .enumerate()
    {
        store.apply(
            1,
            index as u64 + 2,
            ActorEvent::Metadata(ActorMetadataUpdateEvent {
                dimension: 0,
                runtime_id: 7,
                metadata: Arc::from([ActorMetadata {
                    key,
                    value: ActorMetadataValue::Byte(value),
                }]),
                properties: Arc::from([]),
                tick: 0,
            }),
        );
        assert_eq!(
            store.snapshot_by_unique(70).unwrap().metadata.get(&key),
            expected.map(ActorMetadataValue::Byte).as_ref()
        );
    }
}
