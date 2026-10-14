use super::*;

#[test]
fn click_intercept_uses_the_rounded_world_segment() {
    let (mut store, _) = loaded_air_store(ChunkKey::new(0, 524_288, 0));
    set_block(&mut store, [8_388_609, 1, 0], 0, 7);
    let mut registry = CollisionRegistry::with_identity(identity());
    registry.register(0, []).unwrap();
    registry
        .register(7, [Aabb::new(Vec3::ZERO, Vec3::ONE)])
        .unwrap();
    let world = PaletteWorld::new(&store, &registry, 0, Default::default());
    let origin = Vec3::new(8_388_608.0, 0.5, 0.5);
    // X ends one block from the origin after rounding; Y travels 1.6 blocks.
    // The rounded segment passes above the target before reaching its west face.
    assert!(
        world
            .block_interaction_ray_current(origin, Vec3::new(0.6, 0.8, 0.0), 2.0)
            .unwrap()
            .is_none()
    );
    assert!(
        world
            .camera_visibility_ray(origin, Vec3::new(0.6, 0.8, 0.0), 2.0)
            .unwrap()
            .is_none()
    );
}

#[test]
fn block_hit_distance_is_a_widened_float_square_root() {
    let (store, registry, _) = fixture(7);
    let hit = PaletteWorld::new(&store, &registry, 0, Default::default())
        .block_interaction_ray_current(Vec3::new(-1.0, 0.5, -1.0), Vec3::new(1.0, 0.0, 1.0), 4.0)
        .unwrap()
        .unwrap();
    assert_eq!(hit.hit_local, Vec3::new(0.0, 0.5, 0.0));
    assert_eq!(hit.distance, f64::from(2.0_f32.sqrt()));
    assert_ne!(hit.distance, 2.0_f64.sqrt());
}
