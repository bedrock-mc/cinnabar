use super::*;

#[test]
fn biome_definition_snapshot_commits_in_fifo_and_survives_dimension_changes() {
    let mut stream = WorldStream::new(WorldBootstrap {
        dimension: 0,
        local_player_runtime_id: 1,
        player_position: [0.0; 3],
        world_spawn_position: [0; 3],
        air_network_id: 12_530,
        block_network_ids_are_hashes: false,
    });
    let definitions: Arc<[BiomeDefinitionEvent]> = Arc::from(vec![BiomeDefinitionEvent {
        biome_id: None,
        name: Arc::from("minecraft:plains"),
        temperature: 0.8,
        downfall: 0.4,
        snow_foliage: 0.0,
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
            }),
        )
        .unwrap();
    let after_dimension_change = stream.biome_definitions_snapshot();
    assert!(Arc::ptr_eq(&after_dimension_change, &definitions));
}

#[test]
fn stale_biome_definition_event_cannot_replace_the_committed_snapshot() {
    let mut stream = WorldStream::new(WorldBootstrap {
        dimension: 0,
        local_player_runtime_id: 1,
        player_position: [0.0; 3],
        world_spawn_position: [0; 3],
        air_network_id: 12_530,
        block_network_ids_are_hashes: false,
    });
    let committed: Arc<[BiomeDefinitionEvent]> = Arc::from(vec![BiomeDefinitionEvent {
        biome_id: None,
        name: Arc::from("minecraft:plains"),
        temperature: 0.8,
        downfall: 0.4,
        snow_foliage: 0.0,
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
        dimension: 0,
        local_player_runtime_id: 1,
        player_position: [0.0; 3],
        world_spawn_position: [0; 3],
        air_network_id: 12_530,
        block_network_ids_are_hashes: false,
    });
    let definitions: Arc<[BiomeDefinitionEvent]> = Arc::from([BiomeDefinitionEvent {
        biome_id: Some(0xfffe),
        name: Arc::from("example:high"),
        temperature: 0.8,
        downfall: 0.4,
        snow_foliage: 0.0,
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
fn biome_tint_revision_overflow_keeps_the_previous_atomic_snapshot() {
    let mut stream = WorldStream::new(WorldBootstrap {
        dimension: 0,
        local_player_runtime_id: 1,
        player_position: [0.0; 3],
        world_spawn_position: [0; 3],
        air_network_id: 12_530,
        block_network_ids_are_hashes: false,
    });
    stream.biome_tint_revision = u64::MAX;
    let previous = stream.resolved_biome_tints_snapshot();

    stream
        .submit(
            1,
            WorldEvent::BiomeDefinitions(BiomeDefinitionsEvent {
                definitions: Arc::from([BiomeDefinitionEvent {
                    biome_id: Some(42),
                    name: Arc::from("example:overflow"),
                    temperature: 0.8,
                    downfall: 0.4,
                    snow_foliage: 0.0,
                    map_water_color: 0xff44_6688,
                }]),
            }),
        )
        .unwrap();

    assert_eq!(stream.biome_tint_revision(), u64::MAX);
    assert!(stream.biome_definitions_snapshot().is_empty());
    assert!(Arc::ptr_eq(
        &previous,
        &stream.resolved_biome_tints_snapshot()
    ));
    assert_eq!(
        stream
            .stats()
            .normalization_reasons
            .biome_tint_revision_overflows,
        1
    );
}

#[test]
fn palette_native_biome_packing_uses_exact_lookup_and_safe_fallbacks() {
    let mut stream = WorldStream::new(WorldBootstrap {
        dimension: 0,
        local_player_runtime_id: 1,
        player_position: [0.0; 3],
        world_spawn_position: [0; 3],
        air_network_id: 12_530,
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
                    map_water_color: 0xff44_6688,
                }]),
            }),
        )
        .unwrap();
    let key = SubChunkKey::new(0, 0, -4, 0);
    stream.store.commit_biome_column(
        key.chunk(),
        DecodedBiomeColumn::decode(-4, 1, &[1, 84]).unwrap(),
    );
    let resolved_storage = stream.store.biome_storage(key).unwrap();
    let resolved = stream.resolved_biome_tints_snapshot();

    let packed = super::pack_biome_record(
        &biome_neighbourhood_with_center(Some(resolved_storage)),
        &resolved,
    );
    assert_eq!(packed.tint_index(0, 0, 0), Some(resolved.dense_index(42)));

    stream.store.commit_biome_column(
        key.chunk(),
        DecodedBiomeColumn::decode(-4, 1, &[1, 86]).unwrap(),
    );
    let missing_storage = stream.store.biome_storage(key).unwrap();
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
        dimension: 0,
        local_player_runtime_id: 1,
        player_position: [0.0; 3],
        world_spawn_position: [0; 3],
        air_network_id: 12_530,
        block_network_ids_are_hashes: false,
    });
    let definition = |name: &'static str, temperature| BiomeDefinitionEvent {
        biome_id: Some(42),
        name: Arc::from(name),
        temperature,
        downfall: 0.4,
        snow_foliage: 0.0,
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
    stream.store.commit_level_chunk(
        key.chunk(),
        DecodedLevelChunk::decode(
            -4,
            1,
            include_bytes!(concat!(
                env!("CARGO_MANIFEST_DIR"),
                "/../world/fixtures/uniform_non_air.bin"
            )),
        )
        .unwrap(),
    );
    stream.store.commit_biome_column(
        key.chunk(),
        DecodedBiomeColumn::decode(-4, 1, &[1, 84]).unwrap(),
    );
    stream.resident.insert(key);
    let source = stream.store.sub_chunk(key).unwrap();
    let biome_source = stream.store.biome_storage(key).unwrap();
    let old_generation = stream.revisions.mark_dirty(key, Instant::now());
    stream.in_flight.insert(key, old_generation);
    let old_tint_identity = stream.biome_tint_identity();
    let old_tint_revision = old_tint_identity.revision();
    let old_resolved = stream.resolved_biome_tints_snapshot();
    let queued_mesh = mesh_sub_chunk(
        &stream.classifier,
        &stream.runtime_assets,
        stream.network_id_mode,
        &Neighbourhood::empty(),
        &source,
    );
    let in_flight_mesh = mesh_sub_chunk(
        &stream.classifier,
        &stream.runtime_assets,
        stream.network_id_mode,
        &Neighbourhood::empty(),
        &source,
    );

    stream.accept_mesh_completion(MeshCompletion {
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
        duration: Duration::ZERO,
    });
    assert_eq!(stream.pending_mesh_change_count(), 1);
    stream.in_flight.insert(key, old_generation);

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
    assert!(!stream.in_flight.contains_key(&key));
    assert!(stream.pending_mesh.contains_key(&key));
    assert!(!stream.revisions.is_current(key, old_generation));

    stream.accept_mesh_completion(MeshCompletion {
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
        duration: Duration::ZERO,
    });
    assert_eq!(stream.stats().stale_mesh_jobs, 1);
    assert!(stream.pop_mesh_change().is_none());
}

#[test]
fn network_mode_and_runtime_assets_are_selected_once_per_stream() {
    let runtime_assets = Arc::new(RuntimeAssets::diagnostic());
    let stream = WorldStream::new_with_assets(
        WorldBootstrap {
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

    assert_eq!(stream.network_id_mode, NetworkIdMode::Hashed);
    assert_eq!(stream.classifier.air_network_id(), 0xdbf4_4120);
    assert!(Arc::ptr_eq(&stream.runtime_assets, &runtime_assets));
}

#[test]
fn world_stream_classifier_uses_runtime_registry_air_for_each_network_mode() {
    let runtime_assets = Arc::new(non_default_air_runtime_assets());
    for (hashes, expected_air) in [(false, 2), (true, 0xdbf4_4120)] {
        let stream = WorldStream::new_with_assets(
            WorldBootstrap {
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
fn render_mesh_api_consumes_only_the_shared_world_neighbourhood() {
    let _: for<'a, 'b, 'c, 'd> fn(
        &'a BlockClassifier,
        &'b RuntimeAssets,
        NetworkIdMode,
        &'c world::MeshNeighbourhood<'d>,
    ) -> ::meshing::ChunkMesh = ::meshing::mesh_sub_chunk_in_neighbourhood;
}

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
        center: Arc::new(uniform_sub_chunk(1)),
        biomes: std::array::from_fn(|_| None),
        adjacent: std::array::from_fn(|_| None),
        light_halo,
    };

    let mesh = snapshot.mesh(
        BlockClassifier::new(0),
        &RuntimeAssets::diagnostic(),
        NetworkIdMode::Sequential,
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
        .store
        .commit_sub_chunk(key, uniform_sub_chunk(0))
        .unwrap();
    stream
        .store
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
        .store
        .update_block(key, BlockUpdate::new(4, 4, 4, 0, 3), 0)
        .unwrap();
    stream
        .store
        .update_block(key, BlockUpdate::new(4, 4, 4, 1, 1), 0)
        .unwrap();
    assert_eq!(
        stream.camera_medium([4.5, 4.5, 4.5]),
        ::meshing::CameraMedium::Water
    );

    stream
        .store
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
fn block_entity_visual_diagnostics_preserve_zero_remesh_live_updates() {
    let mut stream = block_entity_visual_stream();
    let routes = [
        ("Barrel", 7_069),
        ("BlastFurnace", 15_143),
        ("Furnace", 15_688),
        ("Smoker", 2_699),
        ("Jukebox", 8_516),
    ];
    let mut sequence = 1;
    for (chunk_x, (id, runtime_id)) in routes.into_iter().enumerate() {
        let position = [chunk_x as i32 * 16 + 1, -63, 2];
        stream
            .submit(
                sequence,
                inline_block_entity_event(
                    chunk_x as i32,
                    runtime_id,
                    block_entity_nbt(id, position),
                ),
            )
            .unwrap();
        sequence += 1;
        complete_pending_decode_jobs(&mut stream);
    }
    let note_position = [5 * 16 + 1, -63, 2];
    stream
        .submit(
            sequence,
            inline_block_entity_event(
                5,
                1_936,
                idless_note_block_entity_nbt(note_position, 24, 1, 0),
            ),
        )
        .unwrap();
    sequence += 1;
    complete_pending_decode_jobs(&mut stream);

    let stats = stream.stats();
    assert_eq!(stats.adjudicated_static_block_entities, 4);
    assert_eq!(stats.adjudicated_logical_block_entities, 2);
    assert_eq!(stats.deferred_block_entities, 0);
    assert_eq!(stats.unknown_block_entities, 0);

    let revision_before = stream.revisions.next_revision;
    let pending_before = stream.pending_mesh.len();
    let changes_before = stream.mesh_changes.len();
    for (chunk_x, (id, _)) in routes.into_iter().enumerate() {
        let position = [chunk_x as i32 * 16 + 1, -63, 2];
        stream
            .submit(
                sequence,
                WorldEvent::BlockEntityUpdate(BlockEntityUpdateEvent {
                    dimension: 0,
                    position,
                    nbt: block_entity_nbt_with_marker(id, position, 1),
                }),
            )
            .unwrap();
        sequence += 1;
    }
    stream
        .submit(
            sequence,
            WorldEvent::BlockEntityUpdate(BlockEntityUpdateEvent {
                dimension: 0,
                position: note_position,
                nbt: idless_note_block_entity_nbt(note_position, 0, 0, 1),
            }),
        )
        .unwrap();
    sequence += 1;
    complete_pending_decode_jobs(&mut stream);
    assert_eq!(stream.revisions.next_revision, revision_before);
    assert_eq!(stream.pending_mesh.len(), pending_before);
    assert_eq!(stream.mesh_changes.len(), changes_before);
    assert_eq!(stream.stats().adjudicated_static_block_entities, 4);
    assert_eq!(stream.stats().adjudicated_logical_block_entities, 2);

    let retained = stream.store.block_entity(BlockEntityKey::new(
        0,
        note_position[0],
        note_position[1],
        note_position[2],
    ));
    stream
        .submit(
            sequence,
            WorldEvent::BlockEntityUpdate(BlockEntityUpdateEvent {
                dimension: 0,
                position: note_position,
                nbt: vec![10, 0, 8],
            }),
        )
        .unwrap();
    complete_pending_decode_jobs(&mut stream);
    assert!(Arc::ptr_eq(
        retained.as_ref().unwrap(),
        &stream
            .store
            .block_entity(BlockEntityKey::new(
                0,
                note_position[0],
                note_position[1],
                note_position[2],
            ))
            .unwrap()
    ));
    assert_eq!(stream.stats().adjudicated_logical_block_entities, 2);
    assert_eq!(stream.revisions.next_revision, revision_before);
}

#[test]
fn block_entity_visual_diagnostics_preserve_zero_churn_inline_nbt_replacements() {
    let mut stream = block_entity_visual_stream();
    let mut cases = [
        ("Barrel", 7_069, Vec::new(), Vec::new()),
        ("BlastFurnace", 15_143, Vec::new(), Vec::new()),
        ("Furnace", 15_688, Vec::new(), Vec::new()),
        ("Smoker", 2_699, Vec::new(), Vec::new()),
        ("Jukebox", 8_516, Vec::new(), Vec::new()),
        ("", 1_936, Vec::new(), Vec::new()),
    ];
    for (chunk_x, (id, _, initial, replacement)) in cases.iter_mut().enumerate() {
        let position = [chunk_x as i32 * 16 + 1, -63, 2];
        if id.is_empty() {
            *initial = idless_note_block_entity_nbt(position, 24, 1, 0);
            *replacement = idless_note_block_entity_nbt(position, 0, 0, 1);
        } else {
            *initial = block_entity_nbt(id, position);
            *replacement = block_entity_nbt_with_marker(id, position, 1);
        }
    }

    let mut sequence = 1;
    for (chunk_x, (_, runtime_id, initial, _)) in cases.iter().enumerate() {
        stream
            .submit(
                sequence,
                inline_block_entity_event(chunk_x as i32, *runtime_id, initial.clone()),
            )
            .unwrap();
        sequence += 1;
        complete_pending_decode_jobs(&mut stream);
    }
    assert_eq!(stream.stats().adjudicated_static_block_entities, 4);
    assert_eq!(stream.stats().adjudicated_logical_block_entities, 2);

    let mesh_revision_before = stream.revisions.next_revision;
    let block_generation_before = stream.next_block_generation;
    let light_revision_before = stream.light_revisions.next_revision;
    let render_generation_before = stream.connectivity_generation;
    let block_generations_before = stream.block_generations.clone();
    let applied_mesh_generations_before = stream.applied_mesh_generations.clone();
    let pending_mesh_before = stream.pending_mesh.len();
    let pending_light_before = stream.pending_light.len();
    let mesh_changes_before = stream.mesh_changes.len();

    for (chunk_x, (_, runtime_id, _, replacement)) in cases.iter().enumerate() {
        stream
            .submit(
                sequence,
                inline_block_entity_event(chunk_x as i32, *runtime_id, replacement.clone()),
            )
            .unwrap();
        sequence += 1;
        complete_pending_decode_jobs(&mut stream);
    }

    assert_eq!(stream.revisions.next_revision, mesh_revision_before);
    assert_eq!(stream.next_block_generation, block_generation_before);
    assert_eq!(stream.light_revisions.next_revision, light_revision_before);
    assert_eq!(stream.connectivity_generation, render_generation_before);
    assert_eq!(stream.block_generations, block_generations_before);
    assert_eq!(
        stream.applied_mesh_generations,
        applied_mesh_generations_before
    );
    assert_eq!(stream.pending_mesh.len(), pending_mesh_before);
    assert_eq!(stream.pending_light.len(), pending_light_before);
    assert_eq!(stream.mesh_changes.len(), mesh_changes_before);
    assert_eq!(stream.stats().adjudicated_static_block_entities, 4);
    assert_eq!(stream.stats().adjudicated_logical_block_entities, 2);
    for (chunk_x, (_, _, _, replacement)) in cases.iter().enumerate() {
        let position = [chunk_x as i32 * 16 + 1, -63, 2];
        let retained = stream
            .store
            .block_entity(BlockEntityKey::new(
                0,
                position[0],
                position[1],
                position[2],
            ))
            .expect("inline replacement retained");
        assert_eq!(retained.bytes(), replacement);
    }
}

#[test]
fn block_entity_visual_diagnostics_preserve_zero_remesh_request_mode_nbt_replacements() {
    let mut stream = block_entity_visual_stream();
    let mut cases = [
        ("Barrel", 7_069, Vec::new(), Vec::new()),
        ("BlastFurnace", 15_143, Vec::new(), Vec::new()),
        ("Furnace", 15_688, Vec::new(), Vec::new()),
        ("Smoker", 2_699, Vec::new(), Vec::new()),
        ("Jukebox", 8_516, Vec::new(), Vec::new()),
        ("", 1_936, Vec::new(), Vec::new()),
    ];
    for (chunk_x, (id, _, initial, replacement)) in cases.iter_mut().enumerate() {
        let position = [chunk_x as i32 * 16 + 1, -63, 2];
        if id.is_empty() {
            *initial = idless_note_block_entity_nbt(position, 24, 1, 0);
            *replacement = idless_note_block_entity_nbt(position, 0, 0, 1);
        } else {
            *initial = block_entity_nbt(id, position);
            *replacement = block_entity_nbt_with_marker(id, position, 1);
        }
    }

    let mut sequence = 1;
    for (chunk_x, (_, runtime_id, initial, _)) in cases.iter().enumerate() {
        stream
            .submit(
                sequence,
                request_block_entity_event(chunk_x as i32, initial.clone()),
            )
            .unwrap();
        sequence += 1;
        complete_pending_decode_jobs(&mut stream);
        assert_eq!(stream.take_requests().len(), 1);
        stream
            .submit(
                sequence,
                requested_block_entity_sub_chunk_event(
                    chunk_x as i32,
                    *runtime_id,
                    initial.clone(),
                ),
            )
            .unwrap();
        sequence += 1;
        complete_pending_decode_jobs(&mut stream);
    }
    assert_eq!(stream.stats().adjudicated_static_block_entities, 4);
    assert_eq!(stream.stats().adjudicated_logical_block_entities, 2);

    let revision_before = stream.revisions.next_revision;
    let pending_before = stream.pending_mesh.len();
    let changes_before = stream.mesh_changes.len();
    for (chunk_x, (_, runtime_id, _, replacement)) in cases.iter().enumerate() {
        stream
            .submit(
                sequence,
                request_block_entity_event(chunk_x as i32, replacement.clone()),
            )
            .unwrap();
        sequence += 1;
        complete_pending_decode_jobs(&mut stream);
        assert_eq!(stream.take_requests().len(), 1);
        stream
            .submit(
                sequence,
                requested_block_entity_sub_chunk_event(
                    chunk_x as i32,
                    *runtime_id,
                    replacement.clone(),
                ),
            )
            .unwrap();
        sequence += 1;
        complete_pending_decode_jobs(&mut stream);
    }

    assert_eq!(stream.revisions.next_revision, revision_before);
    assert_eq!(stream.pending_mesh.len(), pending_before);
    assert_eq!(stream.mesh_changes.len(), changes_before);
    assert_eq!(stream.stats().adjudicated_static_block_entities, 4);
    assert_eq!(stream.stats().adjudicated_logical_block_entities, 2);
    for (chunk_x, (_, _, _, replacement)) in cases.iter().enumerate() {
        let position = [chunk_x as i32 * 16 + 1, -63, 2];
        let retained = stream
            .store
            .block_entity(BlockEntityKey::new(
                0,
                position[0],
                position[1],
                position[2],
            ))
            .expect("request replacement retained");
        assert_eq!(retained.bytes(), replacement);
    }
}

#[test]
fn request_mode_changed_biome_keeps_destructive_column_replacement() {
    let mut stream = block_entity_visual_stream();
    let position = [1, -63, 2];
    let key = SubChunkKey::new(0, 0, -4, 0);
    let initial = block_entity_nbt("Jukebox", position);
    stream
        .submit(1, request_block_entity_event(0, initial.clone()))
        .unwrap();
    complete_pending_decode_jobs(&mut stream);
    assert_eq!(stream.take_requests().len(), 1);
    stream
        .submit(2, requested_block_entity_sub_chunk_event(0, 8_516, initial))
        .unwrap();
    complete_pending_decode_jobs(&mut stream);
    assert!(stream.store.sub_chunk(key).is_some());
    assert!(stream.resident.contains(&key));
    let revision_before = stream.revisions.next_revision;

    stream
        .submit(
            3,
            request_block_entity_event_with_biome(
                0,
                2,
                block_entity_nbt_with_marker("Jukebox", position, 1),
            ),
        )
        .unwrap();
    complete_pending_decode_jobs(&mut stream);

    assert!(stream.store.sub_chunk(key).is_none());
    assert!(!stream.resident.contains(&key));
    assert!(stream.revisions.next_revision > revision_before);
    assert!(stream.pending_mesh.contains_key(&key));
    assert_eq!(stream.take_requests().len(), 1);
}

#[test]
fn request_mode_changed_backing_dirties_and_replaces_preserved_column() {
    let mut stream = block_entity_visual_stream();
    let position = [1, -63, 2];
    let key = SubChunkKey::new(0, 0, -4, 0);
    let initial = block_entity_nbt("Barrel", position);
    stream
        .submit(1, request_block_entity_event(0, initial.clone()))
        .unwrap();
    complete_pending_decode_jobs(&mut stream);
    assert_eq!(stream.take_requests().len(), 1);
    stream
        .submit(2, requested_block_entity_sub_chunk_event(0, 7_069, initial))
        .unwrap();
    complete_pending_decode_jobs(&mut stream);
    assert_eq!(stream.stats().adjudicated_static_block_entities, 1);
    let revision_before = stream.revisions.next_revision;
    let block_generation_before = stream.next_block_generation;

    let replacement = block_entity_nbt_with_marker("Barrel", position, 1);
    stream
        .submit(3, request_block_entity_event(0, replacement.clone()))
        .unwrap();
    complete_pending_decode_jobs(&mut stream);
    assert_eq!(stream.revisions.next_revision, revision_before);
    assert_eq!(stream.take_requests().len(), 1);
    stream
        .submit(
            4,
            requested_block_entity_sub_chunk_event(0, 7_070, replacement),
        )
        .unwrap();
    complete_pending_decode_jobs(&mut stream);

    assert!(stream.revisions.next_revision > revision_before);
    assert!(stream.next_block_generation > block_generation_before);
    assert!(stream.pending_mesh.contains_key(&key));
    assert_eq!(
        stream
            .store
            .sub_chunk(key)
            .and_then(|sub_chunk| sub_chunk.runtime_id(0, 1, 1, 2)),
        Some(7_070)
    );
    assert_eq!(stream.stats().adjudicated_static_block_entities, 1);
}
