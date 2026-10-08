use sim::{CollisionRegistry, PaletteWorld, WorldQueryError};
use world::{BlockUpdate, ChunkKey, ChunkStore, SubChunkKey};

#[test]
fn primary_runtime_lookup_preserves_sparse_air_negative_cells_and_storage_order() {
    let air = 23;
    let primary = 71;
    let extra = 99;
    let mut registry = CollisionRegistry::new();
    registry.set_air_runtime_id(air);
    let mut store = ChunkStore::new();
    let key = SubChunkKey::new(3, -1, -1, -1);
    store.mark_sub_chunk_loaded(key).unwrap();
    assert_eq!(
        PaletteWorld::new(&store, &registry, 3).primary_runtime_id([-1; 3]),
        Ok(air)
    );
    for (layer, id) in [(0, primary), (1, extra)] {
        store
            .update_block(key, BlockUpdate::new(15, 15, 15, layer, id), air)
            .unwrap();
    }
    let world = PaletteWorld::new(&store, &registry, 3);
    assert_eq!(world.primary_runtime_id([-1; 3]), Ok(primary));
    assert_eq!(world.primary_runtime_id([-2, -1, -1]), Ok(air));
    assert!(matches!(
        world.primary_runtime_id([0; 3]),
        Err(WorldQueryError::UnloadedChunk(_))
    ));

    // A complete sparse air column covers all Y without a stored sub-chunk.
    store.mark_chunk_loaded(ChunkKey::new(3, 0, 0)).unwrap();
    assert_eq!(
        PaletteWorld::new(&store, &registry, 3).primary_runtime_id([0, 1000, 0]),
        Ok(air)
    );
}

#[test]
fn primary_air_is_not_replaced_by_a_non_air_extra_layer() {
    let air = 23;
    let mut registry = CollisionRegistry::new();
    registry.set_air_runtime_id(air);
    let mut store = ChunkStore::new();
    let key = SubChunkKey::new(0, 0, 0, 0);
    store.mark_sub_chunk_loaded(key).unwrap();
    store
        .update_block(key, BlockUpdate::new(0, 0, 0, 1, 99), air)
        .unwrap();
    assert_eq!(
        PaletteWorld::new(&store, &registry, 0).primary_runtime_id([0; 3]),
        Ok(air)
    );
}
