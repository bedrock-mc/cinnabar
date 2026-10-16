//! Small actors in a loaded liquid cell, shared by the work-count test and benchmark.

use sim::{Aabb, BlockPhysicsFlags, CollisionRegistry, SurfaceResponse, Vec3};
use world::{BlockUpdate, ChunkKey, ChunkStore, SubChunkKey};

/// Builds a loaded column with water in both layers at the origin.
pub fn world() -> (ChunkStore, CollisionRegistry) {
    let mut registry = CollisionRegistry::new();
    registry.register(0, []).unwrap();
    registry
        .register_physics(
            1,
            [],
            0.6,
            1.0,
            1.0,
            0.875,
            BlockPhysicsFlags::WATER,
            SurfaceResponse::None,
        )
        .unwrap();
    let mut store = ChunkStore::new();
    store.mark_chunk_loaded(ChunkKey::new(0, 0, 0)).unwrap();
    for layer in 0..2 {
        store
            .update_block(
                SubChunkKey::new(0, 0, 0, 0),
                BlockUpdate::new(0, 0, 0, layer, 1),
                0,
            )
            .unwrap();
    }
    (store, registry)
}

/// A small body whose body and breathing probes stay inside the liquid cell.
pub fn body() -> Aabb {
    Aabb::new(Vec3::new(0.3, 0.25, 0.3), Vec3::new(0.7, 0.65, 0.7))
}
