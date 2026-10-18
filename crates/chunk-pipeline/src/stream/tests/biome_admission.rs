use super::*;

#[test]
fn biome_definition_snapshot_commits_in_fifo_and_survives_dimension_changes() {
    let mut stream = WorldStream::new(WorldBootstrap {
        local_player_unique_id: 1,
        dimension: 0,
        local_player_runtime_id: 1,
        player_position: [0.0; 3],
        world_spawn_position: [0; 3],
        air_network_id: protocol_ids::SEQUENTIAL_AIR_NETWORK_ID,
        block_network_ids_are_hashes: false,
    });
    let definitions: Arc<[BiomeDefinitionEvent]> = Arc::from(vec![BiomeDefinitionEvent {
        biome_id: None,
        name: Arc::from("minecraft:plains"),
        temperature: 0.8,
        downfall: 0.4,
        snow_foliage: 0.0,
        max_snow_accumulation: None,
        map_water_color: 0xff44_6688,
    }]);

    assert!(stream.biome_definitions_snapshot().is_empty());
    stream
        .submit(
            2,
            WorldEvent::BiomeDefinitions(BiomeDefinitionsEvent {
                definitions: Arc::clone(&definitions),
            }),
        )
        .unwrap();
    assert!(
        stream.biome_definitions_snapshot().is_empty(),
        "sequence two must wait for sequence one"
    );

    stream.submit(1, WorldEvent::ChunkRadiusUpdated(8)).unwrap();
    let committed = stream.biome_definitions_snapshot();
    assert!(Arc::ptr_eq(&committed, &definitions));

    stream
        .submit(
            3,
            WorldEvent::ChangeDimension(ChangeDimensionEvent {
                dimension: 1,
                position: [0.0, 64.0, 0.0],
                ..Default::default()
            }),
        )
        .unwrap();
    let after_dimension_change = stream.biome_definitions_snapshot();
    assert!(Arc::ptr_eq(&after_dimension_change, &definitions));
}

#[test]
fn stale_biome_definition_event_cannot_replace_the_committed_snapshot() {
    let mut stream = WorldStream::new(WorldBootstrap {
        local_player_unique_id: 1,
        dimension: 0,
        local_player_runtime_id: 1,
        player_position: [0.0; 3],
        world_spawn_position: [0; 3],
        air_network_id: protocol_ids::SEQUENTIAL_AIR_NETWORK_ID,
        block_network_ids_are_hashes: false,
    });
    let committed: Arc<[BiomeDefinitionEvent]> = Arc::from(vec![BiomeDefinitionEvent {
        biome_id: None,
        name: Arc::from("minecraft:plains"),
        temperature: 0.8,
        downfall: 0.4,
        snow_foliage: 0.0,
        max_snow_accumulation: None,
        map_water_color: 0xff44_6688,
    }]);
    stream
        .submit(
            1,
            WorldEvent::BiomeDefinitions(BiomeDefinitionsEvent {
                definitions: Arc::clone(&committed),
            }),
        )
        .unwrap();

    let stale: Arc<[BiomeDefinitionEvent]> = Arc::from(vec![BiomeDefinitionEvent {
        biome_id: Some(600),
        name: Arc::from("example:stale"),
        temperature: 0.0,
        downfall: 0.0,
        snow_foliage: 0.0,
        max_snow_accumulation: None,
        map_water_color: 0,
    }]);
    let error = stream
        .submit(
            1,
            WorldEvent::BiomeDefinitions(BiomeDefinitionsEvent { definitions: stale }),
        )
        .unwrap_err();

    assert!(matches!(
        error,
        super::WorldStreamError::DuplicateOrPast {
            sequence: 1,
            next: 2
        }
    ));
    assert!(Arc::ptr_eq(
        &stream.biome_definitions_snapshot(),
        &committed
    ));
}

#[test]
fn live_biome_resolution_commits_in_fifo_with_exact_raw_id_lookup() {
    let mut stream = WorldStream::new(WorldBootstrap {
        local_player_unique_id: 1,
        dimension: 0,
        local_player_runtime_id: 1,
        player_position: [0.0; 3],
        world_spawn_position: [0; 3],
        air_network_id: protocol_ids::SEQUENTIAL_AIR_NETWORK_ID,
        block_network_ids_are_hashes: false,
    });
    let definitions: Arc<[BiomeDefinitionEvent]> = Arc::from([BiomeDefinitionEvent {
        biome_id: Some(0xfffe),
        name: Arc::from("example:high"),
        temperature: 0.8,
        downfall: 0.4,
        snow_foliage: 0.0,
        max_snow_accumulation: None,
        map_water_color: 0xff44_6688,
    }]);

    assert_eq!(stream.biome_tint_revision(), 0);
    assert_eq!(
        stream.resolved_biome_tints_snapshot().dense_index(0xfffe),
        0
    );
    stream
        .submit(
            2,
            WorldEvent::BiomeDefinitions(BiomeDefinitionsEvent {
                definitions: Arc::clone(&definitions),
            }),
        )
        .unwrap();
    assert_eq!(stream.biome_tint_revision(), 0);

    stream.submit(1, WorldEvent::ChunkRadiusUpdated(8)).unwrap();
    let resolved = stream.resolved_biome_tints_snapshot();
    assert_eq!(stream.biome_tint_revision(), 1);
    assert_ne!(resolved.dense_index(0xfffe), 0);
    assert_eq!(resolved.dense_index(123), 0);
    assert!(Arc::ptr_eq(
        &stream.biome_definitions_snapshot(),
        &definitions
    ));
}

#[test]
fn palette_native_biome_packing_uses_exact_lookup_and_safe_fallbacks() {
    let mut stream = WorldStream::new(WorldBootstrap {
        local_player_unique_id: 1,
        dimension: 0,
        local_player_runtime_id: 1,
        player_position: [0.0; 3],
        world_spawn_position: [0; 3],
        air_network_id: protocol_ids::SEQUENTIAL_AIR_NETWORK_ID,
        block_network_ids_are_hashes: false,
    });
    stream
        .submit(
            1,
            WorldEvent::BiomeDefinitions(BiomeDefinitionsEvent {
                definitions: Arc::from([BiomeDefinitionEvent {
                    biome_id: Some(42),
                    name: Arc::from("example:resolved"),
                    temperature: 0.8,
                    downfall: 0.4,
                    snow_foliage: 0.0,
                    max_snow_accumulation: None,
                    map_water_color: 0xff44_6688,
                }]),
            }),
        )
        .unwrap();
    let key = SubChunkKey::new(0, 0, -4, 0);
    stream.authority.commit_biome_column(
        key.chunk(),
        DecodedBiomeColumn::decode(-4, 1, &[1, 84], &RAW_BIOMES),
    );
    let resolved_storage = stream.authority.terrain().biome_storage(key).unwrap();
    let resolved = stream.resolved_biome_tints_snapshot();

    let packed = super::pack_biome_record(
        &biome_neighbourhood_with_center(Some(resolved_storage)),
        &resolved,
    );
    assert_eq!(packed.tint_index(0, 0, 0), Some(resolved.dense_index(42)));

    stream.authority.commit_biome_column(
        key.chunk(),
        DecodedBiomeColumn::decode(-4, 1, &[1, 86], &RAW_BIOMES),
    );
    let missing_storage = stream.authority.terrain().biome_storage(key).unwrap();
    let missing = super::pack_biome_record(
        &biome_neighbourhood_with_center(Some(missing_storage)),
        &resolved,
    );
    assert_eq!(missing.tint_index(0, 0, 0), Some(0));

    let absent = super::pack_biome_record(&biome_neighbourhood_with_center(None), &resolved);
    assert_eq!(absent.tint_index(0, 0, 0), Some(0));
}

#[test]
fn definition_replacement_supersedes_queued_and_in_flight_old_tints() {
    let mut stream = WorldStream::new(WorldBootstrap {
        local_player_unique_id: 1,
        dimension: 0,
        local_player_runtime_id: 1,
        player_position: [0.0; 3],
        world_spawn_position: [0; 3],
        air_network_id: protocol_ids::SEQUENTIAL_AIR_NETWORK_ID,
        block_network_ids_are_hashes: false,
    });
    let definition = |name: &'static str, temperature| BiomeDefinitionEvent {
        biome_id: Some(42),
        name: Arc::from(name),
        temperature,
        downfall: 0.4,
        snow_foliage: 0.0,
        max_snow_accumulation: None,
        map_water_color: 0xff44_6688,
    };
    stream
        .submit(
            1,
            WorldEvent::BiomeDefinitions(BiomeDefinitionsEvent {
                definitions: Arc::from([definition("example:old", 0.8)]),
            }),
        )
        .unwrap();

    let key = SubChunkKey::new(0, 0, -4, 0);
    stream
        .authority
        .commit_level_chunk(
            key.chunk(),
            DecodedLevelChunk::decode(
                -4,
                1,
                include_bytes!(concat!(
                    env!("CARGO_MANIFEST_DIR"),
                    "/../world/fixtures/uniform_non_air.bin"
                )),
                &RAW_IDS,
            ),
        )
        .unwrap();
    stream.authority.commit_biome_column(
        key.chunk(),
        DecodedBiomeColumn::decode(-4, 1, &[1, 84], &RAW_BIOMES),
    );
    stream.resident.insert(key);
    let source = stream.authority.terrain().sub_chunk(key).unwrap();
    let biome_source = stream.authority.terrain().biome_storage(key).unwrap();
    let old_generation = stream.revisions.mark_dirty(key, Instant::now());
    stream.mesh_jobs.in_flight.insert(key, old_generation);
    let old_tint_identity = stream.biome_tint_identity();
    let old_tint_revision = old_tint_identity.revision();
    let old_resolved = stream.resolved_biome_tints_snapshot();
    let queued_mesh = mesh_sub_chunk(
        &stream.classifier,
        stream.runtime_assets(),
        stream.network_id_mode(),
        &Neighbourhood::empty(),
        &source,
    );
    let in_flight_mesh = mesh_sub_chunk(
        &stream.classifier,
        stream.runtime_assets(),
        stream.network_id_mode(),
        &Neighbourhood::empty(),
        &source,
    );

    stream.accept_mesh_completion(MeshCompletion {
        output_permit: None,
        _job_permit: None,
        key,
        revision: old_generation,
        source: Arc::clone(&source),
        biome_sources: biome_neighbourhood_with_center(Some(Arc::clone(&biome_source))),
        biome: super::pack_biome_record(
            &biome_neighbourhood_with_center(Some(Arc::clone(&biome_source))),
            &old_resolved,
        ),
        tint_identity: old_tint_identity,
        mesh: queued_mesh,
        dependency_mask: MeshDependencyMask::default(),
        light_halo: Default::default(),
        queue_wait: Duration::ZERO,
        dispatch_wait: Duration::ZERO,
        duration: Duration::ZERO,
        urgent: false,
    });
    assert_eq!(stream.pending_mesh_change_count(), 1);
    stream.mesh_jobs.in_flight.insert(key, old_generation);

    stream
        .submit(
            2,
            WorldEvent::BiomeDefinitions(BiomeDefinitionsEvent {
                definitions: Arc::from([definition("example:new", 0.2)]),
            }),
        )
        .unwrap();

    assert_eq!(stream.biome_tint_revision(), old_tint_revision + 1);
    assert_eq!(stream.pending_mesh_change_count(), 0);
    assert!(!stream.mesh_jobs.in_flight.contains_key(&key));
    assert!(stream.mesh_jobs.pending.contains_key(&key));
    assert!(!stream.revisions.is_current(key, old_generation));

    stream.accept_mesh_completion(MeshCompletion {
        output_permit: None,
        _job_permit: None,
        key,
        revision: old_generation,
        source,
        biome_sources: biome_neighbourhood_with_center(Some(biome_source)),
        biome: super::pack_biome_record(&biome_neighbourhood_with_center(None), &old_resolved),
        tint_identity: old_tint_identity,
        mesh: in_flight_mesh,
        dependency_mask: MeshDependencyMask::default(),
        light_halo: Default::default(),
        queue_wait: Duration::ZERO,
        dispatch_wait: Duration::ZERO,
        duration: Duration::ZERO,
        urgent: false,
    });
    assert_eq!(stream.stats().stale_mesh_jobs, 1);
    assert!(stream.pop_mesh_change().is_none());
}

#[test]
fn network_mode_and_runtime_assets_are_selected_once_per_stream() {
    let runtime_assets = Arc::new(RuntimeAssets::diagnostic());
    let stream = WorldStream::new_with_assets(
        WorldBootstrap {
            local_player_unique_id: 1,
            dimension: 0,
            local_player_runtime_id: 1,
            player_position: [0.0; 3],
            world_spawn_position: [0; 3],
            air_network_id: 0xdbf4_4120,
            block_network_ids_are_hashes: true,
        },
        Arc::clone(&runtime_assets),
        [0.0, super::server_position::SAFE_SERVER_HEIGHT, 0.0],
        None,
    );

    assert_eq!(stream.network_id_mode(), NetworkIdMode::Hashed);
    assert_eq!(stream.network_id_mode(), NetworkIdMode::Hashed);
    assert!(std::ptr::eq(
        stream.collision_store(),
        stream.authority.terrain()
    ));
    assert_eq!(stream.classifier.air_network_id(), 0xdbf4_4120);
    assert!(Arc::ptr_eq(stream.runtime_assets(), &runtime_assets));
}

#[test]
fn world_stream_classifier_uses_runtime_registry_air_for_each_network_mode() {
    let runtime_assets = Arc::new(non_default_air_runtime_assets());
    for (hashes, expected_air) in [(false, 2), (true, 0xdbf4_4120)] {
        let stream = WorldStream::new_with_assets(
            WorldBootstrap {
                local_player_unique_id: 1,
                dimension: 0,
                local_player_runtime_id: 1,
                player_position: [0.0; 3],
                world_spawn_position: [0; 3],
                air_network_id: 99,
                block_network_ids_are_hashes: hashes,
            },
            Arc::clone(&runtime_assets),
            [0.0, super::server_position::SAFE_SERVER_HEIGHT, 0.0],
            None,
        );

        assert_eq!(stream.classifier.air_network_id(), expected_air);
    }
}

#[test]
fn inline_level_chunk_decodes_full_dimension_biomes_independent_of_block_count() {
    let mut stream = WorldStream::new(WorldBootstrap {
        local_player_unique_id: 1,
        dimension: 0,
        local_player_runtime_id: 1,
        player_position: [0.0; 3],
        world_spawn_position: [0; 3],
        air_network_id: protocol_ids::SEQUENTIAL_AIR_NETWORK_ID,
        block_network_ids_are_hashes: false,
    });
    define_custom_biomes(&mut stream, [7]);
    let mut payload = vec![9, 0, (-4_i8) as u8];
    payload.extend(biome_payload(0, 7));

    stream
        .submit(
            1,
            WorldEvent::LevelChunk(LevelChunkEvent {
                dimension: 0,
                x: 0,
                z: 0,
                mode: LevelChunkMode::Inline { count: 1 },
                payload,
            }),
        )
        .unwrap();
    complete_pending_decode_jobs(&mut stream);

    assert_eq!(
        stream
            .authority
            .terrain()
            .biome_id(SubChunkKey::new(0, 0, -4, 0), 0, 0, 0),
        Some(7)
    );
    assert_eq!(
        stream
            .authority
            .terrain()
            .biome_id(SubChunkKey::new(0, 0, 19, 0), 0, 0, 0),
        Some(7)
    );
}

#[test]
fn request_level_chunk_decodes_biomes_before_enqueuing_sub_chunk_requests() {
    let mut stream = WorldStream::new(WorldBootstrap {
        local_player_unique_id: 1,
        dimension: 0,
        local_player_runtime_id: 1,
        player_position: [0.0; 3],
        world_spawn_position: [0; 3],
        air_network_id: protocol_ids::SEQUENTIAL_AIR_NETWORK_ID,
        block_network_ids_are_hashes: false,
    });

    define_custom_biomes(&mut stream, [9]);
    stream
        .submit(
            1,
            request_level_chunk_event(0, 0, 0, LevelChunkMode::LimitedRequests { highest: 1 }, 9),
        )
        .unwrap();

    assert_eq!(stream.pending_decode.len(), 1);
    assert!(stream.take_requests().is_empty());

    complete_pending_decode_jobs(&mut stream);

    assert_eq!(
        stream
            .authority
            .terrain()
            .biome_id(SubChunkKey::new(0, 0, -4, 0), 0, 0, 0),
        Some(9)
    );
    assert_eq!(
        stream
            .authority
            .terrain()
            .biome_id(SubChunkKey::new(0, 0, 19, 0), 0, 0, 0),
        Some(9)
    );
    assert_eq!(stream.take_requests().len(), 1);
}

#[test]
fn request_mode_biome_arrival_dirties_vertical_diagonal_blend_dependents() {
    let mut stream = WorldStream::new(WorldBootstrap {
        local_player_unique_id: 1,
        dimension: 0,
        local_player_runtime_id: 1,
        player_position: [0.0; 3],
        world_spawn_position: [0; 3],
        air_network_id: protocol_ids::SEQUENTIAL_AIR_NETWORK_ID,
        block_network_ids_are_hashes: false,
    });
    let neighbour = SubChunkKey::new(0, 1, -5, 1);
    let mut payload = include_bytes!(concat!(
        env!("CARGO_MANIFEST_DIR"),
        "/../world/fixtures/uniform_non_air.bin"
    ))
    .to_vec();
    // Version 9 carries the absolute subchunk Y in its third byte.
    payload[2] = neighbour.y as i8 as u8;
    stream
        .authority
        .commit_level_chunk(
            neighbour.chunk(),
            DecodedLevelChunk::decode(-5, 1, &payload, &RAW_IDS),
        )
        .unwrap();
    stream.resident.insert(neighbour);
    let generation = stream.mark_dirty_exact(neighbour, Instant::now());
    assert!(stream.register_mesh_dependency_mask(
        neighbour,
        generation,
        MeshDependencyMask::default(),
    ));
    stream.mesh_jobs.pending.clear();

    stream
        .submit(
            1,
            request_level_chunk_event(0, 0, 0, LevelChunkMode::LimitedRequests { highest: 1 }, 9),
        )
        .unwrap();
    complete_pending_decode_jobs(&mut stream);

    assert!(
        stream.mesh_jobs.pending.contains_key(&neighbour),
        "a newly committed biome column must invalidate a vertical diagonal tint halo"
    );
}

#[test]
fn inline_biome_replacement_dirties_diagonal_cross_chunk_blend_dependents() {
    let mut stream = WorldStream::new(WorldBootstrap {
        local_player_unique_id: 1,
        dimension: 0,
        local_player_runtime_id: 1,
        player_position: [0.0; 3],
        world_spawn_position: [0; 3],
        air_network_id: protocol_ids::SEQUENTIAL_AIR_NETWORK_ID,
        block_network_ids_are_hashes: false,
    });
    let source = SubChunkKey::new(0, 0, -4, 0);
    let diagonal = SubChunkKey::new(0, 1, -4, 1);
    let block_payload = include_bytes!(concat!(
        env!("CARGO_MANIFEST_DIR"),
        "/../world/fixtures/uniform_non_air.bin"
    ));
    let mut original_payload = block_payload.to_vec();
    original_payload.extend(biome_payload(0, 42));
    stream
        .authority
        .commit_level_chunk(
            source.chunk(),
            DecodedLevelChunk::decode_inline(
                source.chunk(),
                world::DimensionSlots {
                    base_sub_chunk_y: -4,
                    count: 24,
                },
                1,
                &original_payload,
                &RAW_IDS,
                &RAW_BIOMES,
            ),
        )
        .unwrap();
    stream
        .authority
        .commit_level_chunk(
            diagonal.chunk(),
            DecodedLevelChunk::decode(-4, 1, block_payload, &RAW_IDS),
        )
        .unwrap();
    stream.resident.extend([source, diagonal]);
    let generation = stream.mark_dirty_exact(diagonal, Instant::now());
    assert!(stream.register_mesh_dependency_mask(
        diagonal,
        generation,
        MeshDependencyMask::default(),
    ));
    stream.mesh_jobs.pending.clear();

    let mut replacement_payload = block_payload.to_vec();
    replacement_payload.extend(biome_payload(0, 43));
    stream
        .submit(
            1,
            WorldEvent::LevelChunk(LevelChunkEvent {
                dimension: 0,
                x: 0,
                z: 0,
                mode: LevelChunkMode::Inline { count: 1 },
                payload: replacement_payload,
            }),
        )
        .unwrap();
    complete_pending_decode_jobs(&mut stream);

    assert!(
        stream.mesh_jobs.pending.contains_key(&diagonal),
        "an inline biome replacement must invalidate a diagonal tint halo independently of AO"
    );
}

#[test]
fn evicting_a_diagonal_biome_only_column_dirties_the_remaining_blend_boundary() {
    let mut stream = WorldStream::new(WorldBootstrap {
        local_player_unique_id: 1,
        dimension: 0,
        local_player_runtime_id: 1,
        player_position: [0.0; 3],
        world_spawn_position: [0; 3],
        air_network_id: protocol_ids::SEQUENTIAL_AIR_NETWORK_ID,
        block_network_ids_are_hashes: false,
    });
    let center = SubChunkKey::new(0, 0, -4, 0);
    stream
        .authority
        .commit_level_chunk(
            center.chunk(),
            DecodedLevelChunk::decode(
                -4,
                1,
                include_bytes!(concat!(
                    env!("CARGO_MANIFEST_DIR"),
                    "/../world/fixtures/uniform_non_air.bin"
                )),
                &RAW_IDS,
            ),
        )
        .unwrap();
    stream.authority.commit_biome_column(
        ChunkKey::new(0, 1, 1),
        DecodedBiomeColumn::decode(-4, 1, &[1, 84], &RAW_BIOMES),
    );
    stream.resident.insert(center);
    let generation = stream.mark_dirty_exact(center, Instant::now());
    assert!(stream.register_mesh_dependency_mask(
        center,
        generation,
        MeshDependencyMask::default(),
    ));
    stream.mesh_jobs.pending.clear();

    stream.evict_column(ChunkKey::new(0, 1, 1));

    assert!(stream.mesh_jobs.pending.contains_key(&center));
}

#[test]
fn truncated_request_level_chunk_biomes_repeat_the_last_storage() {
    let mut stream = WorldStream::new(WorldBootstrap {
        local_player_unique_id: 1,
        dimension: 0,
        local_player_runtime_id: 1,
        player_position: [0.0; 3],
        world_spawn_position: [0; 3],
        air_network_id: protocol_ids::SEQUENTIAL_AIR_NETWORK_ID,
        block_network_ids_are_hashes: false,
    });
    define_custom_biomes(&mut stream, [5, 9]);

    stream
        .submit(
            1,
            request_level_chunk_event(0, 0, 0, LevelChunkMode::LimitedRequests { highest: 1 }, 5),
        )
        .unwrap();
    complete_pending_decode_jobs(&mut stream);
    assert_eq!(stream.take_requests().len(), 1);

    stream
        .submit(
            2,
            WorldEvent::LevelChunk(LevelChunkEvent {
                dimension: 0,
                x: 0,
                z: 0,
                mode: LevelChunkMode::LimitedRequests { highest: 1 },
                payload: vec![1, 18],
            }),
        )
        .unwrap();
    complete_pending_decode_jobs(&mut stream);

    assert!(stream.take_fatal_error().is_none());
    assert_eq!(stream.stats().decode_errors, 0);
    for y in [-4, 19] {
        assert_eq!(
            stream
                .authority
                .terrain()
                .biome_id(SubChunkKey::new(0, 0, y, 0), 0, 0, 0),
            Some(9)
        );
    }
}
