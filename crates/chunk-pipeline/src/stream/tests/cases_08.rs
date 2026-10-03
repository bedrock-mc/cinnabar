use super::*;

#[test]
fn player_and_visible_retries_precede_far_initial_prefetch_without_losing_fifo_ties() {
    let mut stream = WorldStream::new(WorldBootstrap {
        local_player_unique_id: 1,
        dimension: 0,
        local_player_runtime_id: 1,
        player_position: [0.5, 70.0, 0.5],
        world_spawn_position: [0, 70, 0],
        air_network_id: 12_530,
        block_network_ids_are_hashes: false,
    });
    stream.poll([0.5, 70.0, 0.5], 0);
    stream
        .submit(
            1,
            WorldEvent::PublisherUpdate(PublisherUpdateEvent {
                center: [0, 70, 0],
                radius_blocks: 128,
            }),
        )
        .unwrap();
    for (sequence, chunk, count) in [
        (2, ChunkKey::new(0, 6, 0), 1),
        (3, ChunkKey::new(0, 2, 0), 1),
        (4, ChunkKey::new(0, 0, 0), 2),
    ] {
        stream
            .submit(
                sequence,
                request_level_chunk_event(
                    chunk.dimension,
                    chunk.x,
                    chunk.z,
                    LevelChunkMode::LimitedRequests { highest: count },
                    1,
                ),
            )
            .unwrap();
    }
    complete_pending_decode_jobs(&mut stream);

    let player = ChunkKey::new(0, 0, 0);
    let visible = ChunkKey::new(0, 2, 0);
    let prefetch = ChunkKey::new(0, 6, 0);
    stream.required_columns = BTreeSet::from([player, visible]);
    stream.requests.retain(|slot| {
        !matches!(slot, super::OutboundRequestSlot::Ready(request) if request.chunk == player)
    });
    for y in [-4, -3] {
        let key = SubChunkKey::from_chunk(player, y);
        assert_eq!(
            stream.try_schedule_exact_retry(key),
            super::RetrySchedule::Scheduled
        );
        stream.record_retry_scheduled(key);
    }

    let first = stream.pop_next_request().unwrap();
    let second = stream.pop_next_request().unwrap();
    let third = stream.pop_next_request().unwrap();
    let fourth = stream.pop_next_request().unwrap();
    assert_eq!((first.chunk, first.base_sub_chunk_y), (player, -4));
    assert_eq!((second.chunk, second.base_sub_chunk_y), (player, -3));
    assert_eq!(third.chunk, visible);
    assert_eq!(fourth.chunk, prefetch);
}

#[test]
fn request_priority_uses_last_finite_polled_player_chunk_and_horizontal_distance() {
    let mut stream = WorldStream::new(WorldBootstrap {
        local_player_unique_id: 1,
        dimension: 0,
        local_player_runtime_id: 1,
        player_position: [0.5, 70.0, 0.5],
        world_spawn_position: [0, 70, 0],
        air_network_id: 12_530,
        block_network_ids_are_hashes: false,
    });
    stream.poll([0.5, 70.0, 0.5], 0);
    stream.poll([f32::NAN, 70.0, 1_600.0], 0);
    for (sequence, x) in [(1, 4), (2, 2)] {
        stream
            .submit(
                sequence,
                request_level_chunk_event(
                    0,
                    x,
                    0,
                    LevelChunkMode::LimitedRequests { highest: 1 },
                    1,
                ),
            )
            .unwrap();
    }
    complete_pending_decode_jobs(&mut stream);
    stream.required_columns = BTreeSet::from([ChunkKey::new(0, 4, 0), ChunkKey::new(0, 2, 0)]);

    assert_eq!(stream.pop_next_request().unwrap().chunk.x, 2);
    assert_eq!(stream.pop_next_request().unwrap().chunk.x, 4);
}

#[test]
fn restoring_unsent_request_preserves_original_fifo_tie_identity() {
    let mut stream = WorldStream::new(WorldBootstrap {
        local_player_unique_id: 1,
        dimension: 0,
        local_player_runtime_id: 1,
        player_position: [0.5, 70.0, 0.5],
        world_spawn_position: [0, 70, 0],
        air_network_id: 12_530,
        block_network_ids_are_hashes: false,
    });
    stream.poll([0.5, 70.0, 0.5], 0);
    for (sequence, x) in [(1, 1), (2, -1)] {
        stream
            .submit(
                sequence,
                request_level_chunk_event(
                    0,
                    x,
                    0,
                    LevelChunkMode::LimitedRequests { highest: 1 },
                    1,
                ),
            )
            .unwrap();
    }
    complete_pending_decode_jobs(&mut stream);
    stream.required_columns = BTreeSet::from([ChunkKey::new(0, 1, 0), ChunkKey::new(0, -1, 0)]);

    let first = stream.pop_next_request().unwrap();
    assert_eq!(first.chunk.x, 1);
    assert!(stream.retry_request_front(first).is_ok());
    assert_eq!(stream.pop_next_request().unwrap().chunk.x, 1);
    assert_eq!(stream.pop_next_request().unwrap().chunk.x, -1);
}

/// A permit-denied current mesh waits in staging instead of being meshed again.
#[test]
fn permit_denied_mesh_publishes_from_staging_without_a_second_mesh_job() {
    let mut stream = WorldStream::new(WorldBootstrap {
        local_player_unique_id: 1,
        dimension: 0,
        local_player_runtime_id: 1,
        player_position: [0.0; 3],
        world_spawn_position: [0; 3],
        air_network_id: 12_530,
        block_network_ids_are_hashes: false,
    });
    let key = SubChunkKey::new(0, 0, -4, 0);
    stream
        .authority
        .update_block(key, BlockUpdate::new(0, 0, 0, 0, 99), 12_530)
        .unwrap();
    stream.resident.insert(key);
    stream.mark_light_changed_sources([key]);
    light_scheduler::settle_light(&mut stream, [0.0; 3]);
    stream.mark_dirty_exact(key, Instant::now());
    let config = crate::PublicationServiceConfig::PHASE2_GATE;
    let allowance = crate::PublicationAllowance::new(config);
    allowance.begin_frame(1, 0, 0, 0, 0);
    stream.set_publication_allowance(allowance.clone());

    let mut dispatched = 0;
    let deadline = Instant::now() + std::time::Duration::from_secs(10);
    for _ in 0..64 {
        dispatched += stream.poll([0.0; 3], 32).mesh_jobs_dispatched;
        if dispatched > 0 && stream.in_flight.is_empty() && stream.pending_mesh.is_empty() {
            break;
        }
        assert!(Instant::now() < deadline);
        std::thread::sleep(std::time::Duration::from_millis(5));
    }
    for _ in 0..4 {
        dispatched += stream.poll([0.0; 3], 32).mesh_jobs_dispatched;
    }
    assert_eq!(
        dispatched, 1,
        "a denied permit must not remesh the same revision"
    );
    assert_eq!(stream.staged_mesh_completions.len(), 1);
    assert!(stream.take_mesh_changes().is_empty());

    allowance.begin_frame(
        2,
        config.maximum_frame_items,
        config.maximum_frame_bytes,
        config.maximum_zero_byte_operations_per_frame,
        config.maximum_frame_items,
    );
    dispatched += stream.poll([0.0; 3], 32).mesh_jobs_dispatched;
    let changes = stream.take_mesh_changes();
    assert_eq!(dispatched, 1);
    assert!(stream.staged_mesh_completions.is_empty());
    assert!(matches!(
        changes.as_slice(),
        [WorldMeshChange::Upsert { key: published, permit: Some(_), .. }] if *published == key
    ));
}
