use sim::{
    Aabb, CollisionIdSpace, CollisionRegistry, CollisionRegistryIdentity, PaletteWorld, Vec3,
    WorldCollisionIdentity, WorldQueryError,
};
use world::{BlockUpdate, ChunkKey, ChunkStore, SubChunkKey};

fn identity() -> CollisionRegistryIdentity {
    CollisionRegistryIdentity {
        protocol: 2168,
        id_space: CollisionIdSpace::Sequential,
        preg_sha256: [0x5a; 32],
    }
}

fn zig_zag_i32(value: i32) -> Vec<u8> {
    let mut value = ((value as u32) << 1) ^ ((value >> 31) as u32);
    let mut encoded = Vec::new();
    loop {
        let mut byte = (value & 0x7f) as u8;
        value >>= 7;
        if value != 0 {
            byte |= 0x80;
        }
        encoded.push(byte);
        if value == 0 {
            return encoded;
        }
    }
}

fn uniform(runtime_id: u32) -> Vec<u8> {
    let mut bytes = vec![9, 1, 0, 1];
    bytes.extend(zig_zag_i32(runtime_id as i32));
    bytes
}

fn loaded_air_store(center: ChunkKey) -> (ChunkStore, Vec<ChunkKey>) {
    let mut store = ChunkStore::new();
    let chunks = (center.x - 2..=center.x + 2)
        .flat_map(|x| {
            (center.z - 2..=center.z + 2).map(move |z| ChunkKey::new(center.dimension, x, z))
        })
        .collect::<Vec<_>>();
    for &chunk in &chunks {
        for y in -3..=3 {
            let key = SubChunkKey::from_chunk(chunk, y);
            store.apply_request_mode_air(key).unwrap();
            store.mark_sub_chunk_loaded(key).unwrap();
        }
    }
    (store, chunks)
}

fn set_block(store: &mut ChunkStore, block: [i32; 3], layer: u32, runtime_id: u32) {
    let key = SubChunkKey::new(0, block[0] >> 4, block[1] >> 4, block[2] >> 4);
    store
        .update_block(
            key,
            BlockUpdate::new(
                block[0].rem_euclid(16) as u8,
                block[1].rem_euclid(16) as u8,
                block[2].rem_euclid(16) as u8,
                layer,
                runtime_id,
            ),
            0,
        )
        .unwrap();
}

fn expected(store: &ChunkStore, chunks: &[ChunkKey]) -> WorldCollisionIdentity {
    WorldCollisionIdentity::new(
        identity(),
        chunks
            .iter()
            .map(|&chunk| store.collision_revision(chunk).unwrap()),
    )
    .unwrap()
}

fn fixture(runtime_id: u32) -> (ChunkStore, CollisionRegistry, WorldCollisionIdentity) {
    let (mut store, chunks) = loaded_air_store(ChunkKey::new(0, 0, 0));
    set_block(&mut store, [0, 0, 0], 0, runtime_id);
    let expected = WorldCollisionIdentity::new(
        identity(),
        chunks
            .into_iter()
            .map(|chunk| store.collision_revision(chunk).unwrap()),
    )
    .unwrap();
    let mut registry = CollisionRegistry::with_identity(identity());
    registry.register(0, []).unwrap();
    registry
        .register(runtime_id, [Aabb::new(Vec3::ZERO, Vec3::ONE)])
        .unwrap();
    (store, registry, expected)
}

#[test]
fn full_cube_hit_reports_authoritative_contract() {
    let (store, registry, expected) = fixture(7);
    let hit = PaletteWorld::new(&store, &registry, 0)
        .block_interaction_ray(
            Vec3::new(0.5, 0.5, -1.0),
            Vec3::new(0.0, 0.0, 2.0),
            2.0,
            &expected,
        )
        .unwrap()
        .unwrap();

    assert_eq!(hit.block_pos, [0, 0, 0]);
    assert_eq!(hit.face, 2);
    assert_eq!(hit.hit_local, Vec3::new(0.5, 0.5, 0.0));
    assert_eq!(hit.runtime_id, 7);
    assert_eq!(hit.distance, 1.0);
    assert_eq!(hit.identity.registry, identity());
    assert!(!hit.identity.chunks.is_empty());
    assert!(
        hit.identity
            .chunks
            .iter()
            .all(|revision| expected.chunks.contains(revision))
    );
}

#[test]
fn all_six_faces_use_the_bedrock_numeric_mapping() {
    let (store, registry, expected) = fixture(7);
    let world = PaletteWorld::new(&store, &registry, 0);
    for (origin, direction, face) in [
        (Vec3::new(0.5, -1.0, 0.5), Vec3::new(0.0, 1.0, 0.0), 0),
        (Vec3::new(0.5, 2.0, 0.5), Vec3::new(0.0, -1.0, 0.0), 1),
        (Vec3::new(0.5, 0.5, -1.0), Vec3::new(0.0, 0.0, 1.0), 2),
        (Vec3::new(0.5, 0.5, 2.0), Vec3::new(0.0, 0.0, -1.0), 3),
        (Vec3::new(-1.0, 0.5, 0.5), Vec3::new(1.0, 0.0, 0.0), 4),
        (Vec3::new(2.0, 0.5, 0.5), Vec3::new(-1.0, 0.0, 0.0), 5),
    ] {
        let hit = world
            .block_interaction_ray(origin, direction, 2.0, &expected)
            .unwrap()
            .unwrap();
        assert_eq!(hit.face, face);
        assert_eq!(hit.distance, 1.0);
        assert!(
            [hit.hit_local.x, hit.hit_local.y, hit.hit_local.z]
                .into_iter()
                .all(|value| value.is_finite() && (0.0..=1.0).contains(&value))
        );
    }
}

#[test]
fn exact_boundary_and_corner_ties_are_deterministic() {
    let (mut store, chunks) = loaded_air_store(ChunkKey::new(0, 0, 0));
    set_block(&mut store, [0, 0, 0], 0, 7);
    set_block(&mut store, [0, -1, 0], 0, 8);
    let expected = expected(&store, &chunks);
    let mut registry = CollisionRegistry::with_identity(identity());
    registry.register(0, []).unwrap();
    registry
        .register(7, [Aabb::new(Vec3::ZERO, Vec3::ONE)])
        .unwrap();
    registry
        .register(8, [Aabb::new(Vec3::ZERO, Vec3::ONE)])
        .unwrap();
    let world = PaletteWorld::new(&store, &registry, 0);

    let boundary = world
        .block_interaction_ray(
            Vec3::new(0.0, 0.5, 0.5),
            Vec3::new(1.0, 0.0, 0.0),
            1.0,
            &expected,
        )
        .unwrap()
        .unwrap();
    assert_eq!(
        (boundary.block_pos, boundary.face, boundary.distance),
        ([0, 0, 0], 4, 0.0)
    );

    let tied = world
        .block_interaction_ray(
            Vec3::new(-1.0, -1.0, 0.5),
            Vec3::new(1.0, 1.0, 0.0),
            3.0,
            &expected,
        )
        .unwrap()
        .unwrap();
    assert_eq!(tied.block_pos, [0, -1, 0]);
    assert_eq!(tied.distance, 2.0_f64.sqrt());
}

#[test]
fn fractional_two_axis_crossing_visits_every_tied_cell() {
    let (mut store, chunks) = loaded_air_store(ChunkKey::new(0, 0, 0));
    for (runtime_id, block) in [(20, [1, 0, 0]), (21, [0, 1, 0]), (22, [1, 1, 0])] {
        set_block(&mut store, block, 0, runtime_id);
    }
    let expected = expected(&store, &chunks);
    let mut registry = CollisionRegistry::with_identity(identity());
    registry.register(0, []).unwrap();
    for runtime_id in 20..=22 {
        registry
            .register(runtime_id, [Aabb::new(Vec3::ZERO, Vec3::ONE)])
            .unwrap();
    }
    let hit = PaletteWorld::new(&store, &registry, 0)
        .block_interaction_ray(
            Vec3::new(0.9, 0.7, 0.5),
            Vec3::new(0.1, 0.3, 0.0),
            2.0,
            &expected,
        )
        .unwrap()
        .unwrap();
    assert_eq!((hit.block_pos, hit.runtime_id), ([0, 1, 0], 21));
}

#[test]
fn fractional_three_axis_crossing_visits_every_proper_subset_cell() {
    let (mut store, chunks) = loaded_air_store(ChunkKey::new(0, 0, 0));
    let blocks = [
        [1, 0, 0],
        [0, 1, 0],
        [0, 0, 1],
        [1, 1, 0],
        [1, 0, 1],
        [0, 1, 1],
        [1, 1, 1],
    ];
    for (index, block) in blocks.into_iter().enumerate() {
        set_block(&mut store, block, 0, 30 + index as u32);
    }
    let expected = expected(&store, &chunks);
    let mut registry = CollisionRegistry::with_identity(identity());
    registry.register(0, []).unwrap();
    for runtime_id in 30..37 {
        registry
            .register(runtime_id, [Aabb::new(Vec3::ZERO, Vec3::ONE)])
            .unwrap();
    }
    let hit = PaletteWorld::new(&store, &registry, 0)
        .block_interaction_ray(
            Vec3::new(0.9, 0.7, 0.4),
            Vec3::new(0.1, 0.3, 0.6),
            2.0,
            &expected,
        )
        .unwrap()
        .unwrap();
    assert_eq!((hit.block_pos, hit.runtime_id), ([0, 0, 1], 32));
}

#[test]
fn close_but_distinct_crossings_are_not_merged() {
    let (mut store, chunks) = loaded_air_store(ChunkKey::new(0, 0, 0));
    set_block(&mut store, [1, 0, 0], 0, 40);
    set_block(&mut store, [0, 1, 0], 0, 41);
    let expected = expected(&store, &chunks);
    let mut registry = CollisionRegistry::with_identity(identity());
    registry.register(0, []).unwrap();
    for runtime_id in 40..=41 {
        registry
            .register(runtime_id, [Aabb::new(Vec3::ZERO, Vec3::ONE)])
            .unwrap();
    }
    let hit = PaletteWorld::new(&store, &registry, 0)
        .block_interaction_ray(
            Vec3::new(0.9, 0.7, 0.5),
            Vec3::new(f64::from_bits(0.1_f64.to_bits() + 64), 0.3, 0.0),
            2.0,
            &expected,
        )
        .unwrap()
        .unwrap();
    assert_eq!((hit.block_pos, hit.runtime_id), ([1, 0, 0], 40));
}

#[test]
fn compound_partial_shapes_choose_the_nearest_intercept() {
    let (mut store, chunks) = loaded_air_store(ChunkKey::new(0, 0, 0));
    set_block(&mut store, [0, 0, 0], 0, 9);
    let expected = expected(&store, &chunks);
    let mut registry = CollisionRegistry::with_identity(identity());
    registry.register(0, []).unwrap();
    registry
        .register(
            9,
            [
                Aabb::new(Vec3::new(0.25, 0.0, 0.75), Vec3::new(0.75, 1.0, 1.0)),
                Aabb::new(Vec3::new(0.25, 0.0, 0.25), Vec3::new(0.75, 1.0, 0.5)),
            ],
        )
        .unwrap();
    let hit = PaletteWorld::new(&store, &registry, 0)
        .block_interaction_ray(
            Vec3::new(0.5, 0.5, -1.0),
            Vec3::new(0.0, 0.0, 1.0),
            3.0,
            &expected,
        )
        .unwrap()
        .unwrap();
    assert_eq!(hit.runtime_id, 9);
    assert_eq!(hit.face, 2);
    assert_eq!(hit.distance, 1.25);
    assert_eq!(hit.hit_local, Vec3::new(0.5, 0.5, 0.25));
}

#[test]
fn overhanging_shapes_are_found_from_the_adjacent_voxel() {
    let (mut store, chunks) = loaded_air_store(ChunkKey::new(0, 0, 0));
    set_block(&mut store, [1, 0, 0], 0, 9);
    let expected = expected(&store, &chunks);
    let mut registry = CollisionRegistry::with_identity(identity());
    registry.register(0, []).unwrap();
    registry
        .register(
            9,
            [Aabb::new(
                Vec3::new(-0.5, 0.0, 0.0),
                Vec3::new(0.5, 1.0, 1.0),
            )],
        )
        .unwrap();
    let hit = PaletteWorld::new(&store, &registry, 0)
        .block_interaction_ray(
            Vec3::new(-1.0, 0.5, 0.5),
            Vec3::new(1.0, 0.0, 0.0),
            3.0,
            &expected,
        )
        .unwrap()
        .unwrap();
    assert_eq!(hit.block_pos, [1, 0, 0]);
    assert_eq!(hit.runtime_id, 9);
    assert_eq!(hit.distance, 1.5);
    assert_eq!(hit.hit_local, Vec3::new(0.0, 0.5, 0.5));
}

#[test]
fn surface_contacts_hit_only_when_the_ray_enters_shape_interior() {
    let (mut store, chunks) = loaded_air_store(ChunkKey::new(0, 0, 0));
    set_block(&mut store, [0, 0, 0], 0, 7);
    let expected_identity = expected(&store, &chunks);
    let mut registry = CollisionRegistry::with_identity(identity());
    registry.register(0, []).unwrap();
    registry
        .register(7, [Aabb::new(Vec3::ZERO, Vec3::ONE)])
        .unwrap();
    registry
        .register(
            99,
            [Aabb::new(
                Vec3::new(-0.25, 0.0, 0.0),
                Vec3::new(1.25, 1.0, 1.0),
            )],
        )
        .unwrap();
    let world = PaletteWorld::new(&store, &registry, 0);

    for (origin, direction, expected_block, face) in [
        (
            Vec3::new(0.0, 0.5, 0.5),
            Vec3::new(1.0, 0.0, 0.0),
            [0, 0, 0],
            4,
        ),
        (
            Vec3::new(1.0, 0.5, 0.5),
            Vec3::new(-1.0, 0.0, 0.0),
            [0, 0, 0],
            5,
        ),
    ] {
        let hit = world
            .block_interaction_ray(origin, direction, 1.0, &expected_identity)
            .unwrap()
            .unwrap();
        assert_eq!(
            (hit.block_pos, hit.face, hit.distance),
            (expected_block, face, 0.0)
        );
    }
    for (origin, direction) in [
        (Vec3::new(0.0, 0.5, 0.5), Vec3::new(-1.0, 0.0, 0.0)),
        (Vec3::new(1.0, 0.5, 0.5), Vec3::new(1.0, 0.0, 0.0)),
        (Vec3::new(0.0, 0.5, 0.5), Vec3::new(0.0, 0.0, 1.0)),
    ] {
        assert!(
            world
                .block_interaction_ray(origin, direction, 0.25, &expected_identity)
                .unwrap()
                .is_none()
        );
    }

    let (mut behind_store, behind_chunks) = loaded_air_store(ChunkKey::new(0, 0, 0));
    set_block(&mut behind_store, [-1, 0, 0], 0, 8);
    let behind_expected = expected(&behind_store, &behind_chunks);
    registry
        .register(8, [Aabb::new(Vec3::ZERO, Vec3::ONE)])
        .unwrap();
    assert!(
        PaletteWorld::new(&behind_store, &registry, 0)
            .block_interaction_ray(
                Vec3::new(0.0, 0.5, 0.5),
                Vec3::new(1.0, 0.0, 0.0),
                0.25,
                &behind_expected,
            )
            .unwrap()
            .is_none()
    );
}

#[test]
fn degenerate_registered_boxes_do_not_become_interaction_occluders() {
    let (mut store, chunks) = loaded_air_store(ChunkKey::new(0, 0, 0));
    set_block(&mut store, [0, 0, 0], 0, 9);
    let expected = expected(&store, &chunks);
    let mut registry = CollisionRegistry::with_identity(identity());
    registry.register(0, []).unwrap();
    registry
        .register(
            9,
            [Aabb::new(
                Vec3::new(0.0, 0.0, 0.5),
                Vec3::new(1.0, 1.0, 0.5),
            )],
        )
        .unwrap();
    assert!(
        PaletteWorld::new(&store, &registry, 0)
            .block_interaction_ray(
                Vec3::new(0.5, 0.5, -1.0),
                Vec3::new(0.0, 0.0, 1.0),
                3.0,
                &expected,
            )
            .unwrap()
            .is_none()
    );
}

#[test]
fn nearest_block_occludes_farther_shapes_and_layers_report_the_shape_owner() {
    let (mut store, chunks) = loaded_air_store(ChunkKey::new(0, 0, 0));
    set_block(&mut store, [0, 0, 0], 0, 10);
    set_block(&mut store, [0, 0, 0], 1, 11);
    set_block(&mut store, [0, 0, 2], 0, 12);
    let expected = expected(&store, &chunks);
    let mut registry = CollisionRegistry::with_identity(identity());
    registry.register(0, []).unwrap();
    registry
        .register(10, [Aabb::new(Vec3::new(0.0, 0.0, 0.75), Vec3::ONE)])
        .unwrap();
    registry
        .register(11, [Aabb::new(Vec3::ZERO, Vec3::new(1.0, 1.0, 0.25))])
        .unwrap();
    registry
        .register(12, [Aabb::new(Vec3::ZERO, Vec3::ONE)])
        .unwrap();
    let hit = PaletteWorld::new(&store, &registry, 0)
        .block_interaction_ray(
            Vec3::new(0.5, 0.5, -1.0),
            Vec3::new(0.0, 0.0, 1.0),
            8.0,
            &expected,
        )
        .unwrap()
        .unwrap();
    assert_eq!(
        (hit.block_pos, hit.runtime_id, hit.distance),
        ([0, 0, 0], 11, 1.0)
    );
}

#[test]
fn origin_inside_shape_hits_at_zero_with_a_stable_entry_face() {
    let (store, registry, expected) = fixture(7);
    let hit = PaletteWorld::new(&store, &registry, 0)
        .block_interaction_ray(
            Vec3::new(0.25, 0.5, 0.75),
            Vec3::new(2.0, 1.0, 0.0),
            1.0,
            &expected,
        )
        .unwrap()
        .unwrap();
    assert_eq!(hit.distance, 0.0);
    assert_eq!(hit.face, 4);
    assert_eq!(hit.hit_local, Vec3::new(0.25, 0.5, 0.75));
}

#[test]
fn negative_coordinates_and_direction_scale_preserve_the_hit() {
    let target = [-17, -1, -17];
    let (mut store, chunks) = loaded_air_store(ChunkKey::new(0, -2, -2));
    set_block(&mut store, target, 0, 7);
    let expected = expected(&store, &chunks);
    let mut registry = CollisionRegistry::with_identity(identity());
    registry.register(0, []).unwrap();
    registry
        .register(7, [Aabb::new(Vec3::ZERO, Vec3::ONE)])
        .unwrap();
    let world = PaletteWorld::new(&store, &registry, 0);
    for scale in [f64::MIN_POSITIVE, 0.25, 1.0, 1.0e200] {
        let hit = world
            .block_interaction_ray(
                Vec3::new(-16.5, -0.5, -14.0),
                Vec3::new(0.0, 0.0, -scale),
                4.0,
                &expected,
            )
            .unwrap()
            .unwrap();
        assert_eq!(hit.block_pos, target);
        assert_eq!(hit.face, 3);
        assert_eq!(hit.distance, 2.0);
    }
}

#[test]
fn max_distance_is_inclusive_and_respects_adjacent_f64_values() {
    let (store, registry, expected) = fixture(7);
    let world = PaletteWorld::new(&store, &registry, 0);
    let origin = Vec3::new(0.5, 0.5, -1.0);
    let direction = Vec3::new(0.0, 0.0, 1.0);
    assert!(
        world
            .block_interaction_ray(
                origin,
                direction,
                f64::from_bits(1.0_f64.to_bits() - 1),
                &expected
            )
            .unwrap()
            .is_none()
    );
    for limit in [1.0, f64::from_bits(1.0_f64.to_bits() + 1)] {
        assert_eq!(
            world
                .block_interaction_ray(origin, direction, limit, &expected)
                .unwrap()
                .unwrap()
                .distance,
            1.0
        );
    }
}

#[test]
fn registry_chunk_and_runtime_ambiguity_fail_closed() {
    let (mut store, chunks) = loaded_air_store(ChunkKey::new(0, 0, 0));
    set_block(&mut store, [0, 0, 0], 0, 99);
    let expected = expected(&store, &chunks);
    let mut registry = CollisionRegistry::with_identity(identity());
    registry.register(0, []).unwrap();
    let world = PaletteWorld::new(&store, &registry, 0);
    let query = || {
        world.block_interaction_ray(
            Vec3::new(0.5, 0.5, -1.0),
            Vec3::new(0.0, 0.0, 1.0),
            2.0,
            &expected,
        )
    };
    assert_eq!(
        query(),
        Err(WorldQueryError::UnknownRuntimeId {
            runtime_id: 99,
            block: [0, 0, 0],
        })
    );

    let wrong_registry = WorldCollisionIdentity::new(
        CollisionRegistryIdentity {
            preg_sha256: [1; 32],
            ..identity()
        },
        expected.chunks.iter().copied(),
    )
    .unwrap();
    assert_eq!(
        world.block_interaction_ray(Vec3::ZERO, Vec3::ONE, 1.0, &wrong_registry),
        Err(WorldQueryError::RegistryIdentityMismatch)
    );
}

#[test]
fn stale_identity_and_unloaded_data_before_a_candidate_are_rejected() {
    let (mut store, registry, expected) = fixture(7);
    set_block(&mut store, [0, 0, 0], 0, 0);
    assert!(matches!(
        PaletteWorld::new(&store, &registry, 0).block_interaction_ray(
            Vec3::new(0.5, 0.5, -1.0),
            Vec3::new(0.0, 0.0, 1.0),
            2.0,
            &expected,
        ),
        Err(WorldQueryError::StaleCollisionIdentity { .. })
    ));

    let chunk = ChunkKey::new(0, 0, 0);
    let mut partial = ChunkStore::new();
    partial.apply_level_chunk(chunk, 0, 1, &uniform(0)).unwrap();
    let expected =
        WorldCollisionIdentity::new(identity(), [partial.collision_revision(chunk).unwrap()])
            .unwrap();
    let mut air = CollisionRegistry::with_identity(identity());
    air.register(0, []).unwrap();
    assert_eq!(
        PaletteWorld::new(&partial, &air, 0).block_interaction_ray(
            Vec3::new(15.5, 0.5, 0.5),
            Vec3::new(1.0, 0.0, 0.0),
            2.0,
            &expected,
        ),
        Err(WorldQueryError::UnloadedChunk(ChunkKey::new(0, 1, 0)))
    );
}

#[test]
fn nonfinite_zero_oversized_and_extreme_inputs_are_rejected_without_scanning() {
    let store = ChunkStore::new();
    let registry = CollisionRegistry::with_identity(identity());
    let expected = WorldCollisionIdentity::new(identity(), []).unwrap();
    let world = PaletteWorld::new(&store, &registry, 0);
    for origin in [
        Vec3::new(f64::NAN, 0.0, 0.0),
        Vec3::new(f64::INFINITY, 0.0, 0.0),
        Vec3::new(f64::from(i32::MAX), 0.0, 0.0),
        Vec3::new(f64::from(i32::MIN), 0.0, 0.0),
    ] {
        assert_eq!(
            world.block_interaction_ray(origin, Vec3::ONE, 1.0, &expected),
            Err(WorldQueryError::InvalidRayOrigin)
        );
    }
    for direction in [
        Vec3::ZERO,
        Vec3::new(f64::NAN, 0.0, 0.0),
        Vec3::new(f64::INFINITY, 0.0, 0.0),
    ] {
        assert_eq!(
            world.block_interaction_ray(Vec3::ZERO, direction, 1.0, &expected),
            Err(WorldQueryError::InvalidRayDirection)
        );
    }
    let tiny = Vec3::new(f64::from_bits(1), 0.0, 0.0);
    assert_eq!(
        world.block_interaction_ray(Vec3::ZERO, tiny, 1.0, &expected),
        Err(WorldQueryError::UnloadedChunk(ChunkKey::new(0, 0, 0)))
    );
    for distance in [
        0.0,
        -1.0,
        f64::NAN,
        f64::INFINITY,
        f64::from_bits(sim::MAX_COLLISION_QUERY_EXTENT.to_bits() + 1),
    ] {
        assert_eq!(
            world.block_interaction_ray(Vec3::ZERO, Vec3::ONE, distance, &expected),
            Err(WorldQueryError::InvalidRayDistance)
        );
    }
}

#[test]
fn maximum_distance_full_halo_query_stays_within_the_inspected_block_budget() {
    let mut store = ChunkStore::new();
    let chunks = (-1..=9)
        .flat_map(|x| (-1..=1).map(move |z| ChunkKey::new(0, x, z)))
        .collect::<Vec<_>>();
    for &chunk in &chunks {
        for y in -1..=1 {
            let key = SubChunkKey::from_chunk(chunk, y);
            store.apply_request_mode_air(key).unwrap();
            store.mark_sub_chunk_loaded(key).unwrap();
        }
    }
    let expected = expected(&store, &chunks);
    let mut registry = CollisionRegistry::with_identity(identity());
    registry.register(0, []).unwrap();
    registry
        .register(
            99,
            [Aabb::new(
                Vec3::new(-0.25, -0.25, -0.25),
                Vec3::new(1.25, 1.25, 1.25),
            )],
        )
        .unwrap();
    assert!(
        PaletteWorld::new(&store, &registry, 0)
            .block_interaction_ray(
                Vec3::new(0.5, 0.5, 0.5),
                Vec3::new(1.0, 0.0, 0.0),
                sim::MAX_COLLISION_QUERY_EXTENT,
                &expected,
            )
            .unwrap()
            .is_none()
    );
}
