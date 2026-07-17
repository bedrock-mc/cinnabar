use sim::{
    Aabb, CollisionRegistry, CollisionWorld, MovementInput, PaletteWorld, PlayerState,
    RegistryError, Simulator, Vec3, WorldQueryError,
};
use world::{BlockUpdate, ChunkKey, ChunkStore, SubChunkKey};

fn registry_identity() -> sim::CollisionRegistryIdentity {
    sim::CollisionRegistryIdentity {
        protocol: 1001,
        id_space: sim::CollisionIdSpace::Sequential,
        preg_sha256: [7; 32],
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

fn loaded_uniform_store(chunk: ChunkKey, runtime_id: u32) -> ChunkStore {
    let mut store = ChunkStore::new();
    let zero_storage = [9, 0, 0];
    for x in (chunk.x - 1)..=(chunk.x + 1) {
        for z in (chunk.z - 1)..=(chunk.z + 1) {
            store
                .apply_level_chunk(ChunkKey::new(chunk.dimension, x, z), 0, 1, &zero_storage)
                .unwrap();
        }
    }
    store
        .apply_level_chunk(chunk, 0, 1, &uniform(runtime_id))
        .unwrap();
    store
}

#[test]
fn palette_adapter_preserves_negative_floor_chunk_and_local_coordinates() {
    let chunk = ChunkKey::new(0, -1, 0);
    let store = loaded_uniform_store(chunk, 7);
    let mut registry = CollisionRegistry::new();
    registry.register(0, []).unwrap();
    registry
        .register(7, [Aabb::new(Vec3::ZERO, Vec3::ONE)])
        .unwrap();
    let world = PaletteWorld::new(&store, &registry, 0);

    let boxes = world
        .collision_boxes(Aabb::new(
            Vec3::new(-1.0, 0.0, 0.0),
            Vec3::new(0.0, 1.0, 1.0),
        ))
        .unwrap();
    assert_eq!(
        boxes.value,
        vec![Aabb::new(
            Vec3::new(-1.0, 0.0, 0.0),
            Vec3::new(0.0, 1.0, 1.0)
        )]
    );
}

#[test]
fn palette_adapter_translates_exact_compound_runtime_shapes() {
    let chunk = ChunkKey::new(0, 0, 0);
    let store = loaded_uniform_store(chunk, 9);
    let mut registry = CollisionRegistry::new();
    registry.register(0, []).unwrap();
    registry
        .register(
            9,
            [
                Aabb::new(Vec3::ZERO, Vec3::new(1.0, 0.5, 1.0)),
                Aabb::new(Vec3::new(0.375, 0.5, 0.375), Vec3::new(0.625, 1.5, 0.625)),
            ],
        )
        .unwrap();
    let world = PaletteWorld::new(&store, &registry, 0);

    let boxes = world
        .collision_boxes(Aabb::new(
            Vec3::new(2.0, 0.0, 3.0),
            Vec3::new(3.0, 0.9, 4.0),
        ))
        .unwrap();
    assert_eq!(boxes.value.len(), 2);
    assert!(boxes.value.contains(&Aabb::new(
        Vec3::new(2.0, 0.0, 3.0),
        Vec3::new(3.0, 0.5, 4.0)
    )));
    assert!(boxes.value.contains(&Aabb::new(
        Vec3::new(2.375, 0.5, 3.375),
        Vec3::new(2.625, 1.5, 3.625)
    )));
}

#[test]
fn unknown_runtime_ids_fail_closed_instead_of_becoming_full_cubes_or_air() {
    let chunk = ChunkKey::new(0, 0, 0);
    let store = loaded_uniform_store(chunk, 77);
    let mut registry = CollisionRegistry::new();
    registry.register(0, []).unwrap();
    let world = PaletteWorld::new(&store, &registry, 0);

    assert_eq!(
        world.collision_boxes(Aabb::new(Vec3::ZERO, Vec3::ONE)),
        Err(WorldQueryError::UnknownRuntimeId {
            runtime_id: 77,
            block: [0, 0, 0],
        })
    );
}

#[test]
fn absent_column_fails_closed_even_though_sparse_lookup_looks_like_air() {
    let store = ChunkStore::new();
    let registry = CollisionRegistry::new();
    let world = PaletteWorld::new(&store, &registry, 2);

    assert_eq!(
        world.collision_boxes(Aabb::new(Vec3::ZERO, Vec3::ONE)),
        Err(WorldQueryError::UnloadedChunk(ChunkKey::new(2, -1, -1)))
    );
}

#[test]
fn palette_adapter_reads_runtime_specific_surface_friction_without_flattening() {
    let chunk = ChunkKey::new(0, 0, 0);
    let store = loaded_uniform_store(chunk, 9);
    let mut registry = CollisionRegistry::new();
    registry
        .register_with_friction(9, [Aabb::new(Vec3::ZERO, Vec3::ONE)], 0.98)
        .unwrap();
    let world = PaletteWorld::new(&store, &registry, 0);

    assert_eq!(
        world.block_physics([2, 0, 3]).unwrap().primary().friction,
        0.98
    );
    assert_eq!(
        registry.register_with_friction(10, [], f64::NAN),
        Err(RegistryError::InvalidScalar {
            runtime_id: 10,
            field: "friction",
        })
    );
}

#[test]
fn deterministic_tick_collides_directly_against_palette_packed_store() {
    let chunk = ChunkKey::new(0, 0, 0);
    let mut store = loaded_uniform_store(chunk, 0);
    let floor_key = SubChunkKey::from_chunk(chunk, 0);
    let floor = (0_u8..16)
        .flat_map(|x| (0_u8..16).map(move |z| BlockUpdate::new(x, 0, z, 0, 1)))
        .collect::<Vec<_>>();
    store.update_sub_chunk_blocks(floor_key, &floor, 0).unwrap();

    let mut registry = CollisionRegistry::new();
    registry.register(0, []).unwrap();
    registry
        .register(1, [Aabb::new(Vec3::ZERO, Vec3::ONE)])
        .unwrap();
    let palette_world = PaletteWorld::new(&store, &registry, 0);
    let mut state = PlayerState::new(Vec3::new(8.0, 1.0, 8.0));
    state.on_ground = true;

    let result = Simulator::default()
        .tick(
            &mut state,
            MovementInput {
                forward: 1.0,
                ..MovementInput::default()
            },
            &palette_world,
        )
        .unwrap();

    assert_eq!(result.position.y, 1.0);
    assert!(result.on_ground);
    assert!(result.position.z > 8.09);
}

#[test]
fn collision_registry_rejects_non_finite_and_inverted_shapes() {
    let mut registry = CollisionRegistry::new();
    assert_eq!(
        registry.register(
            11,
            [Aabb::new(Vec3::ZERO, Vec3::new(f64::INFINITY, 1.0, 1.0),)],
        ),
        Err(RegistryError::InvalidShape {
            runtime_id: 11,
            shape_index: 0,
        })
    );
    assert_eq!(
        registry.register(12, [Aabb::new(Vec3::new(1.0, 0.0, 0.0), Vec3::ZERO)],),
        Err(RegistryError::InvalidShape {
            runtime_id: 12,
            shape_index: 0,
        })
    );
}

#[test]
fn collision_registry_rejects_shapes_outside_the_one_block_query_halo() {
    let mut registry = CollisionRegistry::new();
    for (runtime_id, shape) in [
        (13, Aabb::new(Vec3::new(-1.000_000_01, 0.0, 0.0), Vec3::ONE)),
        (14, Aabb::new(Vec3::new(0.0, -1.000_000_01, 0.0), Vec3::ONE)),
        (15, Aabb::new(Vec3::new(0.0, 0.0, -1.000_000_01), Vec3::ONE)),
        (16, Aabb::new(Vec3::ZERO, Vec3::new(2.000_000_01, 1.0, 1.0))),
        (17, Aabb::new(Vec3::ZERO, Vec3::new(1.0, 2.000_000_01, 1.0))),
        (18, Aabb::new(Vec3::ZERO, Vec3::new(1.0, 1.0, 2.000_000_01))),
    ] {
        assert_eq!(
            registry.register(runtime_id, [shape]),
            Err(RegistryError::ShapeOutsideLocalHalo {
                runtime_id,
                shape_index: 0,
            })
        );
    }

    registry
        .register(
            19,
            [Aabb::new(
                Vec3::new(-1.0, -1.0, -1.0),
                Vec3::new(2.0, 2.0, 2.0),
            )],
        )
        .unwrap();
}

#[test]
fn palette_adapter_rejects_oversized_queries_before_chunk_scanning() {
    let store = ChunkStore::new();
    let registry = CollisionRegistry::new();
    let world = PaletteWorld::new(&store, &registry, 0);

    let oversized = sim::MAX_COLLISION_QUERY_EXTENT + 1.0;
    for (min, max) in [
        (Vec3::ZERO, Vec3::new(oversized, 1.0, 1.0)),
        (Vec3::new(-oversized, 0.0, 0.0), Vec3::ONE),
        (Vec3::ZERO, Vec3::new(1.0, oversized, 1.0)),
        (Vec3::new(0.0, -oversized, 0.0), Vec3::ONE),
        (Vec3::ZERO, Vec3::new(1.0, 1.0, oversized)),
        (Vec3::new(0.0, 0.0, -oversized), Vec3::ONE),
    ] {
        assert_eq!(
            world.collision_boxes(Aabb::new(min, max)),
            Err(WorldQueryError::QueryExtentExceeded)
        );
    }
}

#[test]
fn identity_is_exact_sorted_and_changes_with_any_queried_column() {
    let chunk = ChunkKey::new(0, 0, 0);
    let mut store = loaded_uniform_store(chunk, 0);
    let mut registry = CollisionRegistry::with_identity(registry_identity());
    registry.register(0, []).unwrap();
    registry.register(1, []).unwrap();
    let before = PaletteWorld::new(&store, &registry, 0)
        .collision_boxes(Aabb::new(
            Vec3::new(0.0, 0.0, 0.0),
            Vec3::new(16.0, 1.0, 1.0),
        ))
        .unwrap();
    assert!(
        before
            .identity
            .chunks
            .windows(2)
            .all(|pair| pair[0].chunk < pair[1].chunk)
    );

    store
        .update_block(
            SubChunkKey::from_chunk(chunk, 0),
            BlockUpdate::new(0, 0, 0, 0, 1),
            99,
        )
        .unwrap();
    let after = PaletteWorld::new(&store, &registry, 0)
        .collision_boxes(Aabb::new(
            Vec3::new(0.0, 0.0, 0.0),
            Vec3::new(16.0, 1.0, 1.0),
        ))
        .unwrap();
    assert_ne!(before.identity, after.identity);
}

#[test]
fn block_physics_returns_full_facts_and_exact_identity() {
    let chunk = ChunkKey::new(0, 0, 0);
    let mut store = loaded_uniform_store(chunk, 9);
    store
        .update_block(
            SubChunkKey::from_chunk(chunk, 0),
            BlockUpdate::new(2, 0, 3, 1, 10),
            0,
        )
        .unwrap();
    let mut registry = CollisionRegistry::with_identity(registry_identity());
    registry
        .register_physics(
            9,
            [Aabb::new(Vec3::ZERO, Vec3::ONE)],
            0.98,
            0.4,
            0.8,
            0.75,
            sim::BlockPhysicsFlags::WATER,
            sim::SurfaceResponse::BubbleUp,
        )
        .unwrap();
    registry
        .register_physics(
            10,
            [],
            0.5,
            0.25,
            0.5,
            0.8,
            sim::BlockPhysicsFlags::WATER,
            sim::SurfaceResponse::None,
        )
        .unwrap();
    let sample = PaletteWorld::new(&store, &registry, 0)
        .block_physics([2, 0, 3])
        .unwrap();
    assert_eq!(sample.layers.len(), 2);
    assert_eq!(sample.layers[0].friction, 0.98);
    assert_eq!(sample.layers[0].horizontal_speed_factor, 0.4);
    assert_eq!(sample.layers[0].vertical_speed_factor, 0.8);
    assert_eq!(sample.layers[0].fluid_height_blocks, 0.75);
    assert_eq!(sample.layers[0].flags, sim::BlockPhysicsFlags::WATER);
    assert_eq!(
        sample.layers[0].surface_response,
        sim::SurfaceResponse::BubbleUp
    );
    assert_eq!(sample.layers[1].fluid_height_blocks, 0.8);
    assert_eq!(sample.identity.registry, registry_identity());
    assert_eq!(sample.identity.chunks.len(), 1);
}

#[test]
fn identity_merge_is_bounded_and_rejects_mixed_registries() {
    let identity = registry_identity();
    let left = sim::WorldCollisionIdentity::new(identity, []).unwrap();
    let other_registry = sim::CollisionRegistryIdentity {
        preg_sha256: [8; 32],
        ..identity
    };
    let right = sim::WorldCollisionIdentity::new(other_registry, []).unwrap();
    assert_eq!(
        left.merge(&right),
        Err(WorldQueryError::RegistryIdentityMismatch)
    );

    let chunks = (0..=sim::MAX_COLLISION_IDENTITY_CHUNKS)
        .map(|x| world::ChunkCollisionRevision {
            chunk: ChunkKey::new(0, x as i32, 0),
            revision: x as u64 + 1,
        })
        .collect::<Vec<_>>();
    assert_eq!(
        sim::WorldCollisionIdentity::new(identity, chunks),
        Err(WorldQueryError::IdentityChunkLimitExceeded {
            max: sim::MAX_COLLISION_IDENTITY_CHUNKS,
        })
    );
}

#[test]
fn identity_limit_stops_before_polling_a_panic_sentinel() {
    let chunks = (0..=sim::MAX_COLLISION_IDENTITY_CHUNKS)
        .map(|x| world::ChunkCollisionRevision {
            chunk: ChunkKey::new(0, x as i32, 0),
            revision: x as u64 + 1,
        })
        .chain(std::iter::once_with(|| {
            panic!("identity iterator over-polled")
        }));
    assert_eq!(
        sim::WorldCollisionIdentity::new(registry_identity(), chunks),
        Err(WorldQueryError::IdentityChunkLimitExceeded {
            max: sim::MAX_COLLISION_IDENTITY_CHUNKS,
        })
    );
}

#[test]
fn primitive_registration_rejects_contradictory_physics_facts() {
    let mut registry = CollisionRegistry::with_identity(registry_identity());
    let full = [Aabb::new(Vec3::ZERO, Vec3::ONE)];
    let flags =
        |values: &[sim::BlockPhysicsFlags]| values.iter().fold(0, |bits, flag| bits | flag.bits());
    for (runtime_id, boxes, fluid_height, block_flags, response) in [
        (
            20,
            &[][..],
            1.0,
            flags(&[sim::BlockPhysicsFlags::WATER, sim::BlockPhysicsFlags::LAVA]),
            sim::SurfaceResponse::None as u8,
        ),
        (21, &[][..], 1.0, 0, sim::SurfaceResponse::None as u8),
        (22, &[][..], 0.0, 0, sim::SurfaceResponse::BubbleUp as u8),
        (
            23,
            &full[..],
            1.0,
            flags(&[
                sim::BlockPhysicsFlags::WATER,
                sim::BlockPhysicsFlags::PASSABLE,
            ]),
            sim::SurfaceResponse::None as u8,
        ),
    ] {
        assert!(matches!(
            registry.register_primitives(
                runtime_id,
                boxes.iter().copied(),
                0.6,
                1.0,
                1.0,
                fluid_height,
                block_flags,
                response,
            ),
            Err(RegistryError::ContradictoryFacts { runtime_id: rejected }) if rejected == runtime_id
        ));
    }
}
