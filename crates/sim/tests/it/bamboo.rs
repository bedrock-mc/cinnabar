use sim::{Aabb, CollisionRegistry, CollisionWorld, PaletteWorld, Vec3};
use world::{BlockUpdate, ChunkStore, SubChunkKey};

fn fixture() -> (CollisionRegistry, ChunkStore) {
    let mut registry = CollisionRegistry::new();
    registry.register(0, []).unwrap();
    let origin = Aabb::new(
        Vec3::new(0.31666666, 0.0, 0.38333333),
        Vec3::new(0.44166666, 1.0, 0.50833333),
    );
    registry.register(1, [origin]).unwrap();
    registry.set_pick_shapes(1, [origin]);
    assert!(registry.set_bamboo_column_offset(1));
    let mut store = ChunkStore::new();
    for x in -1..=1 {
        for z in -1..=1 {
            store
                .mark_sub_chunk_loaded(SubChunkKey::new(0, x, 0, z))
                .unwrap();
        }
    }
    for y in [8, 9] {
        store
            .update_block(
                SubChunkKey::new(0, 0, 0, 0),
                BlockUpdate::new(1, y, 0, 0, 1),
                0,
            )
            .unwrap();
    }
    (registry, store)
}

#[test]
fn bamboo_visible_column_is_pickable_and_its_old_sampled_position_is_empty() {
    let (registry, store) = fixture();
    let world = PaletteWorld::new(&store, &registry, 0);
    for y in [8.5, 9.5] {
        let origin = Vec3::new(1.8125, y, 2.0);
        let hit = world
            .block_interaction_ray_current(origin, Vec3::new(0.0, 0.0, -1.0), 4.0)
            .unwrap();
        assert_eq!(hit.unwrap().block_pos, [1, y as i32, 0]);
        let camera = world
            .camera_visibility_ray(origin, Vec3::new(0.0, 0.0, -1.0), 4.0)
            .unwrap()
            .unwrap();
        assert!((camera.selection_bounds.min.x - 1.75).abs() < 1e-7);
        assert!((camera.selection_bounds.max.x - 1.875).abs() < 1e-7);
        let aim = world
            .camera_aim_ray(origin, origin + Vec3::new(0.0, 0.0, -4.0), 10, true, false)
            .unwrap()
            .unwrap();
        assert_eq!(aim.selection_bounds, camera.selection_bounds);
        assert!(
            world
                .camera_segment_entry(origin, Vec3::new(0.0, 0.0, -4.0))
                .unwrap()
                .0
                .is_some()
        );
        let empty = Vec3::new(1.375, y, 2.0);
        assert!(
            world
                .block_interaction_ray_current(empty, Vec3::new(0.0, 0.0, -1.0), 4.0)
                .unwrap()
                .is_none()
        );
    }
}

#[test]
fn bamboo_collision_and_camera_use_the_same_displaced_stalk() {
    let (registry, store) = fixture();
    let world = PaletteWorld::new(&store, &registry, 0);
    let visible = Aabb::new(Vec3::new(1.76, 8.1, 0.49), Vec3::new(1.86, 8.9, 0.60));
    let collision = world.collision_boxes(visible).unwrap();
    assert_eq!(collision.value.len(), 1);
    assert!((collision.value[0].min.x - 1.75).abs() < 1e-7);
    let camera = world.collision_boxes_camera_lenient(visible).unwrap();
    assert_eq!(camera.value, collision.value);
    let old = Aabb::new(Vec3::new(1.32, 8.1, 0.39), Vec3::new(1.44, 8.9, 0.50));
    assert!(world.collision_boxes(old).unwrap().value.is_empty());
    let offset = registry.block_shape_offset(1, [1, 8, 0]).unwrap();
    let outline = registry.selection_shapes(1).unwrap()[0].translated(offset);
    assert_eq!(outline, collision.value[0]);
}
