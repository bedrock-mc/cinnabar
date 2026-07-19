use super::*;

#[test]
fn player_and_visible_retries_precede_far_initial_prefetch_without_losing_fifo_ties() {
    let mut stream = WorldStream::new(WorldBootstrap {
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
