use sim::{Aabb, CollisionRegistry, CollisionWorld, DoorFacing, DoorState, PaletteWorld, Vec3};
use world::{BlockUpdate, ChunkStore, SubChunkKey};

/// Registers one door half while preserving an intentionally wrong carrier box.
fn half(registry: &mut CollisionRegistry, id: u32, upper: bool, hinge_right: bool) {
    registry
        .register(id, [Aabb::new(Vec3::ZERO, Vec3::ONE)])
        .unwrap();
    assert!(registry.set_door_state(
        id,
        DoorState {
            family: "minecraft:oak_door".into(),
            facing: if upper {
                DoorFacing::North
            } else {
                DoorFacing::South
            },
            upper,
            open: !upper,
            hinge_right,
        }
    ));
}

/// Produces a loaded interior cell with two registry-backed door halves.
fn fixture() -> (CollisionRegistry, ChunkStore) {
    let mut registry = CollisionRegistry::new();
    registry.register(0, []).unwrap();
    half(&mut registry, 1, false, false);
    half(&mut registry, 2, true, true);
    half(&mut registry, 3, true, false);
    let mut store = ChunkStore::new();
    let key = SubChunkKey::new(0, 0, 0, 0);
    store.mark_sub_chunk_loaded(key).unwrap();
    for (y, id) in [(8, 1), (9, 2)] {
        store
            .update_block(key, BlockUpdate::new(8, y, 8, 0, id), 0)
            .unwrap();
    }
    (registry, store)
}

#[test]
fn both_halves_use_lower_open_state_and_upper_hinge() {
    let (registry, mut store) = fixture();
    let query = Aabb::new(Vec3::new(8.0, 8.0, 8.0), Vec3::new(9.0, 10.0, 9.0));
    let before = PaletteWorld::new(&store, &registry, 0)
        .collision_boxes(query)
        .unwrap();
    assert_eq!(before.value.len(), 2);
    for shape in before.value {
        assert_eq!(
            shape.min.z,
            f64::from(8.0_f32 + f32::from_bits(0x3f51_47ae))
        );
        assert_eq!(shape.max.z, 9.0);
    }
    store
        .update_block(
            SubChunkKey::new(0, 0, 0, 0),
            BlockUpdate::new(8, 9, 8, 0, 3),
            0,
        )
        .unwrap();
    let after = PaletteWorld::new(&store, &registry, 0)
        .collision_boxes(query)
        .unwrap();
    assert_ne!(before.identity, after.identity);
    for shape in after.value {
        assert_eq!(shape.min.z, 8.0);
        assert_eq!(
            shape.max.z,
            f64::from(8.0_f32 + f32::from_bits(0x3e3a_e148))
        );
    }
}

#[test]
fn unpaired_door_uses_native_default_plane() {
    let (registry, mut store) = fixture();
    store
        .update_block(
            SubChunkKey::new(0, 0, 0, 0),
            BlockUpdate::new(8, 9, 8, 0, 0),
            0,
        )
        .unwrap();
    let result = PaletteWorld::new(&store, &registry, 0)
        .collision_boxes(Aabb::new(
            Vec3::new(8.0, 8.0, 8.0),
            Vec3::new(9.0, 9.0, 9.0),
        ))
        .unwrap();
    assert_eq!(result.value.len(), 1);
    assert_eq!(
        result.value[0].max.x,
        f64::from(8.0_f32 + f32::from_bits(0x3e3a_e148))
    );
    assert_eq!(result.value[0].max.z, 9.0);
}

#[test]
fn unrelated_upper_door_does_not_require_unloaded_lower_half() {
    let (registry, _) = fixture();
    let mut store = ChunkStore::new();
    let key = SubChunkKey::new(0, 0, 1, 0);
    store.mark_sub_chunk_loaded(key).unwrap();
    store
        .update_block(key, BlockUpdate::new(8, 0, 8, 0, 2), 0)
        .unwrap();
    let world = PaletteWorld::new(&store, &registry, 0);
    let result = world
        .collision_boxes(Aabb::new(
            Vec3::new(8.1, 17.9, 8.1),
            Vec3::new(8.9, 19.8, 8.9),
        ))
        .unwrap();
    assert!(result.value.is_empty());
    assert!(
        world
            .collision_boxes(Aabb::new(
                Vec3::new(8.1, 16.5, 8.1),
                Vec3::new(8.9, 17.1, 8.9),
            ))
            .is_err()
    );
}

#[test]
fn review_door_pick_ray_observes_the_paired_hinge() {
    let (registry, mut store) = fixture();
    let origin = Vec3::new(7.0, 8.5, 8.9);
    let direction = Vec3::new(1.0, 0.0, 0.0);
    assert!(
        PaletteWorld::new(&store, &registry, 0)
            .block_interaction_ray_current(origin, direction, 3.0)
            .unwrap()
            .is_some()
    );
    store
        .update_block(
            SubChunkKey::new(0, 0, 0, 0),
            BlockUpdate::new(8, 9, 8, 0, 3),
            0,
        )
        .unwrap();
    assert!(
        PaletteWorld::new(&store, &registry, 0)
            .block_interaction_ray_current(origin, direction, 3.0)
            .unwrap()
            .is_none()
    );
}
