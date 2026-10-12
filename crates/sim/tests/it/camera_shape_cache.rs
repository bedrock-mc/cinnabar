use sim::{Aabb, CameraShapeCache, CollisionRegistry, CollisionWorld, PaletteWorld, Vec3};
use world::{BlockUpdate, ChunkStore, SubChunkKey};

/// A full cube and a fence-like block that reaches into the cell above, on both sides of a
/// sub-chunk boundary.
fn fixture() -> (CollisionRegistry, ChunkStore) {
    let mut registry = CollisionRegistry::new();
    registry.register(0, []).unwrap();
    registry
        .register(1, [Aabb::new(Vec3::ZERO, Vec3::ONE)])
        .unwrap();
    registry
        .register(
            2,
            [Aabb::new(
                Vec3::new(0.375, 0.0, 0.375),
                Vec3::new(0.625, 1.5, 0.625),
            )],
        )
        .unwrap();
    let mut store = ChunkStore::new();
    for x in -1..=1 {
        for z in -1..=1 {
            store
                .mark_sub_chunk_loaded(SubChunkKey::new(0, x, 0, z))
                .unwrap();
        }
    }
    for (x, y, z, id) in [(14, 4, 3, 1), (15, 4, 3, 2), (0, 4, 3, 1), (1, 4, 3, 2)] {
        let key = SubChunkKey::new(0, if x >= 14 { -1 } else { 0 }, 0, 0);
        store
            .update_block(key, BlockUpdate::new(x, y, z, 0, id), 0)
            .unwrap();
    }
    (registry, store)
}

// One frame's particles share a cache; each query must still see exactly the shapes an uncached
// scan returns, in the same order, including shapes reaching in from a neighbouring cell.
#[test]
fn cached_camera_scans_match_uncached_scans() {
    let (registry, store) = fixture();
    let world = PaletteWorld::new(&store, &registry, 0, Default::default());
    let mut cache = CameraShapeCache::default();
    let mut seen = 0;
    for (min, max) in [
        ([-2.6, 4.9, 3.1], [-1.4, 5.4, 3.9]),
        ([-1.2, 3.5, 2.5], [1.8, 5.8, 4.5]),
        ([0.2, 5.2, 3.2], [0.4, 5.4, 3.4]),
        ([1.4, 5.3, 3.4], [1.6, 5.45, 3.6]),
        ([-3.0, 6.0, 3.0], [-2.0, 7.0, 4.0]),
    ] {
        let query = Aabb::new(
            Vec3::new(min[0], min[1], min[2]),
            Vec3::new(max[0], max[1], max[2]),
        );
        let uncached = world.collision_boxes_camera_lenient(query).unwrap().value;
        let mut cached = Vec::new();
        world
            .visit_camera_colliders_cached(query, &mut cache, &mut |shape| cached.push(shape))
            .unwrap();
        assert_eq!(cached, uncached, "query {min:?}..{max:?}");
        seen += cached.len();
    }
    assert!(seen >= 4, "the queries reached only {seen} shapes");
}
