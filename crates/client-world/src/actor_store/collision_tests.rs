use super::*;
use protocol::{ActorEvent, ActorMetadata, ActorMetadataUpdateEvent};

/// Spawns a constructor without size metadata to exercise its compiled defaults.
fn spawn(store: &mut ActorStore, id: u64, name: &str) {
    let ActorEvent::Spawn(mut event) =
        super::super::tests::entity_spawn(id, id as i64, name, [].into())
    else {
        unreachable!();
    };
    event.position = [0.0, 4.0, 0.0];
    store.apply(1, id, ActorEvent::Spawn(event));
}

#[test]
fn collidable_definitions_supply_bounds_and_exclude_self_and_mount() {
    let mut store = ActorStore::new(1, 0);
    let records: Vec<_> = EntityFacts::pinned()
        .records
        .iter()
        .filter(|r| r.fact.collidable)
        .collect();
    assert!(!records.is_empty());
    for (i, record) in records.iter().enumerate() {
        spawn(&mut store, i as u64 + 1, &record.identifier);
    }
    let boxes = store.movement_collider_snapshot(0, 0);
    assert_eq!(boxes.len(), records.len());
    for (bounds, record) in boxes.iter().zip(&records) {
        let [width, height] = record.fact.collision.unwrap_or([
            DEFAULT_ACTOR_COLLISION_WIDTH,
            DEFAULT_ACTOR_COLLISION_HEIGHT,
        ]);
        assert!((bounds.1[0] - bounds.0[0] - width).abs() < 1e-6);
        assert!((bounds.1[1] - bounds.0[1] - height).abs() < 1e-6);
    }
    store.rider_to_ridden.insert(
        100,
        super::riding_control::RiderLink {
            vehicle: 2,
            order: 0,
            pending: false,
        },
    );
    assert_eq!(
        store.movement_collider_snapshot(1, 100).len(),
        records.len() - 2
    );
    store.rider_to_ridden.clear();
    assert_eq!(
        store.movement_collider_snapshot(1, 100).len(),
        records.len() - 1
    );
}

#[test]
fn collidable_actors_without_definition_sizes_use_the_server_box() {
    let mut store = ActorStore::new(1, 0);
    let record = EntityFacts::pinned()
        .records
        .iter()
        .find(|record| record.fact.collidable && record.fact.collision.is_none())
        .unwrap();
    let size = 1.0;
    store.apply(
        1,
        1,
        super::super::tests::entity_spawn(
            1,
            1,
            &record.identifier,
            [
                ActorMetadata {
                    key: ActorDataKey::Width.wire_value(),
                    value: ActorMetadataValue::Float(size),
                },
                ActorMetadata {
                    key: ActorDataKey::Height.wire_value(),
                    value: ActorMetadataValue::Float(size),
                },
            ]
            .into(),
        ),
    );
    let (min, max) = store
        .movement_collider_snapshot(0, 0)
        .first()
        .copied()
        .unwrap();
    assert_eq!(max[0] - min[0], size);
    assert_eq!(max[1] - min[1], size);
}

#[test]
fn synced_collision_flags_refresh_membership_and_removal_clears_it() {
    let mut store = ActorStore::new(1, 0);
    let record = EntityFacts::pinned()
        .records
        .iter()
        .find(|r| r.fact.collidable)
        .unwrap();
    spawn(&mut store, 1, &record.identifier);
    assert_eq!(store.movement_collider_snapshot(0, 0).len(), 1);
    for (sequence, flags, expected) in [
        (2, 0, 0),
        (3, 1_u64 << (ActorFlag::Collidable.wire_value() - 64), 1),
    ] {
        store.apply(
            1,
            sequence,
            ActorEvent::Metadata(ActorMetadataUpdateEvent {
                dimension: 0,
                runtime_id: 1,
                metadata: [ActorMetadata {
                    key: ActorDataKey::FlagsTwo.wire_value(),
                    value: ActorMetadataValue::FlagsExtended(flags),
                }]
                .into(),
                properties: [].into(),
                tick: sequence,
            }),
        );
        assert_eq!(store.movement_collider_snapshot(0, 0).len(), expected);
    }
    store.apply(
        1,
        4,
        ActorEvent::Remove(protocol::ActorRemoveEvent {
            dimension: 0,
            unique_id: 1,
        }),
    );
    assert_eq!(store.movement_collider_snapshot(0, 0).len(), 0);
}

#[test]
fn moving_actors_use_current_collision_feet_and_synced_scale() {
    let mut store = ActorStore::new(1, 0);
    let record = EntityFacts::pinned()
        .records
        .iter()
        .find(|r| r.fact.collidable)
        .unwrap();
    spawn(&mut store, 1, &record.identifier);
    let before = store
        .movement_collider_snapshot(0, 0)
        .first()
        .copied()
        .unwrap();
    store.apply(
        1,
        2,
        ActorEvent::Metadata(ActorMetadataUpdateEvent {
            dimension: 0,
            runtime_id: 1,
            metadata: [ActorMetadata {
                key: ActorDataKey::Scale.wire_value(),
                value: ActorMetadataValue::Float(2.0),
            }]
            .into(),
            properties: [].into(),
            tick: 0,
        }),
    );
    let mut movement = match super::super::tests::network_move(1, 4.0, false) {
        ActorEvent::Move(movement) => movement,
        _ => unreachable!(),
    };
    movement.position_origin = protocol::ActorPositionOrigin::Feet;
    movement.position[0] = Some(2.0);
    let half_ticks = ACTOR_INTERPOLATION_TICKS;
    movement.interpolation.ticks = u64::from(half_ticks) * 2;
    store.apply(1, 3, ActorEvent::Move(movement));
    store.advance_interpolation_ticks(half_ticks);
    let midway = store
        .movement_collider_snapshot(0, 0)
        .first()
        .copied()
        .unwrap();
    assert!(((midway.0[0] + midway.1[0]) * 0.5 - 1.0).abs() < 1e-6);
    store.advance_interpolation_ticks(half_ticks);
    let after = store
        .movement_collider_snapshot(0, 0)
        .first()
        .copied()
        .unwrap();
    assert!(
        ((after.0[0] + after.1[0]) * 0.5 - (before.0[0] + before.1[0]) * 0.5 - 2.0).abs() < 1e-6
    );
    assert!((after.1[1] - after.0[1] - 2.0 * (before.1[1] - before.0[1])).abs() < 1e-6);
}
