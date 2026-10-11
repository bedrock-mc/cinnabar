use super::*;

mod fixture;
mod pinned;

const FISH: [&str; 4] = [
    "minecraft:cod",
    "minecraft:salmon",
    "minecraft:pufferfish",
    "minecraft:tropicalfish",
];

fn actor(identifier: &str) -> ActorSnapshot {
    let mut actor = crate::actor_animation::tests::actor_with_metadata(HashMap::new());
    actor.kind = ActorKind::Entity {
        identifier: identifier.into(),
    };
    actor
}

fn store(actor: &ActorSnapshot) -> ActorAnimationStore {
    let ActorKind::Entity { identifier } = &actor.kind else {
        unreachable!()
    };
    let mut store = ActorAnimationStore::with_assets(fixture::assets(identifier));
    store.insert(1, 0, actor);
    store
}

fn tick(store: &mut ActorAnimationStore, actor: &ActorSnapshot, evaluate: bool) {
    store.advance_tick(
        &HashMap::from([(actor.runtime_id, actor.clone())]),
        None,
        None,
        evaluate,
        true,
        |_| ActorTickContext::default(),
    );
}

fn phase(store: &ActorAnimationStore, actor: &ActorSnapshot) -> [f32; 2] {
    store.rigs[&store.runtime_to_lifetime[&actor.runtime_id]]
        .motion
        .fish_phase()
}

fn variables(store: &ActorAnimationStore, actor: &ActorSnapshot) -> [Option<f32>; 2] {
    let state = &store.rigs[&store.runtime_to_lifetime[&actor.runtime_id]];
    [
        store.layout.engine.fish_animation_amount,
        store.layout.engine.fish_animation_amount_previous,
    ]
    .map(|slot| state.variables.number_at(slot.unwrap()))
}

#[test]
fn stationary_fish_phase_advances_once_per_tick_for_each_native_family() {
    for identifier in FISH {
        let actor = actor(identifier);
        let mut store = store(&actor);
        assert_eq!(phase(&store, &actor), [0.0, 0.0]);
        for amount in 1..=4 {
            tick(&mut store, &actor, true);
            let expected = [amount as f32, (amount - 1) as f32];
            assert_eq!(phase(&store, &actor), expected, "{identifier}");
            assert_eq!(variables(&store, &actor), expected.map(Some));
        }
    }
}

#[test]
fn moving_fish_uses_three_dimensional_native_velocity_and_preserves_previous_phase() {
    let mut actor = actor("minecraft:cod");
    let mut store = store(&actor);
    actor.status.native_velocity = [0.3, 0.4, 0.0];
    tick(&mut store, &actor, true);
    assert_eq!(phase(&store, &actor), [1.05, 0.0]);
    actor.status.native_velocity = [0.0, 0.0, -0.2];
    tick(&mut store, &actor, true);
    assert_eq!(phase(&store, &actor), [(1.05 + 1.0) + 0.02, 1.05]);
    assert_eq!(variables(&store, &actor), phase(&store, &actor).map(Some));
}

#[test]
fn fish_phase_uses_retained_native_motion_instead_of_displacement_derived_query_velocity() {
    let mut actor = actor("minecraft:salmon");
    actor.velocity = [30.0, 40.0, 0.0];
    let mut store = store(&actor);
    tick(&mut store, &actor, true);
    assert_eq!(phase(&store, &actor), [1.0, 0.0]);
    actor.status.native_velocity = [0.3, 0.4, 0.0];
    tick(&mut store, &actor, true);
    assert_eq!(phase(&store, &actor), [2.05, 1.0]);
}

#[test]
fn fish_phase_keeps_ticking_when_pose_evaluation_is_skipped_and_holds_between_ticks() {
    let actor = actor("minecraft:pufferfish");
    let mut store = store(&actor);
    tick(&mut store, &actor, false);
    tick(&mut store, &actor, false);
    assert_eq!(phase(&store, &actor), [2.0, 1.0]);
    tick(&mut store, &actor, true);
    assert_eq!(variables(&store, &actor), [Some(3.0), Some(2.0)]);
    let held = phase(&store, &actor);
    for _ in 0..8 {
        assert!(store.get(actor.runtime_id).is_some());
        assert_eq!(phase(&store, &actor), held, "render reads consume no tick");
    }
}

#[test]
fn fish_phase_survives_controller_and_geometry_resets_but_respawn_starts_at_zero() {
    let mut actor = actor("minecraft:tropicalfish");
    let mut store = store(&actor);
    tick(&mut store, &actor, true);
    tick(&mut store, &actor, true);
    store.mark_reset(actor.runtime_id);
    tick(&mut store, &actor, true);
    assert_eq!(phase(&store, &actor), [3.0, 2.0]);
    let first_geometry = store.get(actor.runtime_id).unwrap().rig;
    let variant = Q::Variant.integer_metadata_key().unwrap();
    actor.metadata.insert(variant, ActorMetadataValue::Int(1));
    tick(&mut store, &actor, true);
    assert_ne!(store.get(actor.runtime_id).unwrap().rig, first_geometry);
    assert_eq!(phase(&store, &actor), [4.0, 3.0]);
    assert_eq!(variables(&store, &actor), [Some(4.0), Some(3.0)]);
    actor.spawn_revision += 1;
    store.insert(1, 0, &actor);
    assert_eq!(phase(&store, &actor), [0.0, 0.0]);
    tick(&mut store, &actor, true);
    assert_eq!(variables(&store, &actor), [Some(1.0), Some(0.0)]);
}

#[test]
fn fish_phase_is_not_assigned_to_other_aquatic_or_custom_actors() {
    for identifier in ["minecraft:squid", "minecraft:dolphin", "custom:cod", "cod"] {
        let actor = actor(identifier);
        let mut store = store(&actor);
        for _ in 0..3 {
            tick(&mut store, &actor, true);
        }
        assert_eq!(phase(&store, &actor), [0.0, 0.0], "{identifier}");
        assert_eq!(variables(&store, &actor), [None, None], "{identifier}");
    }
}
