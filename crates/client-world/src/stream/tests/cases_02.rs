use super::*;

#[test]
fn block_entity_visual_diagnostics_follow_request_eviction_and_dimension_lifecycle() {
    let mut stream = block_entity_visual_stream();
    let position = [1, -63, 2];
    let key = BlockEntityKey::new(0, position[0], position[1], position[2]);
    let mut request_payload = biome_payload(0, 1);
    request_payload.extend(block_entity_nbt("Jukebox", position));
    stream
        .submit(
            1,
            WorldEvent::LevelChunk(LevelChunkEvent {
                dimension: 0,
                x: 0,
                z: 0,
                mode: LevelChunkMode::LimitedRequests { highest: 1 },
                payload: request_payload,
            }),
        )
        .unwrap();
    complete_pending_decode_jobs(&mut stream);
    assert!(stream.store.block_entity(key).is_some());
    assert_eq!(stream.stats().adjudicated_logical_block_entities, 0);
    assert_eq!(stream.take_requests().len(), 1);

    let mut payload = vec![9, 1, (-4_i8) as u8, 1];
    payload.extend(zig_zag_i32(8_516));
    payload.extend(block_entity_nbt("Jukebox", position));
    stream
        .submit(
            2,
            WorldEvent::SubChunks(SubChunkBatchEvent {
                dimension: 0,
                entries: vec![SubChunkEntryEvent {
                    position: [0, -4, 0],
                    result: SubChunkResult::Success { payload },
                }],
            }),
        )
        .unwrap();
    complete_pending_decode_jobs(&mut stream);
    assert_eq!(stream.stats().adjudicated_logical_block_entities, 1);

    stream.evict_column(ChunkKey::new(0, 0, 0));
    assert_eq!(stream.stats().adjudicated_static_block_entities, 0);
    assert_eq!(stream.stats().adjudicated_logical_block_entities, 0);
    assert_eq!(stream.stats().deferred_block_entities, 0);
    assert_eq!(stream.stats().unknown_block_entities, 0);

    let position = [17, -63, 2];
    stream
        .submit(
            3,
            inline_block_entity_event(1, 7_069, block_entity_nbt("Barrel", position)),
        )
        .unwrap();
    complete_pending_decode_jobs(&mut stream);
    assert_eq!(stream.stats().adjudicated_static_block_entities, 1);
    let deferred_position = [33, -63, 2];
    stream
        .submit(
            4,
            inline_block_entity_event(2, 846, block_entity_nbt("Beacon", deferred_position)),
        )
        .unwrap();
    let unknown_position = [49, -63, 2];
    stream
        .submit(
            5,
            inline_block_entity_event(3, 1_936, block_entity_nbt("Barrel", unknown_position)),
        )
        .unwrap();
    complete_pending_decode_jobs(&mut stream);
    assert_eq!(stream.stats().deferred_block_entities, 1);
    assert_eq!(stream.stats().unknown_block_entities, 1);
    stream
        .submit(
            6,
            WorldEvent::ChangeDimension(ChangeDimensionEvent {
                dimension: 1,
                position: [0.0; 3],
            }),
        )
        .unwrap();
    assert_eq!(stream.stats().adjudicated_static_block_entities, 0);
    assert_eq!(stream.stats().adjudicated_logical_block_entities, 0);
    assert_eq!(stream.stats().deferred_block_entities, 0);
    assert_eq!(stream.stats().unknown_block_entities, 0);

    let replacement = block_entity_visual_stream();
    assert_eq!(replacement.stats().adjudicated_static_block_entities, 0);
    assert_eq!(replacement.stats().adjudicated_logical_block_entities, 0);
}

#[test]
fn live_block_entity_updates_decode_off_thread_and_commit_in_fifo() {
    let mut stream = WorldStream::new(WorldBootstrap {
        dimension: 0,
        local_player_runtime_id: 1,
        player_position: [0.0; 3],
        world_spawn_position: [0; 3],
        air_network_id: 12_530,
        block_network_ids_are_hashes: false,
    });
    stream.submit(1, inline_air_event(0)).unwrap();
    complete_pending_decode_jobs(&mut stream);

    let position = [1, -63, 2];
    let key = BlockEntityKey::new(0, position[0], position[1], position[2]);
    stream
        .submit(
            2,
            WorldEvent::BlockEntityUpdate(BlockEntityUpdateEvent {
                dimension: 0,
                position,
                nbt: block_entity_nbt("Chest", position),
            }),
        )
        .unwrap();
    let movement = MovePlayerEvent {
        runtime_id: 1,
        position: [1.0, 70.0, 2.0],
        pitch: 0.0,
        yaw: 0.0,
        ..Default::default()
    };
    stream.submit(3, WorldEvent::MovePlayer(movement)).unwrap();

    assert!(stream.store.block_entity(key).is_none());
    assert!(stream.take_committed_controls().is_empty());
    complete_pending_decode_jobs(&mut stream);
    assert_eq!(stream.store.block_entity(key).unwrap().id(), Some("Chest"));
    assert!(matches!(
        stream.take_committed_controls().as_slice(),
        [super::CommittedControlEvent::MovePlayer { sequence: 3, .. }]
    ));

    let before = stream.store.block_entity(key).unwrap();
    stream
        .submit(
            4,
            WorldEvent::BlockEntityUpdate(BlockEntityUpdateEvent {
                dimension: 0,
                position,
                nbt: vec![10, 0, 8],
            }),
        )
        .unwrap();
    complete_pending_decode_jobs(&mut stream);
    assert!(Arc::ptr_eq(
        &before,
        &stream.store.block_entity(key).unwrap()
    ));
    assert_eq!(stream.stats().decode_errors, 1);

    let invalid_position = [1, 320, 2];
    let invalid_key = BlockEntityKey::new(
        0,
        invalid_position[0],
        invalid_position[1],
        invalid_position[2],
    );
    stream
        .submit(
            5,
            WorldEvent::BlockEntityUpdate(BlockEntityUpdateEvent {
                dimension: 0,
                position: invalid_position,
                nbt: block_entity_nbt("Chest", invalid_position),
            }),
        )
        .unwrap();
    complete_pending_decode_jobs(&mut stream);
    assert!(stream.store.block_entity(invalid_key).is_none());
    assert_eq!(
        stream
            .stats()
            .normalization_reasons
            .invalid_block_entity_positions,
        1
    );
}

#[test]
fn inline_and_sub_chunk_ingestion_commit_sparse_block_entity_tails() {
    let bootstrap = WorldBootstrap {
        dimension: 0,
        local_player_runtime_id: 1,
        player_position: [0.0; 3],
        world_spawn_position: [0; 3],
        air_network_id: 12_530,
        block_network_ids_are_hashes: false,
    };
    let inline_position = [1, -63, 2];
    let inline_key = BlockEntityKey::new(
        0,
        inline_position[0],
        inline_position[1],
        inline_position[2],
    );
    let mut inline_payload = vec![9, 0, (-4_i8) as u8];
    inline_payload.extend(biome_payload(0, 1));
    inline_payload.extend(block_entity_nbt("Chest", inline_position));
    let mut stream = WorldStream::new(bootstrap);
    stream
        .submit(
            1,
            WorldEvent::LevelChunk(LevelChunkEvent {
                dimension: 0,
                x: 0,
                z: 0,
                mode: LevelChunkMode::Inline { count: 1 },
                payload: inline_payload,
            }),
        )
        .unwrap();
    complete_pending_decode_jobs(&mut stream);
    assert_eq!(
        stream.store.block_entity(inline_key).unwrap().id(),
        Some("Chest")
    );

    let request_position = [5, -47, 6];
    let request_key = BlockEntityKey::new(
        0,
        request_position[0],
        request_position[1],
        request_position[2],
    );
    let mut request_payload = biome_payload(0, 2);
    request_payload.extend(block_entity_nbt("Note", request_position));
    let mut stream = WorldStream::new(bootstrap);
    stream
        .submit(
            1,
            WorldEvent::LevelChunk(LevelChunkEvent {
                dimension: 0,
                x: 0,
                z: 0,
                mode: LevelChunkMode::LimitedRequests { highest: 1 },
                payload: request_payload,
            }),
        )
        .unwrap();
    complete_pending_decode_jobs(&mut stream);
    assert_eq!(
        stream.store.block_entity(request_key).unwrap().id(),
        Some("Note")
    );

    let (mut stream, sub_chunk) = stream_with_one_expected_sub_chunk();
    let sub_chunk_position = [3, -63, 4];
    let sub_chunk_entity = BlockEntityKey::new(
        0,
        sub_chunk_position[0],
        sub_chunk_position[1],
        sub_chunk_position[2],
    );
    let mut payload = vec![9, 1, (-4_i8) as u8, 1];
    payload.extend(zig_zag_i32(7));
    payload.extend(block_entity_nbt("Sign", sub_chunk_position));
    let mut prepared = super::prepare_sub_chunks(SubChunkBatchEvent {
        dimension: 0,
        entries: vec![SubChunkEntryEvent {
            position: [sub_chunk.x, sub_chunk.y, sub_chunk.z],
            result: SubChunkResult::Success { payload },
        }],
    });
    apply_sub_chunk_result(&mut stream, sub_chunk, prepared.remove(0).result);
    assert_eq!(
        stream.store.block_entity(sub_chunk_entity).unwrap().id(),
        Some("Sign")
    );
}

#[test]
fn clock_daylight_and_weather_commit_in_fifo_order_without_dirtying_world_meshes() {
    let mut stream = WorldStream::new(WorldBootstrap {
        dimension: 0,
        local_player_runtime_id: 1,
        player_position: [0.0; 3],
        world_spawn_position: [0; 3],
        air_network_id: 12_530,
        block_network_ids_are_hashes: false,
    });
    let pending_mesh_count = stream.pending_mesh.len();
    let mesh_change_count = stream.mesh_changes.len();
    let connectivity_generation = stream.connectivity_generation;

    stream
        .submit(1, WorldEvent::SetTime(SetTimeEvent { time: i32::MIN }))
        .unwrap();
    stream
        .submit(
            2,
            WorldEvent::DaylightCycle(DaylightCycleUpdateEvent { enabled: false }),
        )
        .unwrap();
    stream
        .submit(
            3,
            WorldEvent::Weather(WeatherUpdateEvent {
                channel: WeatherChannel::Rain,
                level: 1.0,
            }),
        )
        .unwrap();
    stream
        .submit(
            4,
            WorldEvent::Weather(WeatherUpdateEvent {
                channel: WeatherChannel::Lightning,
                level: 0.0,
            }),
        )
        .unwrap();

    assert_eq!(
        stream.take_committed_controls(),
        vec![
            super::CommittedControlEvent::SetTime {
                sequence: 1,
                update: SetTimeEvent { time: i32::MIN },
            },
            super::CommittedControlEvent::DaylightCycle {
                sequence: 2,
                update: DaylightCycleUpdateEvent { enabled: false },
            },
            super::CommittedControlEvent::Weather {
                sequence: 3,
                update: WeatherUpdateEvent {
                    channel: WeatherChannel::Rain,
                    level: 1.0,
                },
            },
            super::CommittedControlEvent::Weather {
                sequence: 4,
                update: WeatherUpdateEvent {
                    channel: WeatherChannel::Lightning,
                    level: 0.0,
                },
            },
        ]
    );
    assert_eq!(stream.pending_mesh.len(), pending_mesh_count);
    assert_eq!(stream.mesh_changes.len(), mesh_change_count);
    assert_eq!(stream.connectivity_generation, connectivity_generation);
}

#[test]
fn clock_and_weather_wait_behind_an_earlier_decode_before_committing() {
    let mut stream = WorldStream::new(WorldBootstrap {
        dimension: 0,
        local_player_runtime_id: 1,
        player_position: [0.0; 3],
        world_spawn_position: [0; 3],
        air_network_id: 12_530,
        block_network_ids_are_hashes: false,
    });
    stream.submit(1, inline_air_event(0)).unwrap();
    stream
        .submit(2, WorldEvent::SetTime(SetTimeEvent { time: -1 }))
        .unwrap();
    stream
        .submit(
            3,
            WorldEvent::Weather(WeatherUpdateEvent {
                channel: WeatherChannel::Rain,
                level: 0.25,
            }),
        )
        .unwrap();

    assert!(stream.take_committed_controls().is_empty());
    complete_pending_decode_jobs(&mut stream);
    assert_eq!(
        stream.take_committed_controls(),
        vec![
            super::CommittedControlEvent::SetTime {
                sequence: 2,
                update: SetTimeEvent { time: -1 },
            },
            super::CommittedControlEvent::Weather {
                sequence: 3,
                update: WeatherUpdateEvent {
                    channel: WeatherChannel::Rain,
                    level: 0.25,
                },
            },
        ]
    );
}

#[test]
fn inline_level_chunk_decodes_full_dimension_biomes_independent_of_block_count() {
    let mut stream = WorldStream::new(WorldBootstrap {
        dimension: 0,
        local_player_runtime_id: 1,
        player_position: [0.0; 3],
        world_spawn_position: [0; 3],
        air_network_id: 12_530,
        block_network_ids_are_hashes: false,
    });
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
            .store
            .biome_id(SubChunkKey::new(0, 0, -4, 0), 0, 0, 0),
        Some(7)
    );
    assert_eq!(
        stream
            .store
            .biome_id(SubChunkKey::new(0, 0, 19, 0), 0, 0, 0),
        Some(7)
    );
}

#[test]
fn request_level_chunk_decodes_biomes_before_enqueuing_sub_chunk_requests() {
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
            request_level_chunk_event(0, 0, 0, LevelChunkMode::LimitedRequests { highest: 1 }, 9),
        )
        .unwrap();

    assert_eq!(stream.pending_decode.len(), 1);
    assert!(stream.take_requests().is_empty());

    complete_pending_decode_jobs(&mut stream);

    assert_eq!(
        stream
            .store
            .biome_id(SubChunkKey::new(0, 0, -4, 0), 0, 0, 0),
        Some(9)
    );
    assert_eq!(
        stream
            .store
            .biome_id(SubChunkKey::new(0, 0, 19, 0), 0, 0, 0),
        Some(9)
    );
    assert_eq!(stream.take_requests().len(), 1);
}

#[test]
fn request_mode_biome_arrival_dirties_diagonal_cross_chunk_blend_dependents() {
    let mut stream = WorldStream::new(WorldBootstrap {
        dimension: 0,
        local_player_runtime_id: 1,
        player_position: [0.0; 3],
        world_spawn_position: [0; 3],
        air_network_id: 12_530,
        block_network_ids_are_hashes: false,
    });
    let neighbour = SubChunkKey::new(0, 1, -4, 1);
    stream
        .store
        .commit_level_chunk(
            neighbour.chunk(),
            DecodedLevelChunk::decode(
                -4,
                1,
                include_bytes!(concat!(
                    env!("CARGO_MANIFEST_DIR"),
                    "/../world/fixtures/uniform_non_air.bin"
                )),
            )
            .unwrap(),
        )
        .unwrap();
    stream.resident.insert(neighbour);
    let generation = stream.mark_dirty_exact(neighbour, Instant::now());
    assert!(stream.register_mesh_dependency_mask(
        neighbour,
        generation,
        MeshDependencyMask::default(),
    ));
    stream.pending_mesh.clear();

    stream
        .submit(
            1,
            request_level_chunk_event(0, 0, 0, LevelChunkMode::LimitedRequests { highest: 1 }, 9),
        )
        .unwrap();
    complete_pending_decode_jobs(&mut stream);

    assert!(
        stream.pending_mesh.contains_key(&neighbour),
        "a newly committed biome column must invalidate a diagonal tint halo"
    );
}

#[test]
fn inline_biome_replacement_dirties_diagonal_cross_chunk_blend_dependents() {
    let mut stream = WorldStream::new(WorldBootstrap {
        dimension: 0,
        local_player_runtime_id: 1,
        player_position: [0.0; 3],
        world_spawn_position: [0; 3],
        air_network_id: 12_530,
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
        .store
        .commit_level_chunk(
            source.chunk(),
            DecodedLevelChunk::decode_with_biomes(
                -4,
                1,
                -4,
                protocol::vanilla_dimension_range(0)
                    .unwrap()
                    .sub_chunk_count,
                &original_payload,
            )
            .unwrap(),
        )
        .unwrap();
    stream
        .store
        .commit_level_chunk(
            diagonal.chunk(),
            DecodedLevelChunk::decode(-4, 1, block_payload).unwrap(),
        )
        .unwrap();
    stream.resident.extend([source, diagonal]);
    let generation = stream.mark_dirty_exact(diagonal, Instant::now());
    assert!(stream.register_mesh_dependency_mask(
        diagonal,
        generation,
        MeshDependencyMask::default(),
    ));
    stream.pending_mesh.clear();

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
        stream.pending_mesh.contains_key(&diagonal),
        "an inline biome replacement must invalidate a diagonal tint halo independently of AO"
    );
}

#[test]
fn evicting_a_diagonal_biome_only_column_dirties_the_remaining_blend_boundary() {
    let mut stream = WorldStream::new(WorldBootstrap {
        dimension: 0,
        local_player_runtime_id: 1,
        player_position: [0.0; 3],
        world_spawn_position: [0; 3],
        air_network_id: 12_530,
        block_network_ids_are_hashes: false,
    });
    let center = SubChunkKey::new(0, 0, -4, 0);
    stream
        .store
        .commit_level_chunk(
            center.chunk(),
            DecodedLevelChunk::decode(
                -4,
                1,
                include_bytes!(concat!(
                    env!("CARGO_MANIFEST_DIR"),
                    "/../world/fixtures/uniform_non_air.bin"
                )),
            )
            .unwrap(),
        )
        .unwrap();
    stream.store.commit_biome_column(
        ChunkKey::new(0, 1, 1),
        DecodedBiomeColumn::decode(-4, 1, &[1, 84]).unwrap(),
    );
    stream.resident.insert(center);
    let generation = stream.mark_dirty_exact(center, Instant::now());
    assert!(stream.register_mesh_dependency_mask(
        center,
        generation,
        MeshDependencyMask::default(),
    ));
    stream.pending_mesh.clear();

    stream.evict_column(ChunkKey::new(0, 1, 1));

    assert!(stream.pending_mesh.contains_key(&center));
}

#[test]
fn limited_requests_track_omitted_upper_air_and_replace_the_column_atomically() {
    let mut stream = WorldStream::new(WorldBootstrap {
        dimension: 0,
        local_player_runtime_id: 1,
        player_position: [0.0; 3],
        world_spawn_position: [0; 3],
        air_network_id: 12_530,
        block_network_ids_are_hashes: false,
    });
    let chunk = ChunkKey::new(0, 0, 0);

    stream
        .submit(
            1,
            request_level_chunk_event(0, 0, 0, LevelChunkMode::LimitedRequests { highest: 1 }, 1),
        )
        .unwrap();
    complete_pending_decode_jobs(&mut stream);
    let requests = stream.take_requests();
    assert_eq!(requests.len(), 1);
    assert_eq!(requests[0].base_sub_chunk_y, -4);
    assert_eq!(requests[0].count, 1);
    assert!(
        !stream
            .known_air
            .contains(&SubChunkKey::from_chunk(chunk, -4))
    );
    for y in -3..=19 {
        let key = SubChunkKey::from_chunk(chunk, y);
        assert!(stream.resident.contains(&key), "missing resident air y={y}");
        assert!(stream.known_air.contains(&key), "missing known air y={y}");
        assert!(
            stream.pending_light.contains_key(&key),
            "unscheduled light y={y}"
        );
    }

    stream
        .submit(
            2,
            request_level_chunk_event(0, 0, 0, LevelChunkMode::LimitedRequests { highest: 0 }, 2),
        )
        .unwrap();
    complete_pending_decode_jobs(&mut stream);
    assert!(stream.take_requests().is_empty());
    assert!(stream.loaded_columns.contains(&chunk));
    for y in -4..=19 {
        assert!(
            stream
                .known_air
                .contains(&SubChunkKey::from_chunk(chunk, y)),
            "empty replacement lost authoritative air y={y}"
        );
    }

    stream
        .submit(
            3,
            request_level_chunk_event(0, 0, 0, LevelChunkMode::LimitedRequests { highest: 1 }, 3),
        )
        .unwrap();
    complete_pending_decode_jobs(&mut stream);
    assert_eq!(stream.take_requests().len(), 1);
    let requested = SubChunkKey::from_chunk(chunk, -4);
    assert!(!stream.resident.contains(&requested));
    assert!(!stream.known_air.contains(&requested));
    assert!(stream.is_expected_sub_chunk(requested));
    for y in -3..=19 {
        assert!(
            stream
                .known_air
                .contains(&SubChunkKey::from_chunk(chunk, y)),
            "filled replacement lost implicit upper air y={y}"
        );
    }
}

#[test]
fn malformed_request_level_chunk_neither_commits_nor_enqueues() {
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

    assert_eq!(stream.stats().decode_errors, 1);
    assert!(stream.take_requests().is_empty());
    assert_eq!(
        stream
            .store
            .biome_id(SubChunkKey::new(0, 0, -4, 0), 0, 0, 0),
        Some(5)
    );
    assert_eq!(
        stream
            .store
            .biome_id(SubChunkKey::new(0, 0, 19, 0), 0, 0, 0),
        Some(5)
    );
}

#[test]
fn bootstrap_non_finite_horizontal_position_uses_the_shared_finite_scope_anchor() {
    let bootstrap = WorldBootstrap {
        dimension: 0,
        local_player_runtime_id: 1,
        player_position: [f32::NAN, 80.0, f32::INFINITY],
        world_spawn_position: [0; 3],
        air_network_id: 12_530,
        block_network_ids_are_hashes: false,
    };
    let expected = super::server_position::resolve_server_position(
        bootstrap.player_position,
        [0.0, super::server_position::SAFE_SERVER_HEIGHT, 0.0],
        None,
    );

    let stream = WorldStream::new(bootstrap);

    assert_eq!(stream.resolved_server_position(), expected);
    let anchor = expected.surface_anchor.unwrap();
    assert!(stream.column_is_active(ChunkKey::new(
        0,
        anchor[0].div_euclid(16),
        anchor[1].div_euclid(16),
    )));
}

#[test]
fn change_dimension_non_finite_horizontal_position_keeps_camera_and_scope_together() {
    let mut stream = WorldStream::new(WorldBootstrap {
        dimension: 0,
        local_player_runtime_id: 1,
        player_position: [7.25, 70.0, -8.75],
        world_spawn_position: [0; 3],
        air_network_id: 12_530,
        block_network_ids_are_hashes: false,
    });
    let change = ChangeDimensionEvent {
        dimension: 1,
        position: [f32::NAN, 32_000.0, f32::INFINITY],
    };
    let expected = super::server_position::resolve_server_position(
        change.position,
        stream.resolved_server_position().position,
        stream.resolved_server_position().surface_anchor,
    );

    stream
        .submit(1, WorldEvent::ChangeDimension(change))
        .unwrap();

    assert_eq!(stream.resolved_server_position(), expected);
    let anchor = expected.surface_anchor.unwrap();
    assert!(stream.column_is_active(ChunkKey::new(
        1,
        anchor[0].div_euclid(16),
        anchor[1].div_euclid(16),
    )));
    assert!(matches!(
        stream.take_committed_controls().as_slice(),
        [super::CommittedControlEvent::ChangeDimension { resolved, .. }]
            if *resolved == expected
    ));
}

#[test]
fn newer_inline_chunk_is_validated_after_fifo_blocked_publisher_update_commits() {
    let mut stream = WorldStream::new(WorldBootstrap {
        dimension: 0,
        local_player_runtime_id: 1,
        player_position: [0.0; 3],
        world_spawn_position: [0; 3],
        air_network_id: 12_530,
        block_network_ids_are_hashes: false,
    });

    stream.submit(1, inline_air_event(0)).unwrap();
    stream
        .submit(
            2,
            WorldEvent::PublisherUpdate(PublisherUpdateEvent {
                center: [1_600, 64, 0],
                radius_blocks: 256,
            }),
        )
        .unwrap();
    stream.submit(3, inline_air_event(100)).unwrap();

    assert_eq!(stream.pending_decode.len(), 2);
    complete_pending_decode_jobs(&mut stream);

    let key = SubChunkKey::new(0, 100, -4, 0);
    assert_eq!(stream.publisher_center, Some([1_600, 64, 0]));
    assert!(stream.resident.contains(&key) || stream.known_air.contains(&key));
}

#[test]
fn equal_loaded_count_with_missing_target_and_source_replacement_is_not_exact() {
    let mut stream = WorldStream::new(WorldBootstrap {
        dimension: 0,
        local_player_runtime_id: 1,
        player_position: [0.0; 3],
        world_spawn_position: [0; 3],
        air_network_id: 12_530,
        block_network_ids_are_hashes: false,
    });
    let target = super::ViewCohort {
        dimension: 0,
        center: [10, 10],
        radius: 1,
    };
    assert_eq!(
        super::ViewCohort {
            dimension: 0,
            center: [0, 0],
            radius: 16,
        }
        .expected_columns()
        .len(),
        749
    );
    stream
        .submit(
            1,
            WorldEvent::PublisherUpdate(PublisherUpdateEvent {
                center: [160, 64, 160],
                radius_blocks: 16,
            }),
        )
        .unwrap();
    let target_columns = target.expected_columns();
    let missing = *target_columns.last().unwrap();
    let source = ChunkKey::new(0, 0, 0);
    stream.loaded_columns.insert(source);
    stream.capture_source_columns();
    stream.loaded_columns = target_columns
        .iter()
        .copied()
        .filter(|column| *column != missing)
        .collect();
    stream.loaded_columns.insert(source);

    let status = stream.cohort_status(target);

    assert_eq!(status.expected, 1);
    assert_eq!(status.loaded_target, 0);
    assert_eq!(status.missing_target, 1);
    assert_eq!(status.foreign_loaded, 1);
    assert_eq!(status.source_leftover, 1);
    assert!(!status.is_exact());

    stream.loaded_columns.remove(&source);
    stream.loaded_columns.insert(missing);

    assert!(stream.cohort_status(target).is_exact());
}
