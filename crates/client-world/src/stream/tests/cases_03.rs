use super::*;

#[test]
fn publisher_required_disk_is_distinct_from_square_prefetch_scope() {
    let cohort = super::ViewCohort {
        dimension: 0,
        center: [10, -10],
        radius: 1,
        publisher_geometry: None,
    };

    assert_eq!(cohort.classifier_columns().len(), 5);
    assert!(
        cohort
            .classifier_columns()
            .contains(&ChunkKey::new(0, 11, -10))
    );
    assert!(
        !cohort
            .classifier_columns()
            .contains(&ChunkKey::new(0, 11, -9))
    );
    assert!(cohort.contains_column(0, [11, -9]));
    assert!(!cohort.contains_column(0, [12, -10]));
}

#[test]
fn publisher_block_position_and_radius_use_euclidean_chunk_conversion() {
    let cases = [
        ([-1, 64, -1], 0, [-1, -1], 0),
        ([-16, 64, -16], 1, [-1, -1], 1),
        ([-17, 64, -17], 15, [-2, -2], 1),
        ([15, 64, 15], 16, [0, 0], 1),
        ([16, 64, 16], 17, [1, 1], 2),
    ];

    for (center, radius_blocks, expected_center, expected_radius) in cases {
        let cohort = super::ViewCohort::from_publisher(3, center, radius_blocks);
        assert_eq!(cohort.dimension, 3);
        assert_eq!(cohort.center, expected_center);
        assert_eq!(cohort.radius, expected_radius);
    }
}

#[test]
fn in_scope_prefetch_columns_do_not_prevent_exact_cohort_readiness() {
    let mut stream = WorldStream::new(WorldBootstrap {
        dimension: 0,
        local_player_runtime_id: 1,
        player_position: [0.0; 3],
        world_spawn_position: [0; 3],
        air_network_id: 12_530,
        block_network_ids_are_hashes: false,
    });
    let target = super::ViewCohort::from_publisher(0, [0, 64, 0], 16);
    stream.committed_view_cohort = Some(target);
    stream.publisher_epoch = 1;
    stream.required_columns = super::ViewCohort {
        publisher_geometry: None,
        ..target
    }
    .classifier_columns();
    stream.loaded_columns = stream.required_columns.clone();
    stream.loaded_columns.insert(ChunkKey::new(0, 1, 1));

    let status = stream.cohort_status(target);

    assert_eq!(status.expected, 5);
    assert_eq!(status.loaded_target, 5);
    assert_eq!(status.missing_target, 0);
    assert_eq!(status.foreign_loaded, 0);
    assert!(status.is_exact());
}

#[test]
fn in_scope_prefetch_cannot_replace_a_missing_required_column() {
    let mut stream = WorldStream::new(WorldBootstrap {
        dimension: 0,
        local_player_runtime_id: 1,
        player_position: [0.0; 3],
        world_spawn_position: [0; 3],
        air_network_id: 12_530,
        block_network_ids_are_hashes: false,
    });
    let target = super::ViewCohort::from_publisher(0, [0, 64, 0], 16);
    stream.committed_view_cohort = Some(target);
    stream.publisher_epoch = 1;
    stream.required_columns = super::ViewCohort {
        publisher_geometry: None,
        ..target
    }
    .classifier_columns();
    stream.loaded_columns = stream.required_columns.clone();
    stream.loaded_columns.remove(&ChunkKey::new(0, 1, 0));
    stream.loaded_columns.insert(ChunkKey::new(0, 1, 1));

    let status = stream.cohort_status(target);

    assert_eq!(stream.loaded_columns.len(), status.expected);
    assert_eq!(status.loaded_target, 4);
    assert_eq!(status.missing_target, 1);
    assert_eq!(status.foreign_loaded, 0);
    assert!(!status.is_exact());
}

#[test]
fn columns_outside_square_prefetch_scope_prevent_exact_cohort_readiness() {
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
        center: [0, 0],
        radius: 1,
        publisher_geometry: None,
    };
    stream.committed_view_cohort = Some(target);
    stream.loaded_columns = target.classifier_columns();
    stream.loaded_columns.insert(ChunkKey::new(0, 2, 0));

    let status = stream.cohort_status(target);

    assert_eq!(status.foreign_loaded, 1);
    assert!(!status.is_exact());
}

#[test]
fn publisher_cohort_is_exposed_only_after_fifo_commit() {
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
        center: [100, 0],
        radius: 16,
        publisher_geometry: Some(super::PublisherViewGeometry {
            center_blocks: [1_600, 0],
            radius_blocks: 256,
        }),
    };

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

    assert_ne!(stream.cohort_status(target).committed, Some(target));

    complete_pending_decode_jobs(&mut stream);

    assert_eq!(stream.cohort_status(target).committed, Some(target));
}

#[test]
fn publisher_cohort_accessor_is_exposed_only_after_fifo_commit() {
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
        center: [100, 0],
        radius: 16,
        publisher_geometry: Some(super::PublisherViewGeometry {
            center_blocks: [1_600, 0],
            radius_blocks: 256,
        }),
    };

    assert_eq!(stream.committed_view_cohort(), None);

    stream
        .submit(
            2,
            WorldEvent::PublisherUpdate(PublisherUpdateEvent {
                center: [1_600, 64, 0],
                radius_blocks: 256,
            }),
        )
        .unwrap();

    assert_eq!(stream.committed_view_cohort(), None);

    stream
        .submit(1, WorldEvent::ChunkRadiusUpdated(16))
        .unwrap();

    assert_eq!(stream.committed_view_cohort(), Some(target));
}

#[test]
fn source_capture_occurs_at_move_fifo_commit_before_later_publisher_eviction() {
    let mut stream = WorldStream::new(WorldBootstrap {
        dimension: 0,
        local_player_runtime_id: 1,
        player_position: [0.5, 70.0, 0.5],
        world_spawn_position: [0, 70, 0],
        air_network_id: 12_530,
        block_network_ids_are_hashes: false,
    });
    let source = ChunkKey::new(0, 0, 0);
    let source_cohort = super::ViewCohort {
        dimension: 0,
        center: [0, 0],
        radius: 16,
        publisher_geometry: Some(super::PublisherViewGeometry {
            center_blocks: [0, 0],
            radius_blocks: 256,
        }),
    };
    stream.loaded_columns.insert(source);
    stream.chunk_radius = Some(16);
    stream.schedule_source_capture(2);

    stream
        .submit(
            2,
            WorldEvent::MovePlayer(MovePlayerEvent {
                runtime_id: 1,
                position: [1_040.5, 70.0, 1_040.5],
                pitch: 0.0,
                yaw: 0.0,
                ..Default::default()
            }),
        )
        .unwrap();
    stream
        .submit(
            3,
            WorldEvent::PublisherUpdate(PublisherUpdateEvent {
                center: [1_040, 70, 1_040],
                radius_blocks: 256,
            }),
        )
        .unwrap();
    stream
        .submit(
            1,
            WorldEvent::PublisherUpdate(PublisherUpdateEvent {
                center: [0, 70, 0],
                radius_blocks: 256,
            }),
        )
        .unwrap();

    assert!(stream.source_columns.contains(&source));
    assert!(!stream.tracked_columns().contains(&source));
    assert!(matches!(
        stream.take_committed_controls().as_slice(),
        [super::CommittedControlEvent::MovePlayer {
            sequence: 2,
            source_cohort: Some(cohort),
            ..
        }] if *cohort == source_cohort
    ));
}

#[test]
fn disjoint_local_teleport_accepts_destination_chunks_before_publisher_update() {
    let mut stream = WorldStream::new(WorldBootstrap {
        dimension: 0,
        local_player_runtime_id: 1,
        player_position: [0.5, 70.0, 0.5],
        world_spawn_position: [0, 70, 0],
        air_network_id: 12_530,
        block_network_ids_are_hashes: false,
    });
    stream.submit(1, WorldEvent::ChunkRadiusUpdated(8)).unwrap();
    stream
        .submit(
            2,
            WorldEvent::PublisherUpdate(PublisherUpdateEvent {
                center: [0, 70, 0],
                radius_blocks: 128,
            }),
        )
        .unwrap();
    stream
        .submit(
            3,
            request_level_chunk_event(0, 0, 0, LevelChunkMode::LimitedRequests { highest: 1 }, 1),
        )
        .unwrap();
    complete_pending_decode_jobs(&mut stream);
    let source = ChunkKey::new(0, 0, 0);
    let source_request = stream.pop_next_request().unwrap();
    let sent_at = Instant::now();
    stream.record_sub_chunk_request_transport_pending(
        source_request.chunk,
        source_request.base_sub_chunk_y,
        source_request.count,
    );
    stream.acknowledge_sub_chunk_request_sent(
        source_request.chunk,
        source_request.base_sub_chunk_y,
        source_request.count,
        sent_at,
    );
    assert!(stream.tracked_columns().contains(&source));
    assert!(!stream.sub_chunk_deadlines.is_empty());

    let destination = ChunkKey::new(0, 65, 65);
    stream
        .submit(
            4,
            WorldEvent::MovePlayer(MovePlayerEvent {
                runtime_id: 1,
                position: [1_040.5, 70.0, 1_040.5],
                mode: MovePlayerMode::Teleport,
                teleported: true,
                ..Default::default()
            }),
        )
        .unwrap();

    assert_eq!(stream.publisher_center, Some([1_040, 70, 1_040]));
    assert_eq!(stream.committed_view_cohort(), None);
    assert!(!stream.tracked_columns().contains(&source));
    assert!(stream.sub_chunk_deadlines.is_empty());
    assert_eq!(stream.outstanding_sub_chunk_count(), 0);
    let armed = stream.phase2_publication_snapshot(destination);
    assert!(armed.local_reset_armed);
    assert_eq!(armed.local_resets_armed, 1);
    assert_eq!(armed.local_resets_consumed, 0);
    assert_eq!(armed.publisher_center, Some([1_040, 70, 1_040]));

    let inactive_before = stream.stats().normalization_reasons.inactive_level_chunks;
    stream
        .submit(
            5,
            request_level_chunk_event(
                0,
                destination.x,
                destination.z,
                LevelChunkMode::LimitedRequests { highest: 1 },
                2,
            ),
        )
        .unwrap();
    complete_pending_decode_jobs(&mut stream);

    assert_eq!(
        stream.stats().normalization_reasons.inactive_level_chunks,
        inactive_before
    );
    assert!(stream.requested_sub_chunks.contains_key(&destination));
    assert!(stream.required_columns.contains(&destination));

    stream
        .submit(
            6,
            WorldEvent::PublisherUpdate(PublisherUpdateEvent {
                center: [1_040, 70, 1_040],
                radius_blocks: 128,
            }),
        )
        .unwrap();

    assert!(stream.requested_sub_chunks.contains_key(&destination));
    assert!(stream.required_columns.contains(&destination));
    assert_eq!(stream.publisher_epoch, 2);
    assert_eq!(stream.committed_view_cohort().unwrap().center, [65, 65]);
    let consumed = stream.phase2_publication_snapshot(destination);
    assert!(!consumed.local_reset_armed);
    assert_eq!(consumed.local_resets_armed, 1);
    assert_eq!(consumed.local_resets_consumed, 1);
    assert!(consumed.player_column_required);

    stream.poll([1_040.5, 70.0, 1_040.5], 0);
    let request = stream.pop_next_request().unwrap();
    assert_eq!(request.chunk, destination);
    assert_eq!(
        stream
            .phase2_publication_snapshot(destination)
            .local_reset_dispatch_total,
        0,
        "selection alone is not successful-send evidence",
    );
    stream.record_sub_chunk_request_transport_pending(
        request.chunk,
        request.base_sub_chunk_y,
        request.count,
    );
    let dispatched = stream.phase2_publication_snapshot(destination);
    assert_eq!(dispatched.local_reset_dispatch_count, 1);
    assert_eq!(dispatched.local_reset_dispatch_total, 1);
    assert!(!dispatched.local_reset_dispatch_trace_overflowed);
    assert_eq!(
        dispatched.local_reset_dispatch_classes[0],
        Some(RequestClass::PlayerInitial)
    );

    stream
        .submit(
            7,
            WorldEvent::ChangeDimension(ChangeDimensionEvent {
                dimension: 1,
                position: [8.0, 80.0, 9.0],
            }),
        )
        .unwrap();
    let changed_dimension = stream.phase2_publication_snapshot(ChunkKey::new(1, 0, 0));
    assert_eq!(changed_dimension.local_resets_armed, 0);
    assert_eq!(changed_dimension.local_resets_consumed, 0);
    assert_eq!(changed_dimension.local_reset_dispatch_count, 0);
    assert_eq!(changed_dimension.local_reset_dispatch_total, 0);
}

#[test]
fn only_disjoint_local_teleports_may_provisionally_rebase_publisher_retention() {
    fn stream_at_origin() -> WorldStream {
        let mut stream = WorldStream::new(WorldBootstrap {
            dimension: 0,
            local_player_runtime_id: 1,
            player_position: [0.5, 70.0, 0.5],
            world_spawn_position: [0, 70, 0],
            air_network_id: 12_530,
            block_network_ids_are_hashes: false,
        });
        stream.chunk_radius = Some(8);
        stream.publisher_radius_chunks = Some(8);
        let resident = SubChunkKey::new(0, 0, -4, 0);
        let requested = SubChunkKey::new(0, 1, -4, 0);
        let deadline = Instant::now() + super::SUB_CHUNK_RESPONSE_TIMEOUT;
        let pending = super::PendingSubChunk {
            response_deadline: Some(deadline),
            ..Default::default()
        };
        stream.loaded_columns.insert(resident.chunk());
        stream.resident.insert(resident);
        stream
            .requested_sub_chunks
            .insert(requested.chunk(), BTreeMap::from([(requested.y, pending)]));
        stream.sub_chunk_deadlines.insert((deadline, requested));
        stream
    }

    for movement in [
        MovePlayerEvent {
            runtime_id: 2,
            position: [1_040.5, 70.0, 1_040.5],
            mode: MovePlayerMode::Teleport,
            teleported: true,
            ..Default::default()
        },
        MovePlayerEvent {
            runtime_id: 1,
            position: [1_040.5, 70.0, 1_040.5],
            mode: MovePlayerMode::Normal,
            ..Default::default()
        },
        MovePlayerEvent {
            runtime_id: 1,
            position: [16.5, 70.0, 16.5],
            mode: MovePlayerMode::Teleport,
            teleported: true,
            ..Default::default()
        },
    ] {
        let mut stream = stream_at_origin();
        let tracked_before = stream.tracked_columns();
        let deadlines_before = stream.sub_chunk_deadlines.clone();
        stream.submit(1, WorldEvent::MovePlayer(movement)).unwrap();
        assert_eq!(stream.publisher_center, Some([0, 70, 0]));
        assert_eq!(stream.tracked_columns(), tracked_before);
        assert_eq!(stream.sub_chunk_deadlines, deadlines_before);
    }
}

#[test]
fn publisher_cohort_preserves_over_max_radius_while_runtime_scope_clamps() {
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
        center: [0, 0],
        radius: 16,
        publisher_geometry: None,
    };

    stream
        .submit(
            1,
            WorldEvent::PublisherUpdate(PublisherUpdateEvent {
                center: [0, 64, 0],
                radius_blocks: 272,
            }),
        )
        .unwrap();

    assert_eq!(
        stream.cohort_status(target).committed,
        Some(super::ViewCohort {
            dimension: 0,
            center: [0, 0],
            radius: 17,
            publisher_geometry: Some(super::PublisherViewGeometry {
                center_blocks: [0, 0],
                radius_blocks: 272,
            }),
        })
    );
    assert_eq!(stream.stats().publisher_radius_chunks, Some(16));
}

#[test]
fn equal_resident_and_known_air_counts_with_key_replacement_change_identity_hashes() {
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
        center: [0, 0],
        radius: 1,
        publisher_geometry: None,
    };
    stream
        .submit(
            1,
            WorldEvent::PublisherUpdate(PublisherUpdateEvent {
                center: [0, 64, 0],
                radius_blocks: 16,
            }),
        )
        .unwrap();
    let key_a = SubChunkKey::new(0, -1, 0, -1);
    let key_b = SubChunkKey::new(0, 1, 0, 1);
    stream.record_known_air(key_a);
    let before = stream.cohort_status(target);

    stream.resident.clear();
    stream.known_air.clear();
    stream.record_known_air(key_b);
    let after = stream.cohort_status(target);

    assert_eq!(before.resident_count, after.resident_count);
    assert_ne!(before.resident_hash, after.resident_hash);
    assert_eq!(before.known_air_count, after.known_air_count);
    assert_ne!(before.known_air_hash, after.known_air_hash);
}

#[test]
fn newer_subchunk_is_validated_after_fifo_blocked_dimension_change_commits() {
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
            WorldEvent::ChangeDimension(ChangeDimensionEvent {
                dimension: 1,
                position: [1_600.0, 80.0, 0.0],
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
        dimension: 0,
        local_player_runtime_id: 1,
        player_position: [0.0; 3],
        world_spawn_position: [0; 3],
        air_network_id: 12_530,
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

    let error = stream
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
        .unwrap_err();
    assert!(matches!(
        error,
        super::WorldStreamError::OutboundFull { .. }
    ));
    assert_eq!(stream.pending_request_count(), 62);

    complete_pending_decode_jobs(&mut stream);
    assert_eq!(
        stream.pending_request_count(),
        super::OUTBOUND_REQUEST_CAPACITY
    );
}

#[test]
fn heavy_admission_is_bounded_before_rayon_and_retained_work_never_exceeds_constants() {
    let mut stream = WorldStream::new(WorldBootstrap {
        dimension: 0,
        local_player_runtime_id: 1,
        player_position: [0.0; 3],
        world_spawn_position: [0; 3],
        air_network_id: 12_530,
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

    let error = stream
        .submit(
            super::MAX_ADMITTED_HEAVY_EVENTS as u64 + 1,
            inline_air_event(999),
        )
        .unwrap_err();
    assert!(matches!(
        error,
        super::WorldStreamError::AdmissionFull { .. }
    ));
}

#[test]
fn one_dispatch_drains_the_entire_bounded_heavy_admission_window() {
    let mut stream = WorldStream::new(WorldBootstrap {
        dimension: 0,
        local_player_runtime_id: 1,
        player_position: [0.0; 3],
        world_spawn_position: [0; 3],
        air_network_id: 12_530,
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
        dimension: 0,
        local_player_runtime_id: 1,
        player_position: [0.0; 3],
        world_spawn_position: [0; 3],
        air_network_id: 12_530,
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
    assert_eq!(stream.requests.len(), 1);

    stream.evict_column(chunk);
    assert!(stream.requests.is_empty());
    stream
        .submit(
            2,
            WorldEvent::SubChunks(SubChunkBatchEvent {
                dimension: 0,
                entries: vec![SubChunkEntryEvent {
                    position: [key.x, key.y, key.z],
                    result: SubChunkResult::AllAir,
                }],
            }),
        )
        .unwrap();
    assert_eq!(stream.stats().queued_decode_jobs, 1);
    complete_pending_decode_jobs(&mut stream);
    assert!(!stream.resident.contains(&key));
    assert!(stream.store.sub_chunk(key).is_none());
}

#[test]
fn valid_late_inactive_subchunk_reply_records_stale_without_world_side_effects() {
    let mut stream = WorldStream::new(WorldBootstrap {
        dimension: 0,
        local_player_runtime_id: 1,
        player_position: [0.0; 3],
        world_spawn_position: [0; 3],
        air_network_id: 12_530,
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
    assert!(stream.requested_sub_chunks.contains_key(&chunk));

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
    assert!(stream.store.sub_chunk(key).is_none());

    let resident_before = stream.resident.clone();
    let known_air_before = stream.known_air.clone();
    let loaded_columns_before = stream.loaded_columns.clone();
    let requested_sub_chunks_before = stream.requested_sub_chunks.clone();
    let sub_chunk_deadlines_before = stream.sub_chunk_deadlines.clone();
    let deferred_retries_before = stream.deferred_retries.clone();
    let deferred_retry_set_before = stream.deferred_retry_set.clone();
    let requests_before = stream.requests.len();
    let stats_before = stream.stats();

    apply_sub_chunk_result(&mut stream, key, super::PreparedSubChunkResult::AllAir);

    assert!(stream.store.sub_chunk(key).is_none());
    assert_eq!(stream.resident, resident_before);
    assert_eq!(stream.known_air, known_air_before);
    assert_eq!(stream.loaded_columns, loaded_columns_before);
    assert_eq!(stream.requested_sub_chunks, requested_sub_chunks_before);
    assert_eq!(stream.sub_chunk_deadlines, sub_chunk_deadlines_before);
    assert_eq!(stream.deferred_retries, deferred_retries_before);
    assert_eq!(stream.deferred_retry_set, deferred_retry_set_before);
    assert_eq!(stream.requests.len(), requests_before);
    let mut expected_stats = stats_before;
    expected_stats.phase2_outcomes.stale = expected_stats.phase2_outcomes.stale.saturating_add(1);
    assert_eq!(stream.stats(), expected_stats);
    assert_eq!(stream.stats().normalization_reasons.inactive_sub_chunks, 0);
}

#[test]
fn old_dimension_and_out_of_radius_chunks_are_rejected_and_radii_are_clamped() {
    let mut stream = WorldStream::new(WorldBootstrap {
        dimension: 0,
        local_player_runtime_id: 1,
        player_position: [0.0; 3],
        world_spawn_position: [0; 3],
        air_network_id: 12_530,
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
    assert_eq!(
        stream.chunk_radius,
        Some(super::PHASE0_MAX_VIEW_RADIUS_CHUNKS)
    );
    assert_eq!(
        stream.publisher_radius_chunks,
        Some(super::PHASE0_MAX_VIEW_RADIUS_CHUNKS)
    );
    let stats = format!("{:?}", stream.stats());
    assert!(
        stats.contains("received_radius_chunks: Some(16)"),
        "{stats}"
    );
    assert!(
        stats.contains("publisher_radius_chunks: Some(16)"),
        "{stats}"
    );

    stream
        .submit(
            3,
            WorldEvent::LevelChunk(LevelChunkEvent {
                dimension: 0,
                x: super::PHASE0_MAX_VIEW_RADIUS_CHUNKS + 1,
                z: 0,
                mode: LevelChunkMode::LimitlessRequests,
                payload: biome_payload(0, 1),
            }),
        )
        .unwrap();
    complete_pending_decode_jobs(&mut stream);
    assert!(stream.requests.is_empty());

    stream
        .submit(
            4,
            WorldEvent::ChangeDimension(ChangeDimensionEvent {
                dimension: 1,
                position: [0.0, 80.0, 0.0],
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
    assert!(stream.store.sub_chunk(implicit_air).is_none());
    assert_eq!(
        stream.requested_sub_chunks[&ChunkKey::new(0, 0, 0)]
            .keys()
            .copied()
            .collect::<BTreeSet<_>>(),
        BTreeSet::from([-4])
    );
}

#[test]
fn control_effects_are_exposed_only_after_older_heavy_sequence_commits_in_fifo_order() {
    let mut stream = WorldStream::new(WorldBootstrap {
        dimension: 0,
        local_player_runtime_id: 1,
        player_position: [0.0; 3],
        world_spawn_position: [0; 3],
        air_network_id: 12_530,
        block_network_ids_are_hashes: false,
    });
    let movement = MovePlayerEvent {
        runtime_id: 1,
        position: [4.0, 70.0, 5.0],
        pitch: 7.0,
        yaw: 9.0,
        ..Default::default()
    };
    let change = ChangeDimensionEvent {
        dimension: 1,
        position: [8.0, 80.0, 9.0],
    };
    stream.submit(1, inline_air_event(0)).unwrap();
    stream.submit(2, WorldEvent::MovePlayer(movement)).unwrap();
    stream
        .submit(3, WorldEvent::ChangeDimension(change))
        .unwrap();

    assert_eq!(stream.current_dimension(), 0);
    assert!(stream.take_committed_controls().is_empty());

    let super::DecodeJob::InlineLevelChunk {
        mut event,
        base_sub_chunk_y,
        count,
        ..
    } = stream.pending_decode.pop_front().unwrap().job
    else {
        panic!("expected inline decode job")
    };
    let payload = std::mem::take(&mut event.payload);
    let decoded = DecodedLevelChunk::decode(base_sub_chunk_y, count, &payload);
    stream
        .ordered
        .insert(
            1,
            super::PreparedWorldEvent::InlineLevelChunk {
                event,
                decoded,
                duration: std::time::Duration::ZERO,
            },
        )
        .unwrap();
    stream.apply_ready();

    assert_eq!(stream.current_dimension(), 1);
    assert_eq!(
        stream.take_committed_controls(),
        vec![
            super::CommittedControlEvent::MovePlayer {
                sequence: 2,
                movement,
                resolved: super::server_position::ResolvedServerPosition {
                    position: movement.position,
                    surface_anchor: None,
                },
                source_cohort: None,
            },
            super::CommittedControlEvent::ChangeDimension {
                change,
                resolved: super::server_position::ResolvedServerPosition {
                    position: change.position,
                    surface_anchor: None,
                },
            },
        ]
    );
}

#[test]
fn movement_correction_commits_in_fifo_without_move_player_capture_metadata() {
    let mut stream = WorldStream::new(WorldBootstrap {
        dimension: 0,
        local_player_runtime_id: 1,
        player_position: [0.0; 3],
        world_spawn_position: [0; 3],
        air_network_id: 12_530,
        block_network_ids_are_hashes: false,
    });
    let correction = PlayerMovementCorrectionEvent {
        position: [27.5, 111.0, 91.5],
        delta: [0.25, -0.5, 0.75],
        pitch: -12.0,
        yaw: 143.0,
        on_ground: true,
        tick: 4_096,
    };
    stream.submit(1, inline_air_event(0)).unwrap();
    stream
        .submit(2, WorldEvent::PlayerMovementCorrection(correction))
        .unwrap();

    assert!(stream.take_committed_controls().is_empty());

    let super::DecodeJob::InlineLevelChunk {
        mut event,
        base_sub_chunk_y,
        count,
        ..
    } = stream.pending_decode.pop_front().unwrap().job
    else {
        panic!("expected inline decode job")
    };
    let payload = std::mem::take(&mut event.payload);
    let decoded = DecodedLevelChunk::decode(base_sub_chunk_y, count, &payload);
    stream
        .ordered
        .insert(
            1,
            super::PreparedWorldEvent::InlineLevelChunk {
                event,
                decoded,
                duration: std::time::Duration::ZERO,
            },
        )
        .unwrap();
    stream.apply_ready();

    assert_eq!(
        stream.take_committed_controls(),
        vec![super::CommittedControlEvent::PlayerMovementCorrection {
            sequence: 2,
            correction,
            resolved: super::server_position::ResolvedServerPosition {
                position: correction.position,
                surface_anchor: None,
            },
        }]
    );
}

#[test]
fn ui_and_block_crack_events_publish_fifo_with_committed_dimension() {
    let mut stream = WorldStream::new(WorldBootstrap {
        dimension: 2,
        local_player_runtime_id: 1,
        player_position: [0.0; 3],
        world_spawn_position: [0; 3],
        air_network_id: 12_530,
        block_network_ids_are_hashes: false,
    });
    let ui = UiEvent::Hud(HudEvent::Health { health: 17 });
    let crack = BlockCrackEvent {
        position: [-3, 72, 9],
        action: BlockCrackAction::UpdateSpeed {
            progress_per_tick: 2_048,
        },
    };

    stream.submit(1, WorldEvent::Ui(ui.clone())).unwrap();
    stream.submit(2, WorldEvent::BlockCrack(crack)).unwrap();

    assert_eq!(
        stream.take_committed_ui(),
        vec![
            CommittedUiEvent::Ui {
                sequence: 1,
                event: ui,
            },
            CommittedUiEvent::BlockCrack {
                sequence: 2,
                dimension: 2,
                event: crack,
            },
        ]
    );
    assert!(stream.take_committed_ui().is_empty());
}
