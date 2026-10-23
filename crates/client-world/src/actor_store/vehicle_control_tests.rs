use std::sync::Arc;

use protocol::{
    ActorEvent, ActorKind, ActorLinkEvent, ActorLinkType, ActorMetadata, ActorMetadataValue,
    ActorMoveEvent, ActorPositionOrigin, ActorSpawnEvent,
};
use protocol_ids::ActorDataKey;

use super::{ActorStore, LocalVehicleFeed};

const BOAT: i64 = 20;
const PIG: i64 = 30;
const PLAYER: i64 = 10;
const OTHER_PLAYER: i64 = 11;

fn spawn(unique_id: i64, kind: ActorKind, metadata: Vec<ActorMetadata>) -> ActorEvent {
    ActorEvent::Spawn(ActorSpawnEvent {
        dimension: 0,
        unique_id,
        runtime_id: unique_id as u64,
        kind,
        position: [10.0, 5.0, 10.0],
        velocity: [0.0; 3],
        pitch: 0.0,
        yaw: 0.0,
        head_yaw: 0.0,
        body_yaw: 0.0,
        held_item: Default::default(),
        metadata: Arc::from(metadata),
        attributes: Arc::from([]),
        properties: Arc::from([]),
        links: Arc::from([]),
    })
}

fn entity(identifier: &str) -> ActorKind {
    ActorKind::Entity {
        identifier: identifier.into(),
    }
}

fn player(name: &str) -> ActorKind {
    ActorKind::Player {
        uuid: [0; 16],
        username: name.into(),
    }
}

fn link(rider: i64, link_type: ActorLinkType) -> ActorLinkEvent {
    ActorLinkEvent {
        dimension: 0,
        ridden_unique_id: BOAT,
        rider_unique_id: rider,
        link_type,
        immediate: false,
        rider_initiated: false,
    }
}

/// A boat, a pig and two players, with `boat_metadata` on the boat.
fn harbour(boat_metadata: Vec<ActorMetadata>) -> (ActorStore, u64) {
    let mut store = ActorStore::new(1, 0);
    let mut sequence = 0;
    for event in [
        spawn(BOAT, entity("minecraft:boat"), boat_metadata),
        spawn(PIG, entity("minecraft:pig"), Vec::new()),
        spawn(PLAYER, player("Rider"), Vec::new()),
        spawn(OTHER_PLAYER, player("Other"), Vec::new()),
    ] {
        sequence += 1;
        store.apply(1, sequence, event);
    }
    store.exclude_remote_state_for(PLAYER as u64);
    (store, sequence)
}

fn board(store: &mut ActorStore, sequence: &mut u64, riders: &[(i64, ActorLinkType)]) {
    for (rider, link_type) in riders {
        *sequence += 1;
        store.apply_link(1, *sequence, link(*rider, *link_type));
    }
}

#[test]
fn a_player_boarding_behind_a_mob_takes_the_controlling_seat() {
    let (mut store, mut sequence) = harbour(Vec::new());
    board(
        &mut store,
        &mut sequence,
        &[
            (PIG, ActorLinkType::Rider),
            (PLAYER, ActorLinkType::Passenger),
        ],
    );
    assert_eq!(store.controlling_passenger(BOAT), Some(PLAYER));

    // A second player boards behind the first and does not steer.
    board(
        &mut store,
        &mut sequence,
        &[(OTHER_PLAYER, ActorLinkType::Passenger)],
    );
    assert_eq!(store.controlling_passenger(BOAT), Some(PLAYER));

    // When the steering player leaves, the next passenger in order steers: the mob it
    // displaced, not the player who boarded later.
    board(
        &mut store,
        &mut sequence,
        &[(PLAYER, ActorLinkType::Remove)],
    );
    assert_eq!(store.controlling_passenger(BOAT), Some(PIG));
}

#[test]
fn the_first_player_aboard_steers_whatever_the_link_type() {
    let (mut store, mut sequence) = harbour(Vec::new());
    board(
        &mut store,
        &mut sequence,
        &[
            (PLAYER, ActorLinkType::Passenger),
            (OTHER_PLAYER, ActorLinkType::Rider),
            (PIG, ActorLinkType::Passenger),
        ],
    );
    assert_eq!(store.controlling_passenger(BOAT), Some(PLAYER));
}

#[test]
fn the_controlling_seat_index_selects_the_steering_passenger() {
    let (mut store, mut sequence) = harbour(vec![ActorMetadata {
        key: ActorDataKey::ControllingSeatIndex.wire_value(),
        value: ActorMetadataValue::Int(1),
    }]);
    board(
        &mut store,
        &mut sequence,
        &[
            (PLAYER, ActorLinkType::Rider),
            (OTHER_PLAYER, ActorLinkType::Passenger),
        ],
    );
    assert_eq!(store.controlling_passenger(BOAT), Some(OTHER_PLAYER));
}

fn boat_move(x: f32, teleported: bool) -> ActorEvent {
    ActorEvent::Move(ActorMoveEvent {
        dimension: 0,
        runtime_id: BOAT as u64,
        position: [Some(x), Some(5.0), Some(10.0)],
        position_origin: ActorPositionOrigin::Feet,
        pitch: None,
        yaw: Some(45.0),
        head_yaw: None,
        on_ground: None,
        teleported,
        force_local_move: false,
        player_mode: None,
        source_tick: None,
        interpolation: Default::default(),
    })
}

#[test]
fn a_predicted_vehicle_takes_revision_only_when_teleported() {
    let (mut store, mut sequence) = harbour(Vec::new());
    board(&mut store, &mut sequence, &[(PLAYER, ActorLinkType::Rider)]);
    let feed = LocalVehicleFeed {
        runtime_id: BOAT as u64,
        revision: crate::VehicleRevision { pose: 0, motion: 0 },
        position: [12.0, 5.0, 10.0],
        yaw: 30.0,
        velocity: [0.1, 0.0, 0.0],
        render_position: [12.0, 5.0, 10.0],
        render_yaw: 30.0,
    };
    store.sync_local_vehicle(Some(feed));
    sequence += 1;
    store.apply(1, sequence, boat_move(40.0, false));
    store.advance_interpolation_ticks(1);
    let boat = store.snapshot_by_unique(BOAT).unwrap();
    assert_eq!(boat.position, [12.0, 5.0, 10.0]);
    assert_eq!(boat.yaw, 30.0);
    assert_eq!(store.local_vehicle_revision().pose, 0);

    sequence += 1;
    store.apply(1, sequence, boat_move(40.0, true));
    assert_eq!(store.snapshot_by_unique(BOAT).unwrap().position[0], 40.0);
    assert_eq!(store.local_vehicle_revision().pose, 1);

    // A frame without a new prediction keeps feeding the old pose; the teleport stays the
    // target until prediction restarts from it.
    store.sync_local_vehicle(Some(feed));
    store.advance_interpolation_ticks(1);
    let boat = store.snapshot_by_unique(BOAT).unwrap();
    assert_eq!(boat.last_received_pose().position[0], 40.0);
    assert_eq!(boat.position[0], 40.0);
    store.sync_local_vehicle(Some(LocalVehicleFeed {
        revision: crate::VehicleRevision { pose: 1, motion: 0 },
        position: [41.0, 5.0, 10.0],
        render_position: [41.0, 5.0, 10.0],
        ..feed
    }));
    store.advance_interpolation_ticks(1);
    assert_eq!(store.snapshot_by_unique(BOAT).unwrap().position[0], 41.0);

    // Leaving the boat releases it to the server at once, before any presentation frame.
    board(
        &mut store,
        &mut sequence,
        &[(PLAYER, ActorLinkType::Remove)],
    );
    sequence += 1;
    store.apply(1, sequence, boat_move(50.0, false));
    assert_eq!(
        store
            .snapshot_by_unique(BOAT)
            .unwrap()
            .last_received_pose()
            .position[0],
        50.0
    );
}

/// Server moves accepted while the player does not steer restart its prediction when it
/// steers again, even with no prediction tick in between.
#[test]
fn regaining_control_restarts_prediction_from_moves_made_meanwhile() {
    let (mut store, mut sequence) = harbour(Vec::new());
    board(&mut store, &mut sequence, &[(PLAYER, ActorLinkType::Rider)]);
    store.sync_local_vehicle(Some(LocalVehicleFeed {
        runtime_id: BOAT as u64,
        revision: crate::VehicleRevision { pose: 0, motion: 0 },
        position: [12.0, 5.0, 10.0],
        yaw: 0.0,
        velocity: [0.0; 3],
        render_position: [12.0, 5.0, 10.0],
        render_yaw: 0.0,
    }));
    board(
        &mut store,
        &mut sequence,
        &[(PLAYER, ActorLinkType::Remove)],
    );
    sequence += 1;
    store.apply(1, sequence, boat_move(40.0, false));
    board(&mut store, &mut sequence, &[(PLAYER, ActorLinkType::Rider)]);
    assert_eq!(store.local_vehicle_revision().pose, 1);
}

#[test]
fn a_locked_seat_reports_the_vehicle_turn_and_the_seat_range() {
    let mut store = ActorStore::new(1, 0);
    let float = |key: ActorDataKey, value| ActorMetadata {
        key: key.wire_value(),
        value: ActorMetadataValue::Float(value),
    };
    store.apply(1, 1, spawn(BOAT, entity("minecraft:boat"), Vec::new()));
    store.apply(
        1,
        2,
        spawn(
            PLAYER,
            player("Rider"),
            vec![
                ActorMetadata {
                    key: ActorDataKey::SeatLockPassengerRotation.wire_value(),
                    value: ActorMetadataValue::Byte(1),
                },
                float(ActorDataKey::SeatLockPassengerRotationDegrees, 90.0),
                float(ActorDataKey::SeatRotationOffsetDegrees, -90.0),
            ],
        ),
    );
    store.exclude_remote_state_for(PLAYER as u64);
    assert_eq!(store.rider_view_lock(PLAYER, 0.5), None);
    store.apply_link(1, 3, link(PLAYER, ActorLinkType::Rider));
    store.sync_local_vehicle(Some(LocalVehicleFeed {
        runtime_id: BOAT as u64,
        revision: crate::VehicleRevision { pose: 0, motion: 0 },
        position: [10.0, 5.0, 10.0],
        yaw: 20.0,
        velocity: [0.0; 3],
        // Halfway through the tick on the rider's clock.
        render_position: [10.0, 5.0, 10.0],
        render_yaw: 10.0,
    }));
    store.advance_interpolation_ticks(1);
    let lock = store.rider_view_lock(PLAYER, 0.5).unwrap();
    assert_eq!(lock.vehicle_yaw, 10.0);
    assert_eq!(lock.facing(), -80.0);
    assert_eq!(lock.limit, 90.0);
    // A free seat does not hold the view.
    assert_eq!(store.rider_view_lock(PIG, 0.5), None);
}

/// Before the live actors advance, a passenger's view follows the turn this frame will show.
#[test]
fn a_streamed_vehicle_turns_the_view_as_this_frame_will_show_it() {
    let mut store = ActorStore::new(1, 0);
    store.apply(1, 1, spawn(BOAT, entity("minecraft:boat"), Vec::new()));
    store.apply(1, 2, spawn(OTHER_PLAYER, player("Other"), Vec::new()));
    store.apply(
        1,
        3,
        spawn(
            PLAYER,
            player("Rider"),
            vec![
                ActorMetadata {
                    key: ActorDataKey::SeatLockPassengerRotation.wire_value(),
                    value: ActorMetadataValue::Byte(1),
                },
                ActorMetadata {
                    key: ActorDataKey::SeatLockPassengerRotationDegrees.wire_value(),
                    value: ActorMetadataValue::Float(90.0),
                },
            ],
        ),
    );
    store.exclude_remote_state_for(PLAYER as u64);
    let mut sequence = 3;
    board(
        &mut store,
        &mut sequence,
        &[
            (OTHER_PLAYER, ActorLinkType::Rider),
            (PLAYER, ActorLinkType::Passenger),
        ],
    );
    sequence += 1;
    store.apply(1, sequence, boat_move(10.0, false));
    store.predict_remote_motion(1);
    let predicted = store.rider_view_lock(PLAYER, 0.5).unwrap().vehicle_yaw;
    store.advance_interpolation_ticks(1);
    let shown = store.rider_view_lock(PLAYER, 0.5).unwrap().vehicle_yaw;
    assert_ne!(shown, 0.0);
    assert_eq!(predicted, shown);
}

#[test]
fn a_player_linked_before_it_spawns_still_takes_the_front_seat() {
    let mut store = ActorStore::new(1, 0);
    store.apply(1, 1, spawn(BOAT, entity("minecraft:boat"), Vec::new()));
    store.apply(1, 2, spawn(PIG, entity("minecraft:pig"), Vec::new()));
    let mut sequence = 2;
    board(
        &mut store,
        &mut sequence,
        &[
            (PIG, ActorLinkType::Rider),
            (PLAYER, ActorLinkType::Passenger),
        ],
    );
    assert_eq!(store.controlling_passenger(BOAT), Some(PIG));
    store.apply(1, sequence + 1, spawn(PLAYER, player("Rider"), Vec::new()));
    assert_eq!(store.controlling_passenger(BOAT), Some(PLAYER));
}

/// Links queued before their vehicle exists board in the order they arrived, not by id.
#[test]
fn riders_linked_before_their_vehicle_board_in_arrival_order() {
    let mut store = ActorStore::new(1, 0);
    store.apply(1, 1, spawn(PLAYER, player("Rider"), Vec::new()));
    store.apply(1, 2, spawn(OTHER_PLAYER, player("Other"), Vec::new()));
    let mut sequence = 2;
    board(
        &mut store,
        &mut sequence,
        &[
            (OTHER_PLAYER, ActorLinkType::Rider),
            (PLAYER, ActorLinkType::Passenger),
        ],
    );
    assert_eq!(store.controlling_passenger(BOAT), None);
    store.apply(
        1,
        sequence + 1,
        spawn(BOAT, entity("minecraft:boat"), Vec::new()),
    );
    assert_eq!(store.controlling_passenger(BOAT), Some(OTHER_PLAYER));
}

#[test]
fn a_feed_for_a_boat_the_player_does_not_steer_is_ignored() {
    let (mut store, mut sequence) = harbour(Vec::new());
    board(&mut store, &mut sequence, &[(PIG, ActorLinkType::Rider)]);
    board(
        &mut store,
        &mut sequence,
        &[(PLAYER, ActorLinkType::Passenger)],
    );
    // The pig boarded first, but a player boarding behind a mob steers.
    assert_eq!(store.controlling_passenger(BOAT), Some(PLAYER));
    board(
        &mut store,
        &mut sequence,
        &[(PLAYER, ActorLinkType::Remove)],
    );
    store.sync_local_vehicle(Some(LocalVehicleFeed {
        runtime_id: BOAT as u64,
        revision: crate::VehicleRevision { pose: 0, motion: 0 },
        position: [30.0, 5.0, 10.0],
        yaw: 0.0,
        velocity: [0.0; 3],
        render_position: [30.0, 5.0, 10.0],
        render_yaw: 0.0,
    }));
    store.advance_interpolation_ticks(1);
    assert_eq!(store.snapshot_by_unique(BOAT).unwrap().position[0], 10.0);
}

/// A ride restored on join sends the rider's seat before the local actor exists.
#[test]
fn a_seat_sent_before_the_local_actor_exists_still_seats_it() {
    let mut store = ActorStore::new(1, 0);
    store.exclude_remote_state_for(PLAYER as u64);
    store.apply(1, 1, spawn(BOAT, entity("minecraft:boat"), Vec::new()));
    store.apply(
        1,
        2,
        ActorEvent::Metadata(protocol::ActorMetadataUpdateEvent {
            dimension: 0,
            runtime_id: PLAYER as u64,
            metadata: Arc::from([ActorMetadata {
                key: ActorDataKey::SeatOffset.wire_value(),
                value: ActorMetadataValue::Vector([0.0, -0.2, 0.0]),
            }]),
            properties: Arc::from([]),
            tick: 0,
        }),
    );
    store.apply_link(1, 3, link(PLAYER, ActorLinkType::Rider));
    assert_eq!(store.rider_seat_pose(PLAYER), None);
    store.sync_local_player(
        PLAYER as u64,
        PLAYER,
        &super::local_tests::local_feed(0.0, 0.0),
    );
    let (feet, _) = store
        .rider_seat_pose(PLAYER)
        .expect("the held seat applies");
    assert_eq!([feet[0], feet[2]], [10.0, 10.0]);
    assert_eq!(store.controlling_passenger(BOAT), Some(PLAYER));
}

/// Metadata held for an absent local actor stays within the actor's own metadata limit.
#[test]
fn metadata_held_for_the_absent_local_actor_is_bounded() {
    let limit = protocol::MAX_ACTOR_METADATA_ENTRIES;
    let mut store = ActorStore::new(1, 0);
    store.exclude_remote_state_for(PLAYER as u64);
    for (sequence, keys) in [(1, 0..limit), (2, limit..limit + 8)] {
        store.apply(
            1,
            sequence,
            ActorEvent::Metadata(protocol::ActorMetadataUpdateEvent {
                dimension: 0,
                runtime_id: PLAYER as u64,
                metadata: keys
                    .map(|key| ActorMetadata {
                        key: u32::try_from(key).unwrap(),
                        value: ActorMetadataValue::Int(1),
                    })
                    .collect(),
                properties: Arc::from([]),
                tick: 0,
            }),
        );
    }
    assert_eq!(store.pending_local_metadata.len(), limit);
}

/// A boat removed and respawned under the same ids restarts prediction from the new spawn.
#[test]
fn a_respawned_boat_restarts_prediction() {
    let (mut store, mut sequence) = harbour(Vec::new());
    board(&mut store, &mut sequence, &[(PLAYER, ActorLinkType::Rider)]);
    store.sync_local_vehicle(Some(LocalVehicleFeed {
        runtime_id: BOAT as u64,
        revision: crate::VehicleRevision { pose: 0, motion: 0 },
        position: [12.0, 5.0, 10.0],
        yaw: 0.0,
        velocity: [0.0; 3],
        render_position: [12.0, 5.0, 10.0],
        render_yaw: 0.0,
    }));
    sequence += 1;
    store.apply(
        1,
        sequence,
        ActorEvent::Remove(protocol::ActorRemoveEvent {
            dimension: 0,
            unique_id: BOAT,
        }),
    );
    sequence += 1;
    store.apply(
        1,
        sequence,
        spawn(BOAT, entity("minecraft:boat"), Vec::new()),
    );
    board(&mut store, &mut sequence, &[(PLAYER, ActorLinkType::Rider)]);
    assert_ne!(store.local_vehicle_revision().pose, 0);
}

/// Server motion of the steered boat reaches prediction, so the impulse is not overwritten.
#[test]
fn server_motion_of_the_steered_boat_reaches_prediction() {
    let (mut store, mut sequence) = harbour(Vec::new());
    board(&mut store, &mut sequence, &[(PLAYER, ActorLinkType::Rider)]);
    sequence += 1;
    store.apply_motion(
        sequence,
        protocol::ActorMotionEvent {
            actor_runtime_id: BOAT as u64,
            motion: [0.5, 0.0, 0.0],
            tick: 0,
        },
    );
    assert_eq!(
        store.local_vehicle_revision(),
        crate::VehicleRevision { pose: 0, motion: 1 }
    );
    assert_eq!(
        store.snapshot_by_unique(BOAT).unwrap().native_velocity(),
        [0.5, 0.0, 0.0]
    );
}

/// A steered boat the client does not yet predict, such as one whose seat is unresolved,
/// keeps its streamed movement.
#[test]
fn a_steered_boat_without_prediction_keeps_streamed_moves() {
    let (mut store, mut sequence) = harbour(Vec::new());
    board(&mut store, &mut sequence, &[(PLAYER, ActorLinkType::Rider)]);
    sequence += 1;
    store.apply(1, sequence, boat_move(40.0, false));
    assert_eq!(
        store
            .snapshot_by_unique(BOAT)
            .unwrap()
            .last_received_pose()
            .position[0],
        40.0
    );
}

/// A local spawn's own metadata is newer than values held while its actor was absent.
#[test]
fn a_local_spawn_keeps_its_metadata_over_held_values() {
    let seat = |y| ActorMetadata {
        key: ActorDataKey::SeatOffset.wire_value(),
        value: ActorMetadataValue::Vector([0.0, y, 0.0]),
    };
    let mut store = ActorStore::new(1, 0);
    store.exclude_remote_state_for(PLAYER as u64);
    store.apply(
        1,
        1,
        ActorEvent::Metadata(protocol::ActorMetadataUpdateEvent {
            dimension: 0,
            runtime_id: PLAYER as u64,
            metadata: Arc::from([seat(-0.2)]),
            properties: Arc::from([]),
            tick: 0,
        }),
    );
    store.apply(1, 2, spawn(PLAYER, player("Rider"), vec![seat(0.5)]));
    assert_eq!(
        store.get(PLAYER as u64).unwrap().metadata[&ActorDataKey::SeatOffset.wire_value()],
        ActorMetadataValue::Vector([0.0, 0.5, 0.0])
    );
}

/// The predicted boat displays its fed render pose through actor ticks, at any point of the
/// actor clock, while actor ticks keep the fed tick pose for restarts.
#[test]
fn a_predicted_boat_shows_its_render_pose_on_the_rider_s_clock() {
    let (mut store, mut sequence) = harbour(Vec::new());
    board(&mut store, &mut sequence, &[(PLAYER, ActorLinkType::Rider)]);
    store.sync_local_vehicle(Some(LocalVehicleFeed {
        runtime_id: BOAT as u64,
        revision: crate::VehicleRevision { pose: 0, motion: 0 },
        position: [12.0, 5.0, 10.0],
        yaw: 30.0,
        velocity: [0.2, 0.0, 0.0],
        render_position: [11.9, 5.0, 10.0],
        render_yaw: 25.0,
    }));
    store.advance_interpolation_ticks(2);
    let boat = store.snapshot_by_unique(BOAT).unwrap();
    for alpha in [0.0, 0.4, 1.0] {
        assert_eq!(boat.interpolated_position(alpha), Some([11.9, 5.0, 10.0]));
    }
    assert_eq!(boat.yaw, 25.0);
    assert_eq!(boat.status.predicted_render_yaw, Some(25.0));
    assert_eq!(boat.last_received_pose().position, [12.0, 5.0, 10.0]);
    // Without a feed the boat returns to its ticked pose and body yaw.
    store.sync_local_vehicle(None);
    store.advance_interpolation_ticks(1);
    let boat = store.snapshot_by_unique(BOAT).unwrap();
    assert_eq!(boat.status.predicted_render_yaw, None);
    assert_eq!(boat.position, [12.0, 5.0, 10.0]);
}

/// Before the live actors advance, a locked seat on the predicted boat follows the boat's
/// latest feed rather than the picks predicted before it.
#[test]
fn a_predicted_boat_turns_the_seat_as_its_latest_feed_renders_it() {
    let (mut store, mut sequence) = harbour(Vec::new());
    let lock = |key: ActorDataKey, value| ActorMetadata {
        key: key.wire_value(),
        value,
    };
    sequence += 1;
    store.apply(
        1,
        sequence,
        spawn(
            PLAYER,
            player("Rider"),
            vec![
                lock(
                    ActorDataKey::SeatLockPassengerRotation,
                    ActorMetadataValue::Byte(1),
                ),
                lock(
                    ActorDataKey::SeatLockPassengerRotationDegrees,
                    ActorMetadataValue::Float(90.0),
                ),
            ],
        ),
    );
    store.exclude_remote_state_for(PLAYER as u64);
    board(&mut store, &mut sequence, &[(PLAYER, ActorLinkType::Rider)]);
    store.predict_remote_motion(1);
    store.sync_local_vehicle(Some(LocalVehicleFeed {
        runtime_id: BOAT as u64,
        revision: crate::VehicleRevision { pose: 0, motion: 0 },
        position: [10.0, 5.0, 10.0],
        yaw: 40.0,
        velocity: [0.0; 3],
        render_position: [10.0, 5.0, 10.0],
        render_yaw: 30.0,
    }));
    let lock = store.rider_view_lock(PLAYER, 0.5).unwrap();
    assert_eq!(lock.vehicle_runtime_id, BOAT as u64);
    assert_eq!(lock.vehicle_yaw, 30.0);
}
