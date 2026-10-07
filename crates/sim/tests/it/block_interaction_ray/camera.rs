use super::*;

#[test]
fn camera_visibility_matches_interaction_geometry_without_allocations() {
    let (store, registry, _) = fixture(7);
    let world = PaletteWorld::new(&store, &registry, 0);
    for (origin, direction) in [
        (Vec3::new(0.5, 0.5, -1.0), Vec3::new(0.0, 0.0, 1.0)),
        (Vec3::new(-1.0, -1.0, -1.0), Vec3::ONE),
        (Vec3::new(0.5, 0.5, 0.5), Vec3::new(1.0, 0.0, 0.0)),
        (Vec3::new(0.5, 1.0, 0.5), Vec3::new(0.0, -1.0, 0.0)),
    ] {
        let (authority, before) = crate::allocation_count::measure(|| {
            world
                .block_interaction_ray_current(origin, direction, 5.0)
                .unwrap()
                .unwrap()
        });
        let (hit, after) = crate::allocation_count::measure(|| {
            world
                .camera_visibility_ray(origin, direction, 5.0)
                .unwrap()
                .unwrap()
        });
        assert!(before > 0);
        assert_eq!(after, 0);
        assert_eq!(hit.block_pos, authority.block_pos);
        assert_eq!(hit.face, authority.face);
        assert_eq!(hit.distance, authority.distance);
        assert_eq!(hit.hit_local, authority.hit_local);
        assert_eq!(hit.runtime_id, authority.runtime_id);
        eprintln!("camera ray allocations: authority={before}, camera={after}");
    }
}

#[test]
fn camera_visibility_uses_paired_door_shape_without_allocations() {
    let (mut store, mut registry, _) = fixture(7);
    set_block(&mut store, [0, 1, 0], 0, 8);
    registry
        .register(8, [Aabb::new(Vec3::ZERO, Vec3::ONE)])
        .unwrap();
    for (id, upper) in [(7, false), (8, true)] {
        registry.set_door_state(
            id,
            sim::DoorState {
                family: "test:door".into(),
                facing: sim::DoorFacing::South,
                upper,
                open: false,
                hinge_right: false,
            },
        );
    }
    let world = PaletteWorld::new(&store, &registry, 0);
    let origin = Vec3::new(-1.0, 0.5, 0.5);
    let direction = Vec3::new(1.0, 0.0, 0.0);
    let authority = world
        .block_interaction_ray_current(origin, direction, 3.0)
        .unwrap()
        .unwrap();
    let (hit, allocations) = crate::allocation_count::measure(|| {
        world
            .camera_visibility_ray(origin, direction, 3.0)
            .unwrap()
            .unwrap()
    });
    assert_eq!(allocations, 0);
    assert_eq!(hit.distance, authority.distance);
    assert_eq!(hit.face, authority.face);
}

#[test]
fn camera_visibility_rejects_unloaded_or_unknown_geometry() {
    let (store, mut registry, _) = fixture(7);
    registry.remove_runtime_id(7);
    assert!(matches!(
        PaletteWorld::new(&store, &registry, 0).camera_visibility_ray(
            Vec3::new(0.5, 0.5, -1.0),
            Vec3::new(0.0, 0.0, 1.0),
            2.0
        ),
        Err(WorldQueryError::UnknownRuntimeId { runtime_id: 7, .. })
    ));
    assert!(matches!(
        PaletteWorld::new(&ChunkStore::new(), &registry, 0).camera_visibility_ray(
            Vec3::ZERO,
            Vec3::ONE,
            2.0
        ),
        Err(WorldQueryError::UnloadedChunk(_))
    ));
}

#[test]
fn aim_sampling_uses_cell_cubes_and_visibility_uses_outlines() {
    let (store, mut registry, _) = fixture(7);
    registry
        .register(7, [Aabb::new(Vec3::ZERO, Vec3::new(1.0, 0.5, 1.0))])
        .unwrap();
    let world = PaletteWorld::new(&store, &registry, 0);
    let origin = Vec3::new(0.5, 0.75, -1.0);
    let end = Vec3::new(0.5, 0.75, 2.0);
    let (sample, allocations) = crate::allocation_count::measure(|| {
        world
            .camera_aim_ray(origin, end, 3, false, false)
            .unwrap()
            .unwrap()
    });
    assert_eq!(allocations, 0);
    assert_eq!(sample.block_pos, [0, 0, 0]);
    assert_eq!(sample.selection_bounds.max.y, 0.5);
    assert!(
        world
            .camera_aim_ray(origin, end, 3, true, false)
            .unwrap()
            .is_none()
    );
    assert!(
        world
            .camera_aim_ray(origin, end, 0, false, false)
            .unwrap()
            .is_none()
    );
    assert!(
        world
            .camera_aim_ray(Vec3::new(0.5, 0.25, 0.5), end, 3, false, false)
            .unwrap()
            .is_none()
    );
}

#[test]
fn aim_liquids_use_full_outline_source_gating_and_extra_layer_priority() {
    let (mut store, mut registry, _) = fixture(7);
    for (id, depth) in [(8, 0), (9, 4)] {
        registry
            .register_physics(
                id,
                [],
                0.6,
                1.0,
                1.0,
                0.5,
                sim::BlockPhysicsFlags::WATER,
                sim::SurfaceResponse::None,
            )
            .unwrap();
        registry.set_flow_facts(
            id,
            sim::FlowBlockFacts {
                blocks_motion: false,
                is_solid: false,
                blocked_faces: 0,
                allowed_faces: 63,
                liquid_depth: Some(depth),
            },
        );
    }
    let origin = Vec3::new(0.5, 0.9, -1.0);
    let end = Vec3::new(0.5, 0.9, 2.0);
    set_block(&mut store, [0, 0, 0], 0, 8);
    let world = PaletteWorld::new(&store, &registry, 0);
    assert!(
        world
            .camera_aim_ray(origin, end, 3, false, false)
            .unwrap()
            .is_none()
    );
    let (source, allocations) = crate::allocation_count::measure(|| {
        world
            .camera_aim_ray(origin, end, 3, true, true)
            .unwrap()
            .unwrap()
    });
    assert_eq!(allocations, 0);
    assert!(source.targetable);
    assert_eq!(source.selection_bounds.max, Vec3::ONE);
    assert!(
        world
            .camera_eye_in_liquid(Vec3::new(0.5, 0.9, 0.5))
            .unwrap()
    );
    set_block(&mut store, [0, 0, 0], 0, 9);
    assert!(
        !PaletteWorld::new(&store, &registry, 0)
            .camera_aim_ray(origin, end, 3, true, true)
            .unwrap()
            .unwrap()
            .targetable
    );
    set_block(&mut store, [0, 0, 0], 0, 7);
    set_block(&mut store, [0, 0, 0], 1, 9);
    let world = PaletteWorld::new(&store, &registry, 0);
    let extra = world
        .camera_aim_ray(origin, end, 3, true, true)
        .unwrap()
        .unwrap();
    assert_eq!(extra.runtime_id, 9);
    assert!(extra.targetable);
    assert!(
        !world
            .camera_eye_in_liquid(Vec3::new(0.5, 0.9, 0.5))
            .unwrap()
    );
    assert_eq!(
        world
            .camera_aim_ray(origin, end, 3, true, false)
            .unwrap()
            .unwrap()
            .runtime_id,
        7
    );
}

#[test]
fn aim_step_budget_counts_cells_and_corner_ties_choose_z_first() {
    let (mut store, registry, _) = fixture(7);
    set_block(&mut store, [0, 0, 0], 0, 0);
    set_block(&mut store, [0, 0, 1], 0, 7);
    set_block(&mut store, [1, 0, 0], 0, 7);
    let world = PaletteWorld::new(&store, &registry, 0);
    let origin = Vec3::new(0.5, 0.5, 0.5);
    let end = Vec3::new(2.5, 0.5, 2.5);
    assert_eq!(
        world
            .camera_aim_ray(origin, end, 1, false, false)
            .unwrap()
            .unwrap()
            .block_pos,
        [0, 0, 1]
    );
    assert!(
        world
            .camera_aim_ray(origin, end, 0, false, false)
            .unwrap()
            .is_none()
    );
}
