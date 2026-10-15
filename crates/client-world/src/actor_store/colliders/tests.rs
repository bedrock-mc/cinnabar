use std::sync::atomic::Ordering;

use super::*;
use crate::actor_store::{
    ActorApplyResult, ActorStore,
    tests::{entity_spawn, network_move},
};
use protocol::{ActorEvent, ActorMetadata, ActorMetadataUpdateEvent, ActorMetadataValue};
use protocol_ids::{ActorDataKey, ActorFlag};

/// Admits an event at the current session's next sequence.
fn apply(store: &mut ActorStore, event: ActorEvent) {
    let result = store.apply(store.session_id, store.latest_sequence + 1, event);
    assert!(matches!(
        result,
        ActorApplyResult::Inserted
            | ActorApplyResult::Replaced
            | ActorApplyResult::Updated
            | ActorApplyResult::Removed
    ));
}

/// Supplies the synced solid flag without assuming a particular constructor is solid.
fn solid_flag() -> ActorMetadataValue {
    ActorMetadataValue::FlagsExtended(1 << (ActorFlag::Collidable.wire_value() - 64))
}

/// Spawns distinct boxes so runtime ordering and filter changes remain observable.
fn spawn(store: &mut ActorStore, runtime: u64, unique: i64, solid: bool) {
    let ActorEvent::Spawn(mut event) = entity_spawn(runtime, unique, "minecraft:bee", [].into())
    else {
        unreachable!()
    };
    event.dimension = store.dimension;
    event.position[0] = runtime as f32;
    event.metadata = [ActorMetadata {
        key: ActorDataKey::FlagsTwo.wire_value(),
        value: if solid {
            solid_flag()
        } else {
            ActorMetadataValue::FlagsExtended(0)
        },
    }]
    .into();
    apply(store, ActorEvent::Spawn(event));
}

/// Replaces one metadata field through the same path as server updates.
fn metadata(store: &mut ActorStore, runtime: u64, key: ActorDataKey, value: ActorMetadataValue) {
    let dimension = store.dimension;
    apply(
        store,
        ActorEvent::Metadata(ActorMetadataUpdateEvent {
            dimension,
            runtime_id: runtime,
            metadata: [ActorMetadata {
                key: key.wire_value(),
                value,
            }]
            .into(),
            properties: [].into(),
            tick: 0,
        }),
    );
}

/// Moves feet through ordinary interpolation or an immediate teleport.
fn move_to(store: &mut ActorStore, runtime: u64, x: f32, teleported: bool) {
    let ActorEvent::Move(mut event) = network_move(runtime, 64.0, teleported) else {
        unreachable!()
    };
    event.dimension = store.dimension;
    event.position = [Some(x), None, None];
    event.position_origin = protocol::ActorPositionOrigin::Feet;
    event.on_ground = None;
    apply(store, ActorEvent::Move(event));
}

/// Compares cached output with a fresh query of every actor, independently of retained membership.
fn current(store: &ActorStore, mover: u64, rider: i64) -> Arc<[ActorCollisionBounds]> {
    let mount = store.ridden_unique_id(rider);
    let mut actors: Vec<_> = store.actors.values().collect();
    actors.sort_unstable_by_key(|actor| actor.runtime_id);
    let expected: Vec<_> = actors
        .into_iter()
        .filter(|actor| {
            actor.is_collidable() && actor.runtime_id != mover && Some(actor.unique_id) != mount
        })
        .filter_map(ActorSnapshot::movement_collision_bounds)
        .collect();
    let actual = store.movement_collider_snapshot(mover, rider);
    assert_eq!(&*actual, expected.as_slice());
    actual
}

/// Changes a rider link through guarded ingress, including links to mounts not yet spawned.
fn link(store: &mut ActorStore, rider: i64, mount: i64, kind: protocol::ActorLinkType) {
    assert_eq!(
        store.apply_link(
            store.session_id,
            store.latest_sequence + 1,
            protocol::ActorLinkEvent {
                dimension: store.dimension,
                ridden_unique_id: mount,
                rider_unique_id: rider,
                link_type: kind,
                immediate: false,
                rider_initiated: false,
            }
        ),
        ActorApplyResult::Updated
    );
}

/// Admits a server-collidable dragon and starts its fixed-tick death lift.
fn dying_solid_dragon() -> ActorStore {
    let mut store = ActorStore::new(1, 0);
    apply(
        &mut store,
        entity_spawn(
            7,
            17,
            "minecraft:ender_dragon",
            [
                ActorMetadata {
                    key: ActorDataKey::FlagsTwo.wire_value(),
                    value: solid_flag(),
                },
                ActorMetadata {
                    key: ActorDataKey::Width.wire_value(),
                    value: ActorMetadataValue::Float(2.0),
                },
                ActorMetadata {
                    key: ActorDataKey::Height.wire_value(),
                    value: ActorMetadataValue::Float(4.0),
                },
            ]
            .into(),
        ),
    );
    apply(
        &mut store,
        ActorEvent::Status(protocol::ActorStatusEvent {
            runtime_id: 7,
            kind: protocol::ActorStatusKind::Death,
            data: 0,
        }),
    );
    store
}

#[test]
fn dragon_death_lifts_refresh_exact_bounds_and_preserve_old_snapshots() {
    let mut store = dying_solid_dragon();
    let initial = current(&store, 0, 0);
    let (min, max) = initial[0];
    let height = max[1] - min[1];
    let mut y = min[1];
    for _ in 0..6 {
        assert_eq!(store.get(7).unwrap().interpolation_ticks_remaining, 0);
        let previous = current(&store, 0, 0);
        let previous_bounds = previous[0];
        store.advance_interpolation_ticks(1);
        y += super::super::dragon_particles::DEATH_LIFT_PER_TICK;
        let lifted = current(&store, 0, 0);
        assert_eq!(
            lifted[0],
            ([min[0], y, min[2]], [max[0], y + height, max[2]])
        );
        assert!(!Arc::ptr_eq(&previous, &lifted));
        assert_eq!(previous[0], previous_bounds);
        assert_eq!(initial[0], (min, max));
    }
    let settled = current(&store, 0, 0);
    let visits = store
        .effect_members
        .collidable
        .snapshot_visits
        .load(Ordering::Relaxed);
    let (_, allocations) = test_alloc::measure(|| {
        for _ in 0..120 {
            assert!(Arc::ptr_eq(
                &settled,
                &store.movement_collider_snapshot(0, 0)
            ));
        }
    });
    assert_eq!(allocations.calls, 0);
    assert_eq!(
        store
            .effect_members
            .collidable
            .snapshot_visits
            .load(Ordering::Relaxed),
        visits
    );
    for (mover, rider) in [(7, 0), (0, 99)] {
        link(&mut store, 99, 17, protocol::ActorLinkType::Rider);
        let excluded = current(&store, mover, rider);
        assert!(excluded.is_empty());
        store.advance_interpolation_ticks(1);
        assert!(Arc::ptr_eq(&excluded, &current(&store, mover, rider)));
        current(&store, 0, 0);
    }
}

#[test]
fn grouped_dragon_ticks_keep_colliders_current_through_interpolation_and_death_lifts() {
    let mut grouped = dying_solid_dragon();
    let mut stepped = dying_solid_dragon();
    for store in [&mut grouped, &mut stepped] {
        move_to(store, 7, 12.0, false);
    }
    for ticks in [2, 5, 7] {
        let old = current(&grouped, 0, 0);
        let old_bounds = old[0];
        grouped.advance_interpolation_frame(ticks);
        for _ in 0..ticks {
            stepped.advance_interpolation_ticks(1);
            current(&stepped, 0, 0);
        }
        assert_eq!(current(&grouped, 0, 0), current(&stepped, 0, 0));
        assert_eq!(
            grouped.get(7).unwrap().position,
            stepped.get(7).unwrap().position
        );
        assert_eq!(old[0], old_bounds);
    }
    assert_eq!(grouped.get(7).unwrap().interpolation_ticks_remaining, 0);
    let previous = current(&grouped, 0, 0);
    grouped.advance_interpolation_frame(1);
    assert_ne!(previous, current(&grouped, 0, 0));
}

#[test]
fn unchanged_queries_reuse_storage_without_allocating_or_visiting_members() {
    let mut store = ActorStore::new(1, 0);
    for runtime in 1..=128 {
        spawn(&mut store, runtime, runtime as i64, runtime % 4 == 0);
    }
    let before = current(&store, 0, 0);
    let visits = store
        .effect_members
        .collidable
        .snapshot_visits
        .load(Ordering::Relaxed);
    let (_, allocations) = test_alloc::measure(|| {
        for _ in 0..120 {
            assert!(Arc::ptr_eq(
                &before,
                &store.movement_collider_snapshot(0, 0)
            ));
        }
    });
    assert_eq!(allocations.calls, 0);
    assert_eq!(
        store
            .effect_members
            .collidable
            .snapshot_visits
            .load(Ordering::Relaxed),
        visits
    );
    store.advance_interpolation_frame(0);
    assert!(Arc::ptr_eq(&before, &current(&store, 0, 0)));
    move_to(&mut store, 1, 30.0, true);
    metadata(
        &mut store,
        1,
        ActorDataKey::Scale,
        ActorMetadataValue::Float(2.0),
    );
    store.advance_interpolation_ticks(3);
    assert!(Arc::ptr_eq(&before, &current(&store, 0, 0)));
    metadata(
        &mut store,
        4,
        ActorDataKey::Name,
        ActorMetadataValue::String("renamed".into()),
    );
    assert!(Arc::ptr_eq(&before, &current(&store, 0, 0)));
}

#[test]
fn empty_and_single_member_snapshots_release_superseded_storage() {
    let mut store = ActorStore::new(1, 0);
    let empty = current(&store, 0, 0);
    assert!(Arc::ptr_eq(&empty, &current(&store, 0, 0)));
    let old_empty = Arc::downgrade(&empty);
    drop(empty);
    spawn(&mut store, 1, 10, true);
    let single = current(&store, 0, 0);
    assert!(old_empty.upgrade().is_none());
    assert!(Arc::ptr_eq(&single, &current(&store, 0, 0)));
    let old_single = Arc::downgrade(&single);
    drop(single);
    assert!(current(&store, 1, 10).is_empty());
    assert!(old_single.upgrade().is_none());
    assert_eq!(current(&store, 0, 0).len(), 1);
}

#[test]
fn collision_metadata_membership_and_reused_ids_refresh_before_the_next_query() {
    let mut store = ActorStore::new(1, 0);
    spawn(&mut store, 2, 20, true);
    let original = current(&store, 0, 0);
    let original_value = original.to_vec();
    for (key, value) in [
        (ActorDataKey::Width, ActorMetadataValue::Float(3.0)),
        (ActorDataKey::Height, ActorMetadataValue::Float(5.0)),
        (ActorDataKey::Scale, ActorMetadataValue::Float(2.0)),
        (ActorDataKey::Width, ActorMetadataValue::Float(f32::NAN)),
        (ActorDataKey::Width, ActorMetadataValue::Float(1.0)),
        (ActorDataKey::FlagsTwo, ActorMetadataValue::FlagsExtended(0)),
        (ActorDataKey::FlagsTwo, solid_flag()),
    ] {
        let previous = current(&store, 0, 0);
        metadata(&mut store, 2, key, value.clone());
        let changed = current(&store, 0, 0);
        assert!(!Arc::ptr_eq(&previous, &changed));
        metadata(&mut store, 2, key, value);
        assert!(Arc::ptr_eq(&changed, &current(&store, 0, 0)));
    }
    spawn(&mut store, 2, 21, false);
    assert!(current(&store, 0, 0).is_empty());
    spawn(&mut store, 3, 21, true);
    assert_eq!(current(&store, 0, 0).len(), 1);
    apply(
        &mut store,
        ActorEvent::Remove(protocol::ActorRemoveEvent {
            dimension: 0,
            unique_id: 21,
        }),
    );
    assert!(current(&store, 0, 0).is_empty());
    spawn(&mut store, 2, 20, true);
    assert_eq!(&*current(&store, 0, 0), original_value.as_slice());
    assert_eq!(&*original, original_value.as_slice());
}

#[test]
fn interpolation_grouping_prediction_and_teleports_preserve_old_readers() {
    let mut grouped = ActorStore::new(1, 0);
    let mut stepped = ActorStore::new(1, 0);
    for store in [&mut grouped, &mut stepped] {
        spawn(store, 1, 10, true);
        move_to(store, 1, 7.0, false);
    }
    let original = current(&grouped, 0, 0);
    let original_value = original.to_vec();
    grouped.predict_remote_motion(3);
    assert!(Arc::ptr_eq(&original, &current(&grouped, 0, 0)));
    grouped.advance_interpolation_frame(3);
    for _ in 0..3 {
        let previous = current(&stepped, 0, 0);
        stepped.advance_interpolation_ticks(1);
        assert!(!Arc::ptr_eq(&previous, &current(&stepped, 0, 0)));
    }
    assert_eq!(current(&grouped, 0, 0), current(&stepped, 0, 0));
    let settled = current(&grouped, 0, 0);
    grouped.advance_interpolation_ticks(4);
    assert!(Arc::ptr_eq(&settled, &current(&grouped, 0, 0)));
    move_to(&mut grouped, 1, 20.0, true);
    assert_ne!(settled, current(&grouped, 0, 0));
    assert_eq!(&*original, original_value.as_slice());
}

#[test]
fn interpolation_preserves_changed_coordinate_bits() {
    let mut store = ActorStore::new(1, 0);
    spawn(&mut store, 1, 10, true);
    store.set_local_presentation_position(1, [0.0; 3], false);
    let original = current(&store, 0, 0);
    let ActorEvent::Move(mut movement) = network_move(1, -0.0, false) else {
        unreachable!()
    };
    movement.position_origin = protocol::ActorPositionOrigin::Feet;
    movement.on_ground = None;
    apply(&mut store, ActorEvent::Move(movement));
    store.advance_interpolation_ticks(3);
    let changed = current(&store, 0, 0);
    assert_eq!(changed[0].0[1].to_bits(), (-0.0_f32).to_bits());
    assert_eq!(original[0].0[1].to_bits(), 0.0_f32.to_bits());
    assert!(!Arc::ptr_eq(&original, &changed));
}

#[test]
fn mover_mount_seating_and_pending_mount_links_never_reuse_stale_filters() {
    let mut store = ActorStore::new(1, 0);
    spawn(&mut store, 1, 10, true);
    spawn(&mut store, 2, 20, true);
    spawn(&mut store, 3, 30, true);
    let all = current(&store, 0, 0);
    let unmounted = current(&store, 1, 10);
    link(&mut store, 10, 20, protocol::ActorLinkType::Rider);
    let riding = current(&store, 1, 10);
    assert_eq!(riding.len(), 1);
    assert_ne!(unmounted, riding);
    move_to(&mut store, 2, 40.0, true);
    move_to(&mut store, 1, 45.0, true);
    assert!(Arc::ptr_eq(&riding, &current(&store, 1, 10)));
    link(&mut store, 10, 20, protocol::ActorLinkType::Remove);
    assert_eq!(current(&store, 1, 10).len(), 2);
    assert_eq!(current(&store, 2, 20).len(), 2);
    metadata(
        &mut store,
        3,
        ActorDataKey::SeatOffset,
        ActorMetadataValue::Vector([0.0, 1.0, 0.0]),
    );
    link(&mut store, 30, 20, protocol::ActorLinkType::Passenger);
    store.seat_riders();
    current(&store, 0, 0);
    link(&mut store, 10, 90, protocol::ActorLinkType::Rider);
    assert_eq!(current(&store, 1, 10).len(), 2);
    spawn(&mut store, 9, 90, true);
    assert_eq!(current(&store, 1, 10).len(), 2);
    apply(
        &mut store,
        ActorEvent::Remove(protocol::ActorRemoveEvent {
            dimension: 0,
            unique_id: 90,
        }),
    );
    spawn(&mut store, 9, 91, true);
    assert_eq!(current(&store, 1, 10).len(), 3);
    assert_eq!(all.len(), 3);
}

#[test]
fn late_constructor_bounds_presentation_reanchoring_and_resets_refresh_snapshots() {
    let mut store = ActorStore::new(1, 0);
    let base = super::super::EntityFacts::pinned()
        .records
        .iter()
        .find(|record| {
            record.fact.collidable
                && record.fact.collision.is_some_and(|size| {
                    size != [
                        super::super::DEFAULT_ACTOR_COLLISION_WIDTH,
                        super::super::DEFAULT_ACTOR_COLLISION_HEIGHT,
                    ]
                })
        })
        .unwrap();
    apply(
        &mut store,
        entity_spawn(
            1,
            10,
            "fixture:late_solid",
            [ActorMetadata {
                key: ActorDataKey::FlagsTwo.wire_value(),
                value: solid_flag(),
            }]
            .into(),
        ),
    );
    let generic = current(&store, 0, 0);
    apply(
        &mut store,
        ActorEvent::Identifiers(protocol::ActorIdentifierRegistry {
            entries: [protocol::ActorIdentifier {
                identifier: "fixture:late_solid".into(),
                base_identifier: base.identifier.as_ref().into(),
            }]
            .into(),
            skipped: 0,
        }),
    );
    assert_ne!(generic, current(&store, 0, 0));
    store.set_local_presentation_position(1, [9.0, 10.0, 11.0], true);
    let positioned = current(&store, 0, 0);
    assert_ne!(generic, positioned);
    assert_eq!(
        store.reset_dimension(1, store.latest_sequence + 1, 1),
        ActorApplyResult::Reset
    );
    assert!(current(&store, 0, 0).is_empty());
    spawn(&mut store, 1, 10, true);
    assert_ne!(positioned, current(&store, 0, 0));
    store.begin_session(2, 0);
    assert!(current(&store, 0, 0).is_empty());
    spawn(&mut store, 1, 10, true);
    assert_eq!(current(&store, 0, 0).len(), 1);
    assert_eq!(positioned.len(), 1);
}

#[test]
fn falling_block_offset_and_dedicated_painting_bounds_follow_the_live_actor() {
    let mut store = ActorStore::new(1, 0);
    apply(
        &mut store,
        entity_spawn(
            1,
            10,
            "minecraft:falling_block",
            [ActorMetadata {
                key: ActorDataKey::FlagsTwo.wire_value(),
                value: solid_flag(),
            }]
            .into(),
        ),
    );
    let original = current(&store, 0, 0);
    metadata(
        &mut store,
        1,
        ActorDataKey::Height,
        ActorMetadataValue::Float(4.0),
    );
    assert_ne!(original, current(&store, 0, 0));
    for direction in 0..4 {
        let protocol::WorldEvent::Actor(ActorEvent::PaintingSpawn {
            mut spawn,
            painting,
        }) = protocol::test_support::packets::add_painting_event(
            -73,
            91,
            "backyard",
            direction,
            [10.5, 70.5, 20.5],
        )
        .unwrap()
        .unwrap()
        else {
            panic!("painting spawn")
        };
        spawn.metadata = [ActorMetadata {
            key: ActorDataKey::FlagsTwo.wire_value(),
            value: solid_flag(),
        }]
        .into();
        apply(&mut store, ActorEvent::PaintingSpawn { spawn, painting });
        let boxes = current(&store, 0, 0);
        assert_eq!(boxes.len(), 2);
        assert_eq!(boxes[1], store.get(91).unwrap().bounding_box().unwrap());
    }
}
