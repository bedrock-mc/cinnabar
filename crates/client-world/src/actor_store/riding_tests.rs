use std::sync::Arc;

use protocol::{
    ActorEvent, ActorKind, ActorLinkEvent, ActorLinkType, ActorRemoveEvent, ActorSpawnEvent,
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

const fn link(
    rider_unique_id: i64,
    ridden_unique_id: i64,
    link_type: ActorLinkType,
) -> ActorLinkEvent {
    ActorLinkEvent {
        dimension: 0,
        ridden_unique_id,
        rider_unique_id,
        link_type,
        immediate: false,
        rider_initiated: false,
    }
}

/// Supplies a player rider and a boat whose native origins differ from collision feet.
fn player_on_boat(offset: Option<[f32; 3]>, link_first: bool) -> ActorStore {
    let mut store = ActorStore::new(1, 0);
    let ActorEvent::Spawn(mut mount) = spawn(20, 20) else {
        unreachable!();
    };
    mount.kind = ActorKind::Entity {
        identifier: "minecraft:boat".into(),
    };
    mount.position = [10.0, 5.0, 10.0];
    mount.yaw = 90.0;
    mount.metadata = Arc::from([protocol::ActorMetadata {
        key: super::SCALE_METADATA_KEY,
        value: protocol::ActorMetadataValue::Float(3.0),
    }]);
    let ActorEvent::Spawn(mut rider) = spawn(10, 10) else {
        unreachable!();
    };
    rider.kind = ActorKind::Player {
        uuid: [0; 16],
        username: "Rider".into(),
    };
    rider.metadata = offset.map_or_else(
        || Arc::from([]),
        |offset| {
            Arc::from([
                protocol::ActorMetadata {
                    key: super::placement::KEY_SEAT_OFFSET,
                    value: protocol::ActorMetadataValue::Vector(offset),
                },
                protocol::ActorMetadata {
                    key: super::SCALE_METADATA_KEY,
                    value: protocol::ActorMetadataValue::Float(2.0),
                },
            ])
        },
    );
    let mut sequence = 1;
    if link_first {
        store.apply_link(1, sequence, link(10, 20, ActorLinkType::Rider));
        sequence += 1;
    }
    store.apply(1, sequence, ActorEvent::Spawn(mount));
    store.apply(1, sequence + 1, ActorEvent::Spawn(rider));
    if !link_first {
        store.apply_link(1, sequence + 2, link(10, 20, ActorLinkType::Rider));
    }
    store
}

/// Streamed seats position the player's native origin; collision feet remove its origin offset.
#[test]
fn streamed_player_seats_use_native_origins_without_rescaling() {
    for link_first in [false, true] {
        for offset in [[0.0; 3], [1.0, 2.0, 0.5], [-1.0, -2.0, -0.5]] {
            let mut store = player_on_boat(Some(offset), link_first);
            let origin = store.get(20).unwrap().native_position();
            let local = store.rider_seat_pose(10).unwrap().0;
            store.advance_interpolation_ticks(1);
            let rider = store.get(10).unwrap();
            let native = rider.native_position();
            let expected = [
                origin[0] - offset[2],
                origin[1] + offset[1],
                origin[2] + offset[0],
            ];
            for axis in 0..3 {
                assert!(
                    (native[axis] - expected[axis]).abs() < 0.00001,
                    "{offset:?}: {native:?}"
                );
                assert!((rider.position[axis] - local[axis]).abs() < 0.00001);
            }
        }
    }
}

/// Late metadata and subsequent updates reach both placements; link removal releases the pose.
#[test]
fn streamed_player_seat_updates_and_dismount_release_placement() {
    let mut store = player_on_boat(None, true);
    let origin = store.get(20).unwrap().native_position();
    assert!(store.rider_seat_pose(10).is_none());
    for (sequence, offset) in [(4, [0.5, 2.0, 1.0]), (5, [-0.5, -1.0, -1.0])] {
        store.apply(
            1,
            sequence,
            ActorEvent::Metadata(protocol::ActorMetadataUpdateEvent {
                dimension: 0,
                runtime_id: 10,
                metadata: Arc::from([protocol::ActorMetadata {
                    key: super::placement::KEY_SEAT_OFFSET,
                    value: protocol::ActorMetadataValue::Vector(offset),
                }]),
                properties: Arc::from([]),
                tick: 0,
            }),
        );
        store.advance_interpolation_ticks(1);
        assert!(
            (store.get(10).unwrap().native_position()[1] - (origin[1] + offset[1])).abs() < 0.00001
        );
    }
    store.apply_link(1, 6, link(10, 20, ActorLinkType::Remove));
    assert!(store.rider_seat_pose(10).is_none());
    let freed = store.get(10).unwrap().position;
    store.advance_interpolation_ticks(1);
    assert_eq!(store.get(10).unwrap().position, freed);
}

#[test]
fn accepts_link_before_spawn_and_preserves_unknown_and_pair_removal() {
    let mut store = ActorStore::new(1, 0);
    assert_eq!(
        store.apply_link(1, 1, link(10, 20, ActorLinkType::Rider)),
        ActorApplyResult::Updated
    );
    assert_eq!(store.ridden_unique_id(10), Some(20));
    assert_eq!(store.apply(1, 2, spawn(10, 10)), ActorApplyResult::Inserted);
    assert_eq!(store.ridden_unique_id(10), Some(20));
    assert_eq!(
        store.apply_link(1, 3, link(10, 99, ActorLinkType::Unknown(9))),
        ActorApplyResult::Updated
    );
    assert_eq!(
        store.apply_link(1, 4, link(10, 99, ActorLinkType::Remove)),
        ActorApplyResult::Updated
    );
    assert_eq!(store.ridden_unique_id(10), Some(20));
    assert_eq!(
        store.apply_link(1, 5, link(10, 30, ActorLinkType::Passenger)),
        ActorApplyResult::Updated
    );
    assert_eq!(store.ridden_unique_id(10), Some(30));
    assert_eq!(
        store.apply_link(1, 6, link(10, 30, ActorLinkType::Remove)),
        ActorApplyResult::Updated
    );
    assert_eq!(store.ridden_unique_id(10), None);
}

#[test]
fn enforces_session_sequence_dimension_and_capacity() {
    let mut store = ActorStore::with_capacity(1, 0, 2, 1);
    assert_eq!(
        store.apply_link(2, 1, link(1, 10, ActorLinkType::Rider)),
        ActorApplyResult::StaleSession
    );
    let mut wrong_dimension = link(1, 10, ActorLinkType::Rider);
    wrong_dimension.dimension = 1;
    assert_eq!(
        store.apply_link(1, 1, wrong_dimension),
        ActorApplyResult::StaleDimension
    );
    assert_eq!(
        store.apply_link(1, 1, link(1, 10, ActorLinkType::Rider)),
        ActorApplyResult::StaleSequence
    );
    assert_eq!(
        store.apply_link(1, 2, link(1, 10, ActorLinkType::Rider)),
        ActorApplyResult::Updated
    );
    assert_eq!(
        store.apply_link(1, 3, link(2, 20, ActorLinkType::Rider)),
        ActorApplyResult::Updated
    );
    assert_eq!(
        store.apply_link(1, 4, link(3, 30, ActorLinkType::Rider)),
        ActorApplyResult::CapacityRejected
    );
    assert_eq!(
        store.apply_link(1, 5, link(1, 11, ActorLinkType::Passenger)),
        ActorApplyResult::Updated
    );
    assert_eq!(store.ridden_unique_id(1), Some(11));
}

#[test]
fn actor_lifetime_cleanup_removes_both_sides_and_reset_clears_links() {
    let mut store = ActorStore::new(1, 0);
    assert_eq!(store.apply(1, 1, spawn(20, 20)), ActorApplyResult::Inserted);
    assert_eq!(
        store.apply_link(1, 2, link(10, 20, ActorLinkType::Rider)),
        ActorApplyResult::Updated
    );
    assert_eq!(
        store.apply_link(1, 3, link(11, 20, ActorLinkType::Passenger)),
        ActorApplyResult::Updated
    );
    assert_eq!(
        store.apply(
            1,
            4,
            ActorEvent::Remove(ActorRemoveEvent {
                dimension: 0,
                unique_id: 20
            })
        ),
        ActorApplyResult::Removed
    );
    assert_eq!(store.ridden_unique_id(10), None);
    assert_eq!(store.ridden_unique_id(11), None);
    assert_eq!(
        store.apply_link(1, 5, link(10, 30, ActorLinkType::Rider)),
        ActorApplyResult::Updated
    );
    assert_eq!(store.reset_dimension(1, 6, 1), ActorApplyResult::Reset);
    assert_eq!(store.ridden_unique_id(10), None);
}

#[test]
fn removal_cleans_link_authority_even_when_the_actor_never_spawned() {
    let mut store = ActorStore::new(1, 0);
    assert_eq!(
        store.apply_link(1, 1, link(10, 20, ActorLinkType::Rider)),
        ActorApplyResult::Updated
    );
    assert_eq!(
        store.apply(
            1,
            2,
            ActorEvent::Remove(ActorRemoveEvent {
                dimension: 0,
                unique_id: 20,
            })
        ),
        ActorApplyResult::MissingActor
    );
    assert_eq!(store.ridden_unique_id(10), None);
}

#[test]
fn runtime_and_unique_id_replacements_cleanup_the_previous_lifetime_links() {
    let mut store = ActorStore::new(1, 0);
    assert_eq!(store.apply(1, 1, spawn(20, 20)), ActorApplyResult::Inserted);
    assert_eq!(
        store.apply_link(1, 2, link(10, 20, ActorLinkType::Rider)),
        ActorApplyResult::Updated
    );
    assert_eq!(store.apply(1, 3, spawn(20, 21)), ActorApplyResult::Replaced);
    assert_eq!(store.ridden_unique_id(10), None);

    assert_eq!(
        store.apply_link(1, 4, link(10, 21, ActorLinkType::Rider)),
        ActorApplyResult::Updated
    );
    assert_eq!(store.apply(1, 5, spawn(22, 21)), ActorApplyResult::Replaced);
    assert_eq!(store.ridden_unique_id(10), None);
}

#[test]
fn beginning_a_new_session_clears_all_link_authority() {
    let mut store = ActorStore::new(1, 0);
    assert_eq!(
        store.apply_link(1, 1, link(10, 20, ActorLinkType::Rider)),
        ActorApplyResult::Updated
    );
    store.begin_session(2, 0);
    assert_eq!(store.ridden_unique_id(10), None);
    assert_eq!(
        store.apply_link(1, 2, link(10, 30, ActorLinkType::Rider)),
        ActorApplyResult::StaleSession
    );
}

#[test]
fn embedded_spawn_links_apply_once_in_packet_order() {
    let mut store = ActorStore::new(1, 0);
    let ActorEvent::Spawn(mut event) = spawn(10, 10) else {
        unreachable!()
    };
    event.links = Arc::from([
        link(10, 20, ActorLinkType::Rider),
        link(10, 30, ActorLinkType::Passenger),
        link(10, 40, ActorLinkType::Unknown(7)),
    ]);
    assert_eq!(
        store.apply(1, 1, ActorEvent::Spawn(event)),
        ActorApplyResult::Inserted
    );
    assert_eq!(store.ridden_unique_id(10), Some(30));
}

#[test]
fn embedded_link_with_a_stale_dimension_rejects_the_composite_spawn() {
    let mut store = ActorStore::new(1, 0);
    let ActorEvent::Spawn(mut event) = spawn(10, 10) else {
        unreachable!()
    };
    let mut stale = link(10, 20, ActorLinkType::Rider);
    stale.dimension = 1;
    event.links = Arc::from([stale]);
    assert_eq!(
        store.apply(1, 1, ActorEvent::Spawn(event)),
        ActorApplyResult::StaleDimension
    );
    assert!(store.is_empty());
    assert_eq!(store.ridden_unique_id(10), None);
}

#[test]
fn rider_with_a_streamed_seat_offset_follows_its_mount() {
    let mut store = ActorStore::new(1, 0);
    let mut mount = spawn(20, 20);
    let ActorEvent::Spawn(spawned) = &mut mount else {
        unreachable!();
    };
    spawned.position = [10.0, 5.0, 10.0];
    let mut rider = spawn(10, 10);
    let ActorEvent::Spawn(spawned) = &mut rider else {
        unreachable!();
    };
    spawned.metadata = Arc::from([protocol::ActorMetadata {
        key: super::placement::KEY_SEAT_OFFSET,
        value: protocol::ActorMetadataValue::Vector([0.0, 1.0, 0.5]),
    }]);
    store.apply(1, 1, mount);
    store.apply(1, 2, rider);
    store.apply_link(1, 3, link(10, 20, ActorLinkType::Rider));
    store.advance_interpolation_ticks(1);
    assert_eq!(store.get(10).unwrap().position, [10.0, 6.0, 10.5]);
}

#[test]
fn seat_rotation_turns_the_rider_body_and_clamps_its_head() {
    let mut store = ActorStore::new(1, 0);
    let ActorEvent::Spawn(mut mount) = spawn(20, 20) else {
        unreachable!();
    };
    mount.yaw = 90.0;
    let ActorEvent::Spawn(mut rider) = spawn(10, 10) else {
        unreachable!();
    };
    rider.head_yaw = -100.0;
    rider.metadata = Arc::from([
        protocol::ActorMetadata {
            key: super::placement::KEY_SEAT_OFFSET,
            value: protocol::ActorMetadataValue::Vector([0.0; 3]),
        },
        protocol::ActorMetadata {
            key: 58,
            value: protocol::ActorMetadataValue::Float(30.0),
        },
        protocol::ActorMetadata {
            key: 60,
            value: protocol::ActorMetadataValue::Float(-90.0),
        },
    ]);
    store.apply(1, 1, ActorEvent::Spawn(mount));
    store.apply(1, 2, ActorEvent::Spawn(rider));
    store.apply_link(1, 3, link(10, 20, ActorLinkType::Rider));
    store.advance_interpolation_ticks(1);
    let rider = store.get(10).unwrap();
    assert_eq!(rider.yaw, 0.0);
    assert_eq!(rider.head_yaw, -30.0);
}

/// A negative seat lock is odd server data: it is skipped instead of panicking in the clamp.
#[test]
fn a_negative_seat_lock_is_skipped_without_panicking() {
    let mut store = ActorStore::new(1, 0);
    let ActorEvent::Spawn(mount) = spawn(20, 20) else {
        unreachable!();
    };
    let ActorEvent::Spawn(mut rider) = spawn(10, 10) else {
        unreachable!();
    };
    rider.head_yaw = 100.0;
    rider.metadata = Arc::from([
        protocol::ActorMetadata {
            key: super::placement::KEY_SEAT_OFFSET,
            value: protocol::ActorMetadataValue::Vector([0.0; 3]),
        },
        protocol::ActorMetadata {
            key: 58,
            value: protocol::ActorMetadataValue::Float(-30.0),
        },
    ]);
    store.apply(1, 1, ActorEvent::Spawn(mount));
    store.apply(1, 2, ActorEvent::Spawn(rider));
    store.apply_link(1, 3, link(10, 20, ActorLinkType::Rider));
    store.advance_interpolation_ticks(1);
    assert_eq!(store.get(10).unwrap().head_yaw, 100.0);
}
