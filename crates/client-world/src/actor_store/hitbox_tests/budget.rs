use super::*;
use crate::actor_store::hitbox::{Budget, MAX_HITBOXES_PER_ACTOR};

/// Creates an empty store whose geometry limits can be exhausted by small fixtures.
fn limited_store(per_actor: usize, retained: usize) -> ActorStore {
    let mut store = ActorStore::new(1, 0);
    store.hitbox_budget = Budget::with_limits(per_actor, retained);
    store
}

/// Installs one actor with ordered geometry in the store's current dimension.
fn spawn_boxes(
    store: &mut ActorStore,
    sequence: u64,
    runtime_id: u64,
    unique_id: i64,
    entries: &[Entry],
) -> ActorApplyResult {
    let ActorEvent::Spawn(mut event) = spawn(runtime_id, unique_id) else {
        unreachable!();
    };
    event.dimension = store.dimension;
    event.metadata = Arc::from([hitbox_compound(entries)]);
    store.apply(1, sequence, ActorEvent::Spawn(event))
}

/// Applies geometry to an existing actor using an ordinary metadata packet.
fn update_boxes(
    store: &mut ActorStore,
    sequence: u64,
    runtime_id: u64,
    entries: &[Entry],
) -> ActorApplyResult {
    store.apply(
        1,
        sequence,
        ActorEvent::Metadata(ActorMetadataUpdateEvent {
            dimension: store.dimension,
            runtime_id,
            metadata: Arc::from([hitbox_compound(entries)]),
            properties: Arc::from([]),
            tick: 0,
        }),
    )
}

/// Removes one lifetime through the same unique-ID path as the network event.
fn remove(store: &mut ActorStore, sequence: u64, unique_id: i64) {
    assert_eq!(
        store.apply(
            1,
            sequence,
            ActorEvent::Remove(protocol::ActorRemoveEvent {
                dimension: store.dimension,
                unique_id,
            }),
        ),
        ActorApplyResult::Removed,
    );
}

#[test]
fn repeated_small_updates_stop_at_the_production_actor_limit() {
    let mut store = store_with(0.0, vec![hitbox_compound(&[RAISED])]);
    for count in 2..=MAX_HITBOXES_PER_ACTOR {
        assert_eq!(
            update(&mut store, count as u64, hitbox_compound(&[RAISED])),
            ActorApplyResult::Updated,
        );
    }
    let retained = boxes(&store);
    for sequence in MAX_HITBOXES_PER_ACTOR + 1..=MAX_HITBOXES_PER_ACTOR + 3 {
        assert_eq!(
            update(&mut store, sequence as u64, hitbox_compound(&[RAISED])),
            ActorApplyResult::CapacityRejected,
        );
        assert_eq!(boxes(&store), retained);
    }
    assert_eq!(retained.len(), MAX_HITBOXES_PER_ACTOR);
    assert_eq!(store.hitbox_budget.retained(), MAX_HITBOXES_PER_ACTOR);
    assert_eq!(store.hitbox_budget.refused(), 3);
}

#[test]
fn admission_keeps_the_readable_prefix_and_applies_other_metadata() {
    let mut store = limited_store(3, 10);
    spawn_boxes(&mut store, 1, 8, 80, &[RAISED]);
    let shifted = (RAISED.0, RAISED.1, [8.0, 4.0, 1.0]);
    let invalid = ([0.0; 3], [f32::INFINITY; 3], [0.0; 3]);
    assert_eq!(
        update_boxes(&mut store, 2, 8, &[invalid, RAISED, shifted, RAISED]),
        ActorApplyResult::CapacityRejected,
    );
    let actor = store.get(8).unwrap();
    let expected = actor.hit_boxes().collect::<Vec<_>>();
    assert_eq!(expected.len(), 3);
    assert_ne!(expected[1], expected[2]);
    assert_eq!(expected[0], expected[1]);
    assert_eq!(store.hitbox_budget.retained(), 3);
    assert_eq!(
        store.hitbox_budget.refused(),
        1,
        "malformed is counted separately"
    );

    assert_eq!(
        store.apply(
            1,
            3,
            ActorEvent::Metadata(ActorMetadataUpdateEvent {
                dimension: 0,
                runtime_id: 8,
                metadata: Arc::from([
                    hitbox_compound(&[RAISED]),
                    ActorMetadata {
                        key: ActorDataKey::Name.wire_value(),
                        value: ActorMetadataValue::String("Still updated".into()),
                    },
                ]),
                properties: Arc::from([]),
                tick: 0,
            }),
        ),
        ActorApplyResult::CapacityRejected,
    );
    assert_eq!(
        store.actor_display_name(80),
        Some(Arc::from("Still updated"))
    );
    assert_eq!(
        store.get(8).unwrap().hit_boxes().collect::<Vec<_>>(),
        expected
    );
}

#[test]
fn aggregate_admission_is_shared_and_removal_releases_geometry() {
    let mut store = limited_store(4, 6);
    spawn_boxes(&mut store, 1, 8, 80, &[RAISED; 4]);
    spawn_boxes(&mut store, 2, 9, 90, &[RAISED; 4]);
    assert_eq!(store.get(8).unwrap().hit_boxes().count(), 4);
    assert_eq!(store.get(9).unwrap().hit_boxes().count(), 2);
    assert_eq!(store.hitbox_budget.retained(), 6);
    assert_eq!(store.hitbox_budget.refused(), 2);
    assert_eq!(
        update_boxes(&mut store, 3, 9, &[RAISED]),
        ActorApplyResult::CapacityRejected,
    );
    remove(&mut store, 4, 80);
    assert_eq!(store.hitbox_budget.retained(), 2);
    assert_eq!(
        update_boxes(&mut store, 5, 9, &[RAISED; 2]),
        ActorApplyResult::Updated,
    );
    assert_eq!(store.get(9).unwrap().hit_boxes().count(), 4);
    assert_eq!(store.hitbox_budget.retained(), 4);
}

#[test]
fn replacing_runtime_and_unique_ids_releases_both_old_allocations() {
    let mut store = limited_store(4, 4);
    spawn_boxes(&mut store, 1, 8, 80, &[RAISED; 2]);
    spawn_boxes(&mut store, 2, 9, 90, &[RAISED; 2]);
    assert_eq!(
        spawn_boxes(&mut store, 3, 8, 90, &[RAISED; 4]),
        ActorApplyResult::Replaced,
    );
    assert!(store.get(9).is_none());
    assert_eq!(store.get(8).unwrap().unique_id, 90);
    assert_eq!(store.get(8).unwrap().hit_boxes().count(), 4);
    assert_eq!(store.hitbox_budget.retained(), 4);
    assert_eq!(store.hitbox_budget.refused(), 0);
}

#[test]
fn dimension_and_session_reset_reuse_released_capacity() {
    let mut store = limited_store(4, 4);
    spawn_boxes(&mut store, 1, 8, 80, &[RAISED; 4]);
    assert_eq!(store.reset_dimension(1, 2, 1), ActorApplyResult::Reset);
    assert_eq!(store.hitbox_budget.retained(), 0);
    spawn_boxes(&mut store, 3, 9, 90, &[RAISED; 4]);
    assert_eq!(store.get(9).unwrap().hit_boxes().count(), 4);
    store.begin_session(1, 0);
    assert_eq!(store.hitbox_budget.retained(), 0);
    spawn_boxes(&mut store, 1, 10, 100, &[RAISED; 4]);
    assert_eq!(store.get(10).unwrap().hit_boxes().count(), 4);
    assert_eq!(store.hitbox_budget.retained(), 4);
}

#[test]
fn replacement_cannot_spend_geometry_still_owned_by_an_old_snapshot() {
    let mut store = limited_store(4, 4);
    spawn_boxes(&mut store, 1, 8, 80, &[RAISED; 4]);
    let snapshot = store.get(8).unwrap().clone();
    assert_eq!(
        spawn_boxes(&mut store, 2, 8, 90, &[RAISED; 2]),
        ActorApplyResult::Replaced,
    );
    let actor = store.get(8).unwrap();
    assert_eq!(actor.unique_id, 90);
    assert_eq!(
        actor.hit_boxes().collect::<Vec<_>>(),
        [actor.bounding_box().unwrap()]
    );
    assert_eq!(snapshot.hit_boxes().count(), 4);
    assert_eq!(store.hitbox_budget.retained(), 4);
    drop(snapshot);
    assert_eq!(
        update_boxes(&mut store, 3, 8, &[RAISED; 4]),
        ActorApplyResult::Updated,
    );
    assert_eq!(store.get(8).unwrap().hit_boxes().count(), 4);
}

#[test]
fn resets_do_not_forgive_geometry_retained_by_published_snapshots() {
    for reset_session in [false, true] {
        let mut store = limited_store(4, 4);
        spawn_boxes(&mut store, 1, 8, 80, &[RAISED; 4]);
        let snapshot = store.get(8).unwrap().clone();
        if reset_session {
            store.begin_session(1, 0);
        } else {
            assert_eq!(store.reset_dimension(1, 2, 1), ActorApplyResult::Reset);
        }
        assert!(store.is_empty());
        assert_eq!(store.hitbox_budget.retained(), 4);
        spawn_boxes(&mut store, 3, 9, 90, &[RAISED]);
        let actor = store.get(9).unwrap();
        assert_eq!(
            actor.hit_boxes().collect::<Vec<_>>(),
            [actor.bounding_box().unwrap()]
        );
        drop(snapshot);
        assert_eq!(store.hitbox_budget.retained(), 0);
        assert_eq!(
            update_boxes(&mut store, 4, 9, &[RAISED; 4]),
            ActorApplyResult::Updated,
        );
        assert_eq!(store.get(9).unwrap().hit_boxes().count(), 4);
    }
}

#[test]
fn published_snapshots_stay_charged_until_their_last_drop() {
    let mut store = limited_store(4, 4);
    spawn_boxes(&mut store, 1, 8, 80, &[RAISED; 4]);
    let snapshot = store.get(8).unwrap().clone();
    let retained = snapshot.clone();
    remove(&mut store, 2, 80);
    drop(snapshot);
    assert_eq!(store.hitbox_budget.retained(), 4);
    spawn_boxes(&mut store, 3, 9, 90, &[RAISED]);
    let actor = store.get(9).unwrap();
    assert_eq!(
        actor.hit_boxes().collect::<Vec<_>>(),
        [actor.bounding_box().unwrap()]
    );
    assert_eq!(retained.hit_boxes().count(), 4);
    drop(retained);
    assert_eq!(store.hitbox_budget.retained(), 0);
    assert_eq!(
        update_boxes(&mut store, 4, 9, &[RAISED; 4]),
        ActorApplyResult::Updated,
    );
    assert_eq!(store.get(9).unwrap().hit_boxes().count(), 4);
}

#[test]
fn snapshot_copy_requires_capacity_for_both_geometry_allocations() {
    let mut store = limited_store(8, 6);
    spawn_boxes(&mut store, 1, 8, 80, &[RAISED; 3]);
    let snapshot = store.get(8).unwrap().clone();
    assert_eq!(
        update_boxes(&mut store, 2, 8, &[RAISED]),
        ActorApplyResult::CapacityRejected,
    );
    assert_eq!(store.get(8).unwrap().hit_boxes().count(), 3);
    assert_eq!(snapshot.hit_boxes().count(), 3);
    assert_eq!(store.hitbox_budget.retained(), 3);
    drop(snapshot);
    assert_eq!(
        update_boxes(&mut store, 3, 8, &[RAISED]),
        ActorApplyResult::Updated,
    );
    assert_eq!(store.get(8).unwrap().hit_boxes().count(), 4);
    assert_eq!(store.hitbox_budget.retained(), 4);
}

#[test]
fn admitted_snapshot_copy_and_empty_updates_preserve_old_geometry() {
    let mut store = limited_store(8, 8);
    spawn_boxes(&mut store, 1, 8, 80, &[RAISED; 3]);
    let snapshot = store.get(8).unwrap().clone();
    assert_eq!(
        update_boxes(&mut store, 2, 8, &[RAISED]),
        ActorApplyResult::Updated,
    );
    assert_eq!(snapshot.hit_boxes().count(), 3);
    assert_eq!(store.get(8).unwrap().hit_boxes().count(), 4);
    assert_eq!(store.hitbox_budget.retained(), 7);
    let current = store.get(8).unwrap().clone();
    assert_eq!(
        update_boxes(&mut store, 3, 8, &[]),
        ActorApplyResult::Updated,
    );
    assert_eq!(
        store.hitbox_budget.retained(),
        7,
        "empty updates allocate no copy"
    );
    assert_eq!(
        update(&mut store, 4, compound(vec![10, 0, 0xff])),
        ActorApplyResult::Updated,
    );
    assert_eq!(
        store.hitbox_budget.retained(),
        7,
        "unreadable updates allocate no copy"
    );
    drop(current);
    drop(snapshot);
    assert_eq!(store.hitbox_budget.retained(), 4);
    assert_eq!(
        update_boxes(&mut store, 5, 8, &[RAISED; 4]),
        ActorApplyResult::Updated,
    );
    assert_eq!(store.get(8).unwrap().hit_boxes().count(), 8);
    assert_eq!(store.hitbox_budget.retained(), 8);
}
