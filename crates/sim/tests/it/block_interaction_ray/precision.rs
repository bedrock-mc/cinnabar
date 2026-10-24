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
    let origin = Vec3::new(-1.0, 0.5, -1.0);
    let hit = PaletteWorld::new(&store, &registry, 0, Default::default())
        .block_interaction_ray_current(origin, Vec3::new(1.0, 0.0, 1.0), 4.0)
        .unwrap()
        .unwrap();
    assert_eq!(hit.hit_local, Vec3::new(0.0, 0.5, 0.0));
    // The float intercept interpolates every axis, so it lands one ulp short of the corner.
    let [x, y, z] = [
        hit.hit_world.x as f32 - origin.x as f32,
        hit.hit_world.y as f32 - origin.y as f32,
        hit.hit_world.z as f32 - origin.z as f32,
    ];
    assert!((x - 1.0).abs() <= f32::EPSILON && y == 0.0 && (z - 1.0).abs() <= f32::EPSILON);
    assert_eq!(hit.distance, f64::from((z * z + y * y + x * x).sqrt()));
    let [x, y, z] = [f64::from(x), f64::from(y), f64::from(z)];
    assert_ne!(hit.distance, (z * z + y * y + x * x).sqrt());
}

#[test]
fn endpoint_cells_remain_reachable_after_many_crossings() {
    use sim::CollisionWorld;

    for axis in 0..3 {
        let (mut store, chunks) = loaded_air_store(ChunkKey::new(0, 0, 0));
        let mut block = [0; 3];
        block[axis] = 10;
        set_block(&mut store, block, 0, 7);
        let mut registry = CollisionRegistry::with_identity(identity());
        registry.register(0, []).unwrap();
        registry
            .register(7, [Aabb::new(Vec3::ZERO, Vec3::ONE)])
            .unwrap();
        let expected = expected(&store, &chunks);
        let world = PaletteWorld::new(&store, &registry, 0, Default::default());
        let mut origin = Vec3::new(0.5, 0.5, 0.5);
        origin[axis] = 0.0;
        let mut direction = Vec3::ZERO;
        direction[axis] = 1.0;
        let delta = direction * 10.0;

        let hit = world
            .block_interaction_ray(origin, direction, 10.0, &expected)
            .unwrap()
            .unwrap();
        assert_eq!(hit.block_pos, block);
        assert_eq!(hit.distance, 10.0);
        let visible = world
            .camera_visibility_ray(origin, direction, 10.0)
            .unwrap()
            .unwrap();
        assert_eq!(visible.block_pos, block);
        assert_eq!(visible.distance, hit.distance);
        assert_eq!(
            world.camera_segment_entry(origin, delta).unwrap().0,
            Some(1.0)
        );
        for outlines in [false, true] {
            let aimed = world
                .camera_aim_ray(origin, origin + delta, 10, outlines, false)
                .unwrap()
                .unwrap();
            assert_eq!(aimed.block_pos, block);
            assert_eq!(aimed.distance, hit.distance);
        }
    }
}

#[test]
fn rounded_zero_motion_in_a_face_plane_meets_the_face_it_crosses() {
    let (mut store, _) = loaded_air_store(ChunkKey::new(0, 0, 0));
    set_block(&mut store, [10, 0, 0], 0, 7);
    let mut registry = CollisionRegistry::with_identity(identity());
    registry.register(0, []).unwrap();
    registry
        .register(7, [Aabb::new(Vec3::ZERO, Vec3::ONE)])
        .unwrap();
    let world = PaletteWorld::new(&store, &registry, 0, Default::default());

    // On the low face plane the walked cell column holds the block, and closed face bounds admit
    // the crossing; on the high plane the walk stays in the neighbouring column.
    for (x, dx, hits) in [(10.0, -1.0e-8, true), (11.0, 1.0e-8, false)] {
        let origin = Vec3::new(x, 0.5, -1.0);
        let direction = Vec3::new(dx, 0.0, 1.0);
        let hit = world
            .block_interaction_ray_current(origin, direction, 3.0)
            .unwrap();
        assert_eq!(
            hit.map(|hit| (hit.block_pos, hit.distance)),
            hits.then_some(([10, 0, 0], 1.0))
        );
        let visible = world.camera_visibility_ray(origin, direction, 3.0).unwrap();
        assert_eq!(visible.map(|hit| hit.block_pos), hits.then_some([10, 0, 0]));
        for outlines in [false, true] {
            let aimed = world
                .camera_aim_ray(origin, origin + direction * 3.0, 3, outlines, false)
                .unwrap();
            assert_eq!(aimed.map(|hit| hit.block_pos), hits.then_some([10, 0, 0]));
        }
    }
}
