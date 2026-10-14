use sim::{
    Aabb, CollisionRegistry, CollisionWorld, MovementInput, PaletteWorld, PlayerState, Simulator,
    Vec3,
};
use world::{ChunkStore, SubChunkKey};

/// Supplies loaded air around an actor, so every contact comes from its box.
fn terrain() -> (ChunkStore, CollisionRegistry) {
    let mut registry = CollisionRegistry::new();
    registry.register(0, []).unwrap();
    let mut store = ChunkStore::new();
    store
        .mark_sub_chunk_loaded(SubChunkKey::new(0, 0, 0, 0))
        .unwrap();
    (store, registry)
}

/// Derives every default solid actor box from the pinned constructor facts.
fn actors() -> Vec<Aabb> {
    assets::facts::entity::EntityFacts::pinned()
        .records
        .iter()
        .filter(|record| record.fact.collidable)
        .map(|record| {
            let [width, height] = record.fact.collision.unwrap().map(f64::from);
            Aabb::new(
                Vec3::new(8.0 - width / 2.0, 4.0, 8.0 - width / 2.0),
                Vec3::new(8.0 + width / 2.0, 4.0 + height, 8.0 + width / 2.0),
            )
        })
        .collect()
}

/// Chooses the lowest solid actor as the step-up fixture without repeating its dimensions.
fn lowest_actor() -> Aabb {
    actors()
        .into_iter()
        .min_by(|a, b| a.max.y.total_cmp(&b.max.y))
        .unwrap()
}

#[test]
fn actor_collisions_support_landings_and_standing_without_terrain() {
    let (store, registry) = terrain();
    for shape in actors() {
        let world =
            PaletteWorld::for_movement(&store, &registry, 0).with_actor_colliders(vec![shape]);
        let mut state = PlayerState::new(Vec3::new(8.0, shape.max.y + 0.1, 8.0));
        state.velocity.y = -0.2;
        let simulator = Simulator::default();
        simulator
            .tick(&mut state, MovementInput::default(), &world)
            .unwrap();
        assert!((state.position.y - shape.max.y).abs() < 1e-6);
        assert!(state.on_ground);
        for _ in 0..3 {
            simulator
                .tick(&mut state, MovementInput::default(), &world)
                .unwrap();
            assert!((state.position.y - shape.max.y).abs() < 1e-6);
            assert!(state.on_ground);
        }
    }
}

#[test]
fn actor_collisions_block_the_side_and_allow_steps_onto_low_actors() {
    let (store, registry) = terrain();
    for shape in actors() {
        let world =
            PaletteWorld::for_movement(&store, &registry, 0).with_actor_colliders(vec![shape]);
        let start = Vec3::new(shape.min.x - 0.4, 4.0, 8.0);
        let mut state = PlayerState::new(start);
        state.velocity.x = 0.4;
        let result = Simulator::default()
            .tick(&mut state, MovementInput::default(), &world)
            .unwrap();
        assert!(result.collisions.x);
        assert!(state.position.x < shape.min.x);
        state = PlayerState::new(start);
        state.on_ground = true;
        state.velocity = Vec3::new(0.4, -0.08, 0.0);
        Simulator::default()
            .tick(&mut state, MovementInput::default(), &world)
            .unwrap();
        if shape == lowest_actor() {
            assert!((state.position.y - shape.max.y).abs() < 1e-6);
            assert!(state.position.x > start.x + 0.3);
        } else {
            assert!(state.position.x < shape.min.x);
        }
    }
}

#[test]
fn actor_collisions_follow_new_bounds_and_keep_old_bounds_for_replay() {
    let (store, registry) = terrain();
    let shape = lowest_actor();
    let before = PaletteWorld::for_movement(&store, &registry, 0).with_actor_colliders(vec![shape]);
    let snapshot = before.snapshot().unwrap();
    let after = PaletteWorld::for_movement(&store, &registry, 0)
        .with_actor_colliders(vec![shape.translated(Vec3::new(3.0, 0.0, 0.0))]);
    let mut initial = PlayerState::new(Vec3::new(8.0, shape.max.y, 8.0));
    initial.on_ground = true;
    initial.velocity.y = -0.08;
    let mut retained = initial.clone();
    let simulator = Simulator::default();
    simulator
        .tick(&mut retained, MovementInput::default(), &snapshot)
        .unwrap();
    assert!(retained.on_ground);
    assert!((retained.position.y - shape.max.y).abs() < 1e-6);
    simulator
        .tick(&mut initial, MovementInput::default(), &after)
        .unwrap();
    assert!(!initial.on_ground);
    assert!(initial.position.y < shape.max.y);
    let hits = snapshot.collision_boxes_with_provenance(shape).unwrap();
    assert_eq!(hits.value.len(), 1);
    assert_eq!(hits.value[0].block, None);
    assert_eq!(hits.value[0].runtime_id, None);
}
