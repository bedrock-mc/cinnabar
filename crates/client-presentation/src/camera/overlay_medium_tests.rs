//! Embedded-eye classification using live palette and physics adapters.

use super::*;
use sim::{CollisionRegistry, SurfaceResponse};
use world::{BlockUpdate, ChunkStore, SubChunkKey};

/// Supplies a loaded cell with full scaffold collision and optional waterlogging.
fn fixture(primary: u32, water_height: Option<f64>) -> (ChunkStore, CollisionRegistry) {
    let mut registry = CollisionRegistry::new();
    registry.register(0, []).unwrap();
    let unit = Aabb::new(SimVec3::ZERO, SimVec3::ONE);
    for (id, flags) in [
        (1, BlockPhysicsFlags::SCAFFOLDING),
        (2, BlockPhysicsFlags::default()),
    ] {
        registry
            .register_physics(
                id,
                [unit],
                sim::DEFAULT_SURFACE_FRICTION,
                1.0,
                1.0,
                0.0,
                flags,
                SurfaceResponse::None,
            )
            .unwrap();
    }
    registry
        .register_physics(
            3,
            [],
            sim::DEFAULT_SURFACE_FRICTION,
            1.0,
            1.0,
            water_height.unwrap_or(1.0),
            BlockPhysicsFlags::WATER,
            SurfaceResponse::None,
        )
        .unwrap();
    let mut store = ChunkStore::new();
    let key = SubChunkKey::new(0, 0, 0, 0);
    store.apply_request_mode_air(key).unwrap();
    store.mark_sub_chunk_loaded(key).unwrap();
    store
        .update_block(key, BlockUpdate::new(0, 0, 0, 0, primary), 0)
        .unwrap();
    if water_height.is_some() {
        store
            .update_block(key, BlockUpdate::new(0, 0, 0, 1, 3), 0)
            .unwrap();
    }
    (store, registry)
}

#[test]
fn scaffolding_keeps_the_embedded_view_clear_without_changing_water_or_stone() {
    for (primary, water_height, expected) in [
        (1, None, HeadMedium::Air),
        (1, Some(1.0), HeadMedium::Water),
        (1, Some(0.25), HeadMedium::Air),
        (2, None, HeadMedium::Solid),
        (0, None, HeadMedium::Air),
    ] {
        let (store, registry) = fixture(primary, water_height);
        let world = sim::PaletteWorld::new(&store, &registry, 0, Default::default());
        assert_eq!(
            probe_head_medium(&world, Vec3::splat(0.5)),
            expected,
            "primary={primary} water_height={water_height:?}"
        );
    }
}
