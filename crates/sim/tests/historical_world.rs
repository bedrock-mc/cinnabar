use sim::{
    Aabb, CollisionRegistry, CollisionWorld, MovementInput, PaletteWorld, PlayerState,
    PredictionHistory, Simulator, Vec3,
};
use world::{BlockUpdate, ChunkKey, ChunkStore, SubChunkKey};

/// Replay retains each tick's palettes and registry after updates and complete unloads.
#[test]
fn replay_uses_each_historical_world_after_the_live_chunk_is_evicted() {
    let mut registry = CollisionRegistry::new();
    registry.register(0, []).unwrap();
    registry
        .register(1, [Aabb::new(Vec3::ZERO, Vec3::ONE)])
        .unwrap();
    let mut store = ChunkStore::new();
    let key = SubChunkKey::new(0, 0, 0, 0);
    store.mark_sub_chunk_loaded(key).unwrap();
    let simulator = Simulator::default();
    let mut state = PlayerState::new(Vec3::new(8.5, 8.0, 8.5));
    state.velocity.z = 0.2;
    let mut history = PredictionHistory::new(8).unwrap();
    let mut anchor = None;
    for tick in 1..=4 {
        if tick == 2 || tick == 3 {
            store
                .update_block(key, BlockUpdate::new(8, 8, 9, 0, u32::from(tick == 2)), 0)
                .unwrap();
        }
        history
            .predict(
                &mut state,
                MovementInput {
                    forward: 1.0,
                    ..MovementInput::default()
                },
                &simulator,
                &PaletteWorld::new(&store, &registry, 0),
            )
            .unwrap();
        if tick == 1 {
            anchor = Some(state.clone());
        }
    }
    let expected = state.clone();
    store.evict_chunk(ChunkKey::new(0, 0, 0));
    registry.remove_runtime_id(1);
    let result = history
        .rewind_and_replay(
            &mut state,
            anchor.unwrap(),
            &simulator,
            &PaletteWorld::new(&store, &registry, 0),
        )
        .unwrap();
    assert_eq!(result.replayed_ticks, 3);
    assert_eq!(state, expected);
}

/// Snapshots keep block geometry, provenance and collision revisions immutable.
#[test]
fn snapshot_keeps_collision_identity_and_geometry_after_live_replacement() {
    let mut registry = CollisionRegistry::new();
    registry.register(0, []).unwrap();
    registry
        .register(1, [Aabb::new(Vec3::ZERO, Vec3::ONE)])
        .unwrap();
    let mut store = ChunkStore::new();
    let key = SubChunkKey::new(0, 0, 0, 0);
    store.mark_sub_chunk_loaded(key).unwrap();
    store
        .update_block(key, BlockUpdate::new(8, 8, 8, 0, 1), 0)
        .unwrap();
    let query = Aabb::new(Vec3::new(8.0, 8.0, 8.0), Vec3::new(9.0, 9.0, 9.0));
    let original = PaletteWorld::new(&store, &registry, 0);
    let expected = original.collision_boxes_with_provenance(query).unwrap();
    let expected_material = original.primary_is_air([8, 8, 8]).unwrap().unwrap();
    assert!(!expected_material.value);
    let snapshot = original.snapshot().unwrap();
    store
        .update_block(key, BlockUpdate::new(8, 8, 8, 0, 0), 0)
        .unwrap();
    assert_ne!(
        PaletteWorld::new(&store, &registry, 0)
            .collision_boxes_with_provenance(query)
            .unwrap(),
        expected
    );
    assert_eq!(
        snapshot.collision_boxes_with_provenance(query).unwrap(),
        expected
    );
    let live_material = PaletteWorld::new(&store, &registry, 0)
        .primary_is_air([8, 8, 8])
        .unwrap()
        .unwrap();
    assert!(live_material.value);
    assert_ne!(live_material.identity, expected_material.identity);
    assert_eq!(
        snapshot.primary_is_air([8, 8, 8]).unwrap().unwrap(),
        expected_material
    );
}
