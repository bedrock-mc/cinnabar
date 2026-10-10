use super::*;

#[test]
fn ledge_miss_support_keeps_downward_click_and_chooses_horizontal_faces() {
    let (store, registry, _) = fixture(7);
    let world = PaletteWorld::new(&store, &registry, 0);
    let origin = Vec3::new(0.5, 2.62, 0.5);
    for (direction, face) in [
        (Vec3::new(-0.6, -0.8, 0.0), 4),
        (Vec3::new(0.6, -0.8, 0.0), 5),
        (Vec3::new(0.0, -0.8, -0.6), 2),
        (Vec3::new(0.0, -0.8, 0.6), 3),
        (Vec3::new(0.4, -0.825, -0.4), 2),
        (Vec3::new(-0.4, -0.825, 0.4), 3),
    ] {
        assert!(
            world
                .block_interaction_ray_current(origin, direction, 5.7)
                .unwrap()
                .is_none(),
            "mining still sees a clear miss"
        );
        let hit = world
            .block_use_miss_support_current(origin, direction)
            .unwrap()
            .unwrap();
        assert_eq!((hit.block_pos, hit.face, hit.runtime_id), ([0; 3], face, 7));
        assert_eq!(hit.hit_local, Vec3::new(0.5, 1.0, 0.5));
        assert!((hit.distance - 1.62).abs() < 1.0e-9);
        assert!(
            hit.identity
                .chunks
                .iter()
                .all(|revision| store.collision_revision(revision.chunk) == Some(*revision))
        );
    }
}

#[test]
fn support_requires_a_strictly_steep_look_and_nearby_surface() {
    let (store, registry, _) = fixture(7);
    let world = PaletteWorld::new(&store, &registry, 0);
    let threshold = sim::BLOCK_USE_SUPPORT_MAX_Y;
    for y in [threshold, threshold + 0.001, 0.0, 0.8] {
        let y = f64::from(y);
        assert!(
            world
                .block_use_miss_support_current(
                    Vec3::new(0.5, 2.62, 0.5),
                    Vec3::new(0.0, y, (1.0 - y * y).sqrt()),
                )
                .unwrap()
                .is_none()
        );
    }
    let steep = Vec3::new(0.0, -0.8, -0.6);
    for (distance, accepted) in [
        (sim::BLOCK_USE_SUPPORT_DEPTH - 0.001, true),
        (sim::BLOCK_USE_SUPPORT_DEPTH + 0.001, false),
    ] {
        let hit = world
            .block_use_miss_support_current(Vec3::new(0.5, 1.0 + distance, 0.5), steep)
            .unwrap();
        assert_eq!(hit.is_some(), accepted);
    }
    assert!(
        world
            .block_use_miss_support_current(Vec3::new(1.5, 2.62, 0.5), steep)
            .unwrap()
            .is_none()
    );
}

#[test]
fn indirect_support_rejects_unknown_and_unloaded_geometry() {
    let (mut store, registry, _) = fixture(7);
    set_block(&mut store, [0; 3], 0, 99);
    assert!(matches!(
        PaletteWorld::new(&store, &registry, 0)
            .block_use_miss_support_current(Vec3::new(0.5, 2.62, 0.5), Vec3::new(0.0, -0.8, -0.6),),
        Err(WorldQueryError::UnknownRuntimeId { runtime_id: 99, .. })
    ));
    assert!(matches!(
        PaletteWorld::new(&ChunkStore::new(), &registry, 0)
            .block_use_miss_support_current(Vec3::new(0.5, 2.62, 0.5), Vec3::new(0.0, -0.8, -0.6),),
        Err(WorldQueryError::UnloadedChunk(_))
    ));
}
