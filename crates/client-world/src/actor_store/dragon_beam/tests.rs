use protocol::{ActorEvent, ActorKind, ActorRemoveEvent, ActorStatusEvent, ActorStatusKind};

use super::*;

fn spawn(runtime: u64, identifier: &str, position: [f32; 3]) -> ActorEvent {
    let ActorEvent::Spawn(mut event) = super::super::tests::spawn(runtime, runtime as i64) else {
        unreachable!();
    };
    event.kind = ActorKind::Entity {
        identifier: identifier.into(),
    };
    event.position = position;
    ActorEvent::Spawn(event)
}

fn scene() -> ActorStore {
    let mut store = ActorStore::new(1, 0);
    store.apply(1, 1, spawn(1, "minecraft:ender_dragon", [0.0, 64.0, 0.0]));
    store.apply(1, 2, spawn(2, "minecraft:ender_crystal", [12.0, 64.0, 0.0]));
    store.apply(1, 3, spawn(3, "minecraft:ender_crystal", [20.0, 64.0, 0.0]));
    store
}

#[test]
fn healing_beam_without_target_metadata_keeps_nearest_until_refresh() {
    let mut store = scene();
    store.advance_dragon_beams_with(|_| false);
    assert!(store.crystal_beams(0.5).is_empty());
    store.advance_dragon_beams_with(|_| true);
    let beam = store.crystal_beams(0.5)[0];
    assert_eq!(beam.runtime_id, 1);
    assert_eq!(beam.owner_position, [0.0, 64.0, 0.0]);
    assert_eq!(beam.target, [0.0, 66.0, 0.0]);
    assert_eq!(beam.crystal, [12.0, 65.0, 0.0]);
    assert_eq!(beam.age_ticks, 0.5);
    store.apply(1, 4, spawn(4, "minecraft:ender_crystal", [4.0, 64.0, 0.0]));
    store.advance_dragon_beams_with(|_| false);
    assert_eq!(store.crystal_beams(0.0)[0].crystal, [12.0, 65.0, 0.0]);
    store.advance_dragon_beams_with(|_| true);
    assert_eq!(store.crystal_beams(0.0)[0].crystal, [4.0, 65.0, 0.0]);
}

#[test]
fn range_boundary_invalid_positions_and_other_actor_types_do_not_select_a_crystal() {
    for (identifier, position) in [
        ("minecraft:ender_crystal", [32.0, 64.0, 0.0]),
        ("minecraft:ender_crystal", [f32::NAN, 64.0, 0.0]),
        ("minecraft:bee", [1.0, 64.0, 0.0]),
    ] {
        let mut store = ActorStore::new(1, 0);
        store.apply(1, 1, spawn(1, "minecraft:ender_dragon", [0.0, 64.0, 0.0]));
        store.apply(1, 2, spawn(2, identifier, position));
        store.advance_dragon_beams_with(|_| true);
        assert!(store.crystal_beams(0.0).is_empty());
    }
}

#[test]
fn dead_removed_reused_crystals_and_dimension_reset_never_keep_a_stale_beam() {
    let mut store = scene();
    store.advance_dragon_beams_with(|_| true);
    assert_eq!(store.crystal_beams(0.0).len(), 1);
    store.apply(
        1,
        4,
        ActorEvent::Status(ActorStatusEvent {
            runtime_id: 2,
            kind: ActorStatusKind::Death,
            data: 0,
        }),
    );
    assert!(store.crystal_beams(0.0).is_empty());
    store.advance_dragon_beams_with(|_| true);
    assert_eq!(store.crystal_beams(0.0)[0].crystal, [20.0, 65.0, 0.0]);
    store.apply(
        1,
        5,
        ActorEvent::Remove(ActorRemoveEvent {
            dimension: 0,
            unique_id: 3,
        }),
    );
    assert!(store.crystal_beams(0.0).is_empty());
    store.apply(1, 6, spawn(3, "minecraft:ender_crystal", [2.0, 64.0, 0.0]));
    assert!(store.crystal_beams(0.0).is_empty());
    store.advance_dragon_beams_with(|_| false);
    assert!(store.crystal_beams(0.0).is_empty());
    store.advance_dragon_beams_with(|_| true);
    assert_eq!(store.crystal_beams(0.0).len(), 1);
    store.reset_dimension(1, 7, 2);
    assert!(store.crystal_beams(0.0).is_empty());
}
