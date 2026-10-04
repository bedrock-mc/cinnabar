/// Connection states in the real registry must reach both runtime-ID maps.
#[test]
fn registry_connections_extend_fence_and_pane_colliders() {
    let breg = include_bytes!("../../../../assets/data/block-registry-v2193.bin");
    let protocol = active_content_registry_protocol();
    let records = read_registry_for_protocol(breg, protocol).unwrap();
    let preg = synthetic_preg(breg, &records);
    let registries = PhysicsCollisionRegistries::from_assets(breg, &records, &preg, protocol).unwrap();
    for (name, inset, height, end) in [
        ("minecraft:oak_fence", 0.375, 1.5, 0.625),
        ("minecraft:glass_pane", 0.4375, 1.0, 0.5),
        ("minecraft:iron_bars", 0.4375, 1.0, 0.5),
    ] {
        let record = records.iter().find(|record| {
            if record.name.as_ref() != name { return false; }
            let state: serde_json::Value = serde_json::from_str(&record.canonical_state).unwrap();
            state["minecraft:connection_north"]["value"] == 1
                && ["south", "east", "west"].into_iter().all(|direction| {
                    state[format!("minecraft:connection_{direction}")]["value"] == 0
                })
        }).unwrap();
        let expected = [Aabb::new(Vec3::new(inset, 0.0, 0.0), Vec3::new(1.0 - inset, height, end))];
        for (mode, id) in [(NetworkIdMode::Sequential, record.sequential_id), (NetworkIdMode::Hashed, record.network_hash)] {
            assert_eq!(registries.registry(mode).collision_shapes(id).unwrap(), expected, "{name} {mode:?}");
            let air = records.iter().find(|record| record.name.as_ref() == "minecraft:air").unwrap();
            let air_id = match mode { NetworkIdMode::Sequential => air.sequential_id, NetworkIdMode::Hashed => air.network_hash };
            let mut store = world::ChunkStore::new();
            let sub_chunk = world::SubChunkKey::new(0, 0, 0, 0);
            store.mark_sub_chunk_loaded(sub_chunk).unwrap();
            store.update_block(sub_chunk, world::BlockUpdate::new(8, 8, 8, 0, id), air_id).unwrap();
            let world = sim::PaletteWorld::new(&store, registries.registry(mode), 0);
            let arm = world.collision_boxes(Aabb::new(Vec3::new(8.45, 8.0, 8.01), Vec3::new(8.55, 9.0, 8.1))).unwrap();
            assert_eq!(arm.value, [expected[0].translated(Vec3::new(8.0, 8.0, 8.0))]);
        }
    }
}

/// Real registry door properties must pair in both palette identity modes.
#[test]
fn registry_door_halves_share_the_lower_open_state_and_upper_hinge() {
    let breg = include_bytes!("../../../../assets/data/block-registry-v2193.bin");
    let protocol = active_content_registry_protocol();
    let records = read_registry_for_protocol(breg, protocol).unwrap();
    let registries = PhysicsCollisionRegistries::from_assets(breg, &records, &synthetic_preg(breg, &records), protocol).unwrap();
    let halves = [false, true].map(|upper| records.iter().find(|record| {
        if record.name.as_ref() != "minecraft:warped_door" { return false; }
        let state: serde_json::Value = serde_json::from_str(&record.canonical_state).unwrap();
        state["upper_block_bit"]["value"] == i32::from(upper)
            && state["open_bit"]["value"] == i32::from(!upper)
            && state["door_hinge_bit"]["value"] == i32::from(upper)
            && state["minecraft:cardinal_direction"]["value"] == if upper { "north" } else { "south" }
    }).unwrap());
    let air = records.iter().find(|record| record.name.as_ref() == "minecraft:air").unwrap();
    for mode in [NetworkIdMode::Sequential, NetworkIdMode::Hashed] {
        let id = |record: &assets::RegistryRecord| match mode {
            NetworkIdMode::Sequential => record.sequential_id,
            NetworkIdMode::Hashed => record.network_hash,
        };
        let mut store = world::ChunkStore::new();
        let key = world::SubChunkKey::new(0, 0, 0, 0);
        store.mark_sub_chunk_loaded(key).unwrap();
        for (half, y) in halves.into_iter().zip([8, 9]) {
            store.update_block(key, world::BlockUpdate::new(8, y, 8, 0, id(half)), id(air)).unwrap();
        }
        let world = sim::PaletteWorld::new(&store, registries.registry(mode), 0);
        let boxes = world.collision_boxes(Aabb::new(Vec3::new(8.0, 8.0, 8.0), Vec3::new(9.0, 10.0, 9.0))).unwrap();
        assert_eq!(boxes.value.len(), 2);
        assert!(boxes.value.iter().all(|shape| shape.min.z == f64::from(8.0_f32 + f32::from_bits(0x3f51_47ae)) && shape.max.z == 9.0));
    }
}

/// All registry stair corners preserve the native occupied quadrants in both ID spaces.
#[test]
fn registry_stair_corners_and_halves_have_native_piece_geometry() {
    let breg = include_bytes!("../../../../assets/data/block-registry-v2193.bin");
    let protocol = active_content_registry_protocol();
    let records = read_registry_for_protocol(breg, protocol).unwrap();
    let registries = PhysicsCollisionRegistries::from_assets(breg, &records, &synthetic_preg(breg, &records), protocol).unwrap();
    // Step and inner-piece bits are x + 2*z quadrants.
    let quadrants = [[10, 11, 14, 2, 8], [5, 13, 7, 4, 1], [12, 14, 13, 8, 4], [3, 7, 11, 1, 2]];
    let mut states = 0;
    for record in records.iter().filter(|record| record.name.as_ref() == "minecraft:oak_stairs") {
        let state: serde_json::Value = serde_json::from_str(&record.canonical_state).unwrap();
        let direction = state["weirdo_direction"]["value"].as_u64().unwrap() as usize;
        let inverted = state["upside_down_bit"]["value"] == 1;
        let corner = ["none", "inner_left", "inner_right", "outer_left", "outer_right"].iter().position(|name| state["minecraft:corner"]["value"] == *name).unwrap();
        for (mode, id) in [(NetworkIdMode::Sequential, record.sequential_id), (NetworkIdMode::Hashed, record.network_hash)] {
            let boxes = registries.registry(mode).collision_shapes(id).unwrap();
            assert_eq!(boxes.len(), if corner == 1 || corner == 2 { 3 } else { 2 });
            for y in 0..2 {
                for quadrant in 0..4 {
                    let point = Vec3::new(0.25 + f64::from(quadrant & 1) * 0.5, 0.25 + f64::from(y) * 0.5, 0.25 + f64::from(quadrant >> 1) * 0.5);
                    let occupied = boxes.iter().any(|b| b.min.x < point.x && point.x < b.max.x && b.min.y < point.y && point.y < b.max.y && b.min.z < point.z && point.z < b.max.z);
                    let expected = (y == i32::from(inverted)) || quadrants[direction][corner] & (1 << quadrant) != 0;
                    assert_eq!(occupied, expected, "{} {mode:?} y={y} quadrant={quadrant}", record.canonical_state);
                }
            }
        }
        states += 1;
    }
    assert_eq!(states, 40);
}

/// Stable registry scaffolds expose support cubes; falling stability has no collision.
#[test]
fn registry_scaffold_stability_controls_support_geometry() {
    let breg = include_bytes!("../../../../assets/data/block-registry-v2193.bin");
    let protocol = active_content_registry_protocol();
    let records = read_registry_for_protocol(breg, protocol).unwrap();
    let registries = PhysicsCollisionRegistries::from_assets(breg, &records, &synthetic_preg(breg, &records), protocol).unwrap();
    let mut stability_seen = [false; 8];
    for record in records.iter().filter(|record| record.name.as_ref() == "minecraft:scaffolding") {
        let state: serde_json::Value = serde_json::from_str(&record.canonical_state).unwrap();
        let stability = state["stability"]["value"].as_u64().unwrap() as usize;
        stability_seen[stability] = true;
        for (mode, id) in [(NetworkIdMode::Sequential, record.sequential_id), (NetworkIdMode::Hashed, record.network_hash)] {
            let boxes = registries.registry(mode).collision_shapes(id).unwrap();
            if stability == 7 { assert!(boxes.is_empty()); }
            else { assert_eq!(boxes, [Aabb::new(Vec3::ZERO, Vec3::ONE)]); }
        }
    }
    assert!(stability_seen.into_iter().all(|seen| seen));
}
