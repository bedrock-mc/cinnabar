use sim::{
    Aabb, CollisionRegistry, CollisionWorld, MovementInput, PaletteWorld, PlayerState,
    RegistryError, Simulator, Vec3, WorldQueryError,
};
use world::{BlockUpdate, ChunkKey, ChunkStore, SubChunkKey};

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
        boxes,
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
    assert_eq!(boxes.len(), 2);
    assert!(boxes.contains(&Aabb::new(
        Vec3::new(2.0, 0.0, 3.0),
        Vec3::new(3.0, 0.5, 4.0)
    )));
    assert!(boxes.contains(&Aabb::new(
        Vec3::new(2.375, 0.5, 3.375),
        Vec3::new(2.625, 1.5, 3.625)
    )));
}

#[test]
fn unknown_runtime_ids_fail_closed_instead_of_becoming_full_cubes_or_air() {
    let chunk = ChunkKey::new(0, 0, 0);
    let store = loaded_uniform_store(chunk, 77);
    let registry = CollisionRegistry::new();
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

    assert_eq!(world.block_friction([2, 0, 3]).unwrap(), 0.98);
    assert_eq!(
        registry.register_with_friction(10, [], f64::NAN),
        Err(RegistryError::InvalidFriction { runtime_id: 10 })
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
