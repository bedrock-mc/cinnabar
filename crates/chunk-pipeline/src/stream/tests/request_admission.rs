use super::*;

#[test]
fn limited_requests_track_omitted_upper_air_and_replace_the_column_atomically() {
    let mut stream = WorldStream::new(WorldBootstrap {
        local_player_unique_id: 1,
        dimension: 0,
        local_player_runtime_id: 1,
        player_position: [0.0; 3],
        world_spawn_position: [0; 3],
        air_network_id: protocol_ids::SEQUENTIAL_AIR_NETWORK_ID,
        block_network_ids_are_hashes: false,
    });
    define_custom_biomes(&mut stream, [1, 2, 3]);
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
            stream.lighting.jobs.pending.contains_key(&key),
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
    assert!(stream.requests.is_expected(requested));
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
fn request_level_chunk_joins_required_cohort_only_after_decode_and_admission() {
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
            WorldEvent::PublisherUpdate(PublisherUpdateEvent {
                center: [0, 64, 0],
                radius_blocks: 16,
            }),
        )
        .unwrap();
    let key = ChunkKey::new(0, 0, 0);

    stream
        .submit(
            2,
            request_level_chunk_event(
                key.dimension,
                key.x,
                key.z,
                LevelChunkMode::LimitedRequests { highest: 1 },
                7,
            ),
        )
        .unwrap();

    assert!(!stream.required_columns().contains(&key));
    complete_pending_decode_jobs(&mut stream);

    assert!(stream.required_columns().contains(&key));
    assert_eq!(stream.take_requests().len(), 1);
}

#[test]
fn newer_subchunk_is_validated_after_fifo_blocked_dimension_change_commits() {
    let mut stream = WorldStream::new(WorldBootstrap {
        local_player_unique_id: 1,
        dimension: 0,
        local_player_runtime_id: 1,
        player_position: [0.0; 3],
        world_spawn_position: [0; 3],
        air_network_id: protocol_ids::SEQUENTIAL_AIR_NETWORK_ID,
        block_network_ids_are_hashes: false,
    });

    stream.submit(1, inline_air_event(0)).unwrap();
    stream
        .submit(
            2,
            WorldEvent::ChangeDimension(ChangeDimensionEvent {
                dimension: 1,
                position: [1_600.0, 80.0, 0.0],
                ..Default::default()
            }),
        )
        .unwrap();
    stream
        .submit(
            3,
            WorldEvent::LevelChunk(LevelChunkEvent {
                dimension: 1,
                x: 100,
                z: 0,
                mode: LevelChunkMode::LimitedRequests { highest: 1 },
                payload: biome_payload(1, 1),
            }),
        )
        .unwrap();
    stream
        .submit(
            4,
            WorldEvent::SubChunks(SubChunkBatchEvent {
                dimension: 1,
                entries: vec![SubChunkEntryEvent {
                    diagnostics: None,
                    position: [100, 0, 0],
                    result: SubChunkResult::AllAir,
                }],
            }),
        )
        .unwrap();

    assert_eq!(stream.pending_decode.len(), 3);
    complete_pending_decode_jobs(&mut stream);

    let key = SubChunkKey::new(1, 100, 0, 0);
    assert_eq!(stream.current_dimension(), 1);
    assert!(stream.known_air.contains(&key));
    assert!(stream.loaded_columns.contains(&key.chunk()));
}

#[test]
fn deferred_request_events_reserve_outbound_capacity_at_admission() {
    let mut stream = WorldStream::new(WorldBootstrap {
        local_player_unique_id: 1,
        dimension: 0,
        local_player_runtime_id: 1,
        player_position: [0.0; 3],
        world_spawn_position: [0; 3],
        air_network_id: protocol_ids::SEQUENTIAL_AIR_NETWORK_ID,
        block_network_ids_are_hashes: false,
    });

    for sequence in 1..=62_u64 {
        let index = sequence as i32 - 1;
        stream
            .submit(
                sequence,
                WorldEvent::LevelChunk(LevelChunkEvent {
                    dimension: 0,
                    x: index.rem_euclid(9) - 4,
                    z: index.div_euclid(9) - 4,
                    mode: LevelChunkMode::LimitedRequests { highest: 1 },
                    payload: biome_payload(0, 1),
                }),
            )
            .unwrap();
        if sequence == 32 || sequence == 62 {
            complete_pending_decode_jobs(&mut stream);
        }
    }
    // Keep the FIFO blocker on a column that does not supersede one of
    // the 62 queued request-mode columns under test.
    stream.submit(63, inline_air_event(8)).unwrap();
    for (sequence, x) in [(64, 10), (65, 11)] {
        stream
            .submit(
                sequence,
                WorldEvent::LevelChunk(LevelChunkEvent {
                    dimension: 0,
                    x,
                    z: 1,
                    mode: LevelChunkMode::LimitedRequests { highest: 1 },
                    payload: biome_payload(0, 1),
                }),
            )
            .unwrap();
    }

    // Reservations fill the outbound FIFO, so the next request-creating column defers.
    stream
        .submit(
            66,
            WorldEvent::LevelChunk(LevelChunkEvent {
                dimension: 0,
                x: 12,
                z: 10,
                mode: LevelChunkMode::LimitedRequests { highest: 1 },
                payload: biome_payload(0, 1),
            }),
        )
        .unwrap();
    assert_eq!(stream.order.deferred_count(), 1);
    assert_eq!(stream.pending_request_count(), 62);

    complete_pending_decode_jobs(&mut stream);
    assert_eq!(
        stream.pending_request_count(),
        super::OUTBOUND_REQUEST_CAPACITY
    );
    assert_eq!(stream.order.deferred_count(), 1);
}

#[test]
fn heavy_admission_is_bounded_before_rayon_and_retained_work_never_exceeds_constants() {
    let mut stream = WorldStream::new(WorldBootstrap {
        local_player_unique_id: 1,
        dimension: 0,
        local_player_runtime_id: 1,
        player_position: [0.0; 3],
        world_spawn_position: [0; 3],
        air_network_id: protocol_ids::SEQUENTIAL_AIR_NETWORK_ID,
        block_network_ids_are_hashes: false,
    });

    for sequence in 1..=super::MAX_ADMITTED_HEAVY_EVENTS as u64 {
        stream
            .submit(sequence, inline_air_event(sequence as i32))
            .unwrap();
    }
    let stats = stream.stats();
    assert_eq!(
        stats.admitted_heavy_events,
        super::MAX_ADMITTED_HEAVY_EVENTS
    );
    assert_eq!(stats.queued_decode_jobs, super::MAX_ADMITTED_HEAVY_EVENTS);
    assert_eq!(stats.in_flight_decode_jobs, 0);
    assert!(
        stats.queued_decode_jobs + stats.in_flight_decode_jobs + stats.completed_decode_results
            <= super::MAX_ADMITTED_HEAVY_EVENTS
    );

    // Terrain past heavy admission is ordered without decode work, up to its own bound.
    let heavy = super::MAX_ADMITTED_HEAVY_EVENTS as u64;
    let deferred = client_world::ingestion::MAX_DEFERRED_WORLD_EVENTS as u64;
    for sequence in heavy + 1..=heavy + deferred {
        stream
            .submit(sequence, inline_air_event(1000 + sequence as i32))
            .unwrap();
    }
    assert_eq!(
        stream.stats().queued_decode_jobs,
        super::MAX_ADMITTED_HEAVY_EVENTS
    );
    assert_eq!(stream.remaining_admission_capacity(), 0);
    let error = stream
        .submit(heavy + deferred + 1, inline_air_event(999))
        .unwrap_err();
    assert!(matches!(
        error,
        super::WorldStreamError::AdmissionFull { .. }
    ));
}

#[test]
fn one_dispatch_drains_the_entire_bounded_heavy_admission_window() {
    let mut stream = WorldStream::new(WorldBootstrap {
        local_player_unique_id: 1,
        dimension: 0,
        local_player_runtime_id: 1,
        player_position: [0.0; 3],
        world_spawn_position: [0; 3],
        air_network_id: protocol_ids::SEQUENTIAL_AIR_NETWORK_ID,
        block_network_ids_are_hashes: false,
    });

    for sequence in 1..=super::MAX_ADMITTED_HEAVY_EVENTS as u64 {
        stream
            .submit(sequence, inline_air_event(sequence as i32))
            .unwrap();
    }

    stream.dispatch_decode_jobs();

    assert!(stream.pending_decode.is_empty());
    assert_eq!(
        stream.in_flight_decode_jobs,
        super::MAX_ADMITTED_HEAVY_EVENTS
    );
}

#[test]
fn eviction_purges_unsent_requests_and_late_subchunks_cannot_resurrect_the_column() {
    let mut stream = WorldStream::new(WorldBootstrap {
        local_player_unique_id: 1,
        dimension: 0,
        local_player_runtime_id: 1,
        player_position: [0.0; 3],
        world_spawn_position: [0; 3],
        air_network_id: protocol_ids::SEQUENTIAL_AIR_NETWORK_ID,
        block_network_ids_are_hashes: false,
    });
    let chunk = ChunkKey::new(0, 1, 0);
    let key = SubChunkKey::from_chunk(chunk, -4);
    stream
        .submit(
            1,
            WorldEvent::LevelChunk(LevelChunkEvent {
                dimension: 0,
                x: chunk.x,
                z: chunk.z,
                mode: LevelChunkMode::LimitedRequests { highest: 1 },
                payload: biome_payload(0, 1),
            }),
        )
        .unwrap();
    complete_pending_decode_jobs(&mut stream);
    assert_eq!(stream.requests.queue.len(), 1);

    stream.evict_column(chunk);
    assert!(stream.requests.queue.is_empty());
    stream
        .submit(
            2,
            WorldEvent::SubChunks(SubChunkBatchEvent {
                dimension: 0,
                entries: vec![SubChunkEntryEvent {
                    diagnostics: None,
                    position: [key.x, key.y, key.z],
                    result: SubChunkResult::AllAir,
                }],
            }),
        )
        .unwrap();
    assert_eq!(stream.stats().queued_decode_jobs, 1);
    complete_pending_decode_jobs(&mut stream);
    assert!(!stream.resident.contains(&key));
    assert!(stream.authority.terrain().sub_chunk(key).is_none());
}

#[test]
fn valid_late_inactive_subchunk_reply_records_stale_without_world_side_effects() {
    let mut stream = WorldStream::new(WorldBootstrap {
        local_player_unique_id: 1,
        dimension: 0,
        local_player_runtime_id: 1,
        player_position: [0.0; 3],
        world_spawn_position: [0; 3],
        air_network_id: protocol_ids::SEQUENTIAL_AIR_NETWORK_ID,
        block_network_ids_are_hashes: false,
    });
    let chunk = ChunkKey::new(0, 1, 0);
    let key = SubChunkKey::from_chunk(chunk, -4);
    stream
        .submit(
            1,
            WorldEvent::LevelChunk(LevelChunkEvent {
                dimension: 0,
                x: chunk.x,
                z: chunk.z,
                mode: LevelChunkMode::LimitedRequests { highest: 1 },
                payload: biome_payload(0, 1),
            }),
        )
        .unwrap();
    complete_pending_decode_jobs(&mut stream);
    assert!(stream.column_is_active(chunk));
    assert_eq!(stream.take_requests().len(), 1);
    assert!(stream.requests.requested.contains_key(&chunk));

    stream
        .submit(
            2,
            WorldEvent::PublisherUpdate(PublisherUpdateEvent {
                center: [1_600, 64, 0],
                radius_blocks: 0,
            }),
        )
        .unwrap();
    assert!(!stream.column_is_active(chunk));
    assert!(stream.authority.terrain().sub_chunk(key).is_none());

    let resident_before = stream.resident.clone();
    let known_air_before = stream.known_air.clone();
    let loaded_columns_before = stream.loaded_columns.clone();
    let requested_sub_chunks_before = stream.requests.requested.clone();
    let sub_chunk_deadlines_before = stream.requests.deadlines.clone();
    let deferred_retries_before = stream.requests.deferred_retries.clone();
    let deferred_retry_set_before = stream.requests.deferred_retry_set.clone();
    let requests_before = stream.requests.queue.len();
    let stats_before = stream.stats();

    apply_sub_chunk_result(&mut stream, key, super::PreparedSubChunkResult::AllAir);

    assert!(stream.authority.terrain().sub_chunk(key).is_none());
    assert_eq!(stream.resident, resident_before);
    assert_eq!(stream.known_air, known_air_before);
    assert_eq!(stream.loaded_columns, loaded_columns_before);
    assert_eq!(stream.requests.requested, requested_sub_chunks_before);
    assert_eq!(stream.requests.deadlines, sub_chunk_deadlines_before);
    assert_eq!(stream.requests.deferred_retries, deferred_retries_before);
    assert_eq!(
        stream.requests.deferred_retry_set,
        deferred_retry_set_before
    );
    assert_eq!(stream.requests.queue.len(), requests_before);
    let mut expected_stats = stats_before;
    expected_stats.publication_outcomes.stale =
        expected_stats.publication_outcomes.stale.saturating_add(1);
    assert_eq!(stream.stats(), expected_stats);
    assert_eq!(stream.stats().normalization_reasons.inactive_sub_chunks, 0);
}

#[test]
fn old_dimension_and_out_of_radius_chunks_are_rejected_and_radii_are_clamped() {
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
        .submit(1, WorldEvent::ChunkRadiusUpdated(999))
        .unwrap();
    stream
        .submit(
            2,
            WorldEvent::PublisherUpdate(PublisherUpdateEvent {
                center: [0, 64, 0],
                radius_blocks: u32::MAX,
            }),
        )
        .unwrap();
    assert_eq!(stream.chunk_radius, Some(super::MAX_VIEW_RADIUS_CHUNKS));
    assert_eq!(
        stream.publisher.radius_chunks,
        Some(super::MAX_VIEW_RADIUS_CHUNKS)
    );
    let stats = format!("{:?}", stream.stats());
    assert!(
        stats.contains(&format!(
            "received_radius_chunks: Some({})",
            super::MAX_VIEW_RADIUS_CHUNKS
        )),
        "{stats}"
    );
    assert!(
        stats.contains(&format!(
            "publisher_radius_chunks: Some({})",
            super::MAX_VIEW_RADIUS_CHUNKS
        )),
        "{stats}"
    );

    stream
        .submit(
            3,
            WorldEvent::LevelChunk(LevelChunkEvent {
                dimension: 0,
                // The confirmed player grid intentionally retains through
                // radius + 2, independently of publisher control scope.
                x: super::MAX_VIEW_RADIUS_CHUNKS + 3,
                z: 0,
                mode: LevelChunkMode::LimitlessRequests,
                payload: biome_payload(0, 1),
            }),
        )
        .unwrap();
    complete_pending_decode_jobs(&mut stream);
    assert!(stream.requests.queue.is_empty());

    stream
        .submit(
            4,
            WorldEvent::ChangeDimension(ChangeDimensionEvent {
                dimension: 1,
                position: [0.0, 80.0, 0.0],
                ..Default::default()
            }),
        )
        .unwrap();
    stream.submit(5, inline_air_event(0)).unwrap();
    assert_eq!(stream.current_dimension(), 1);
    assert_eq!(stream.stats().queued_decode_jobs, 1);
    complete_pending_decode_jobs(&mut stream);
    assert_eq!(stream.stats().queued_decode_jobs, 0);
}

#[test]
fn subchunk_admission_requires_the_exact_expected_dimension_column_and_y() {
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
            WorldEvent::LevelChunk(LevelChunkEvent {
                dimension: 0,
                x: 0,
                z: 0,
                mode: LevelChunkMode::LimitedRequests { highest: 1 },
                payload: biome_payload(0, 1),
            }),
        )
        .unwrap();
    stream
        .submit(
            2,
            WorldEvent::SubChunks(SubChunkBatchEvent {
                dimension: 0,
                entries: vec![SubChunkEntryEvent {
                    diagnostics: None,
                    position: [0, -3, 0],
                    result: SubChunkResult::AllAir,
                }],
            }),
        )
        .unwrap();

    assert_eq!(stream.stats().queued_decode_jobs, 2);
    complete_pending_decode_jobs(&mut stream);
    assert_eq!(stream.stats().queued_decode_jobs, 0);
    let implicit_air = SubChunkKey::new(0, 0, -3, 0);
    assert!(stream.resident.contains(&implicit_air));
    assert!(stream.known_air.contains(&implicit_air));
    assert!(stream.authority.terrain().sub_chunk(implicit_air).is_none());
    assert_eq!(
        stream.requests.requested[&ChunkKey::new(0, 0, 0)]
            .keys()
            .copied()
            .collect::<BTreeSet<_>>(),
        BTreeSet::from([-4])
    );
}
