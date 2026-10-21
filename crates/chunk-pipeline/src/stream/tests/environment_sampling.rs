use super::*;

#[test]
fn mesh_snapshot_bakes_solved_halo_channels_into_cube_sidecars() {
    let key = SubChunkKey::new(0, 3, 4, 5);
    let light = Arc::new(SubChunkLight::uniform(7, 3, 11).unwrap());
    let light_halo = MeshLightHalo {
        center: Some(key),
        slots: std::array::from_fn(|_| {
            Some(MeshLightSlot {
                key,
                block_generation: 10,
                light_revision: 11,
                light: Arc::clone(&light),
            })
        }),
    };
    let snapshot = MeshSnapshot {
        rendering: ::meshing::MeshRenderSettings::default(),
        center: Arc::new(uniform_sub_chunk(1)),
        biomes: std::array::from_fn(|_| None),
        adjacent: std::array::from_fn(|_| None),
        column: None,
        center_y: key.y,
        light_halo,
        java_meshing: None,
    };

    let mesh = snapshot.mesh(
        BlockClassifier::new(0),
        &RuntimeAssets::diagnostic(),
        NetworkIdMode::Sequential,
        &PackedBiomeRecord::fallback(),
    );

    assert!(!mesh.cube_lighting().is_empty());
    assert_eq!(mesh.cube_lighting().len(), mesh.cube_quads().len());
    assert!(
        mesh.cube_lighting()
            .iter()
            .flat_map(|lighting| lighting.samples())
            .all(|sample| sample & 0x00ff == 0x0037)
    );
}

#[test]
fn camera_medium_samples_exposed_and_waterlogged_liquid_layers_without_flattening() {
    let mut stream = WorldStream::new_with_assets(
        WorldBootstrap {
            local_player_unique_id: 1,
            dimension: 0,
            local_player_runtime_id: 1,
            player_position: [0.0; 3],
            world_spawn_position: [0; 3],
            air_network_id: 0,
            block_network_ids_are_hashes: false,
        },
        Arc::new(camera_medium_assets()),
        [0.0, 4.5, 0.0],
        None,
    );
    let key = SubChunkKey {
        dimension: 0,
        x: 0,
        y: 0,
        z: 0,
    };
    stream
        .authority
        .commit_sub_chunk(key, uniform_sub_chunk(0))
        .unwrap();
    stream
        .authority
        .update_block(key, BlockUpdate::new(4, 4, 4, 0, 1), 0)
        .unwrap();

    assert_eq!(
        stream.camera_medium([4.5, 4.5, 4.5]),
        ::meshing::CameraMedium::Water
    );
    assert_eq!(
        stream.camera_medium([4.5, 4.95, 4.5]),
        ::meshing::CameraMedium::Air
    );

    stream
        .authority
        .update_block(key, BlockUpdate::new(4, 4, 4, 0, 3), 0)
        .unwrap();
    stream
        .authority
        .update_block(key, BlockUpdate::new(4, 4, 4, 1, 1), 0)
        .unwrap();
    assert_eq!(
        stream.camera_medium([4.5, 4.5, 4.5]),
        ::meshing::CameraMedium::Water
    );

    stream
        .authority
        .update_block(key, BlockUpdate::new(4, 4, 4, 1, 2), 0)
        .unwrap();
    assert_eq!(
        stream.camera_medium([4.5, 4.5, 4.5]),
        ::meshing::CameraMedium::Lava
    );
    assert_eq!(
        stream.camera_medium([f32::NAN, 4.5, 4.5]),
        ::meshing::CameraMedium::Air
    );
}

#[test]
fn light_level_at_reads_resident_block_and_sky_and_falls_dark_off_boundary() {
    let mut stream = WorldStream::new_with_assets(
        WorldBootstrap {
            local_player_unique_id: 1,
            dimension: 0,
            local_player_runtime_id: 1,
            player_position: [0.0; 3],
            world_spawn_position: [0; 3],
            air_network_id: 0,
            block_network_ids_are_hashes: false,
        },
        Arc::new(RuntimeAssets::diagnostic()),
        [0.0, 4.5, 0.0],
        None,
    );
    let key = SubChunkKey::new(0, 0, 0, 0);
    stream
        .lighting
        .store
        .insert_resident(key, SubChunkLight::uniform(7, 3, 1).unwrap());
    assert_eq!(stream.light_level_at([4.5, 4.5, 4.5]), (7, 3));
    // A sub-chunk whose light is not resident, and a non-finite sample, both read dark.
    assert_eq!(stream.light_level_at([4.5, 40.0, 4.5]), (0, 0));
    assert_eq!(stream.light_level_at([f32::NAN, 4.5, 4.5]), (0, 0));
}

#[test]
fn light_level_at_uses_height_fallback_without_clamping_to_edge_cells() {
    for dimension in [0, 1, 2] {
        let mut stream = WorldStream::new(WorldBootstrap {
            local_player_unique_id: 1,
            dimension,
            local_player_runtime_id: 1,
            player_position: [0.0; 3],
            world_spawn_position: [0; 3],
            air_network_id: 0,
            block_network_ids_are_hashes: false,
        });
        let range = stream.authority.dimension_range(dimension).unwrap();
        let min = range.base_sub_chunk_y * 16;
        let max = min + range.sub_chunk_count as i32 * 16;
        for y in [min.div_euclid(16), (max - 1).div_euclid(16)] {
            stream.lighting.store.insert_resident(
                SubChunkKey::new(dimension, 0, y, 0),
                SubChunkLight::uniform(9, 2, 1).unwrap(),
            );
        }
        let fallback = if dimension == 0 { (0, 15) } else { (0, 0) };
        for y in [min as f32 - 0.01, max as f32, max as f32 + 50.0] {
            assert_eq!(stream.light_level_at([4.5, y, 4.5]), fallback);
            assert_eq!(stream.solved_light_at([4.5, y, 4.5]), Some(fallback));
        }
        for y in [min as f32, max as f32 - 0.01] {
            assert_eq!(stream.light_level_at([4.5, y, 4.5]), (9, 2));
        }
    }
}

#[test]
fn light_level_at_obeys_admitted_height_instead_of_builtin_height() {
    let mut stream = WorldStream::new(WorldBootstrap {
        local_player_unique_id: 1,
        dimension: 0,
        local_player_runtime_id: 1,
        player_position: [0.0; 3],
        world_spawn_position: [0; 3],
        air_network_id: 0,
        block_network_ids_are_hashes: false,
    });
    stream
        .authority
        .apply_dimension_heights(&[protocol::DimensionHeightDiagnostic {
            name: "minecraft:overworld".into(),
            dimension: 0,
            minimum_y: -128,
            maximum_y: 384,
            generator: 0,
        }]);
    for y in [-8, -5, 20, 23, -9, 24] {
        stream.lighting.store.insert_resident(
            SubChunkKey::new(0, 0, y, 0),
            SubChunkLight::uniform(9, 2, 1).unwrap(),
        );
    }
    for y in [-128.0, -65.0, 320.0, 383.0] {
        assert_eq!(stream.light_level_at([4.5, y, 4.5]), (9, 2));
    }
    for y in [-128.01, 384.0] {
        assert_eq!(stream.light_level_at([4.5, y, 4.5]), (0, 15));
    }
}

#[test]
fn camera_environment_context_exposes_palette_biome_and_effective_block_radius() {
    let mut stream = WorldStream::new_with_assets(
        WorldBootstrap {
            local_player_unique_id: 1,
            dimension: 0,
            local_player_runtime_id: 1,
            player_position: [0.0; 3],
            world_spawn_position: [0; 3],
            air_network_id: 0,
            block_network_ids_are_hashes: false,
        },
        Arc::new(RuntimeAssets::diagnostic()),
        [0.0, -63.5, 0.0],
        None,
    );
    stream.authority.commit_biome_column(
        ChunkKey::new(0, 0, 0),
        DecodedBiomeColumn::decode(-4, 1, &[1, 84], &RAW_BIOMES),
    );
    stream.chunk_radius = Some(16);

    assert_eq!(stream.camera_biome_id([4.5, -63.5, 4.5]), Some(42));
    assert_eq!(stream.camera_biome_id([16.5, -63.5, 4.5]), None);
    assert_eq!(stream.camera_biome_id([f32::NAN, -63.5, 4.5]), None);
    assert_eq!(stream.render_distance_blocks(), 256.0);
}
