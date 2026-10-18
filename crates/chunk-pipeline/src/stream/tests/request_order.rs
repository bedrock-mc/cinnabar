use super::*;

#[test]
fn request_dispatch_keeps_fifo_order_across_prefetch_visible_and_player_retries() {
    let mut stream = WorldStream::new(WorldBootstrap {
        local_player_unique_id: 1,
        dimension: 0,
        local_player_runtime_id: 1,
        player_position: [0.5, 70.0, 0.5],
        world_spawn_position: [0, 70, 0],
        air_network_id: protocol_ids::SEQUENTIAL_AIR_NETWORK_ID,
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
    stream.publisher.required_columns = BTreeSet::from([player, visible]);
    stream
        .requests
        .queue
        .cancel_ready(|request| request.chunk == player);
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
    assert_eq!(first.chunk, prefetch);
    assert_eq!(second.chunk, visible);
    assert_eq!((third.chunk, third.base_sub_chunk_y), (player, -4));
    assert_eq!((fourth.chunk, fourth.base_sub_chunk_y), (player, -3));
}

#[test]
fn request_dispatch_keeps_fifo_order_across_player_polls_and_horizontal_distances() {
    let mut stream = WorldStream::new(WorldBootstrap {
        local_player_unique_id: 1,
        dimension: 0,
        local_player_runtime_id: 1,
        player_position: [0.5, 70.0, 0.5],
        world_spawn_position: [0, 70, 0],
        air_network_id: protocol_ids::SEQUENTIAL_AIR_NETWORK_ID,
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
    stream.publisher.required_columns =
        BTreeSet::from([ChunkKey::new(0, 4, 0), ChunkKey::new(0, 2, 0)]);

    assert_eq!(stream.pop_next_request().unwrap().chunk.x, 4);
    assert_eq!(stream.pop_next_request().unwrap().chunk.x, 2);
}

#[test]
fn restoring_unsent_request_preserves_original_fifo_tie_identity() {
    let mut stream = WorldStream::new(WorldBootstrap {
        local_player_unique_id: 1,
        dimension: 0,
        local_player_runtime_id: 1,
        player_position: [0.5, 70.0, 0.5],
        world_spawn_position: [0, 70, 0],
        air_network_id: protocol_ids::SEQUENTIAL_AIR_NETWORK_ID,
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
    stream.publisher.required_columns =
        BTreeSet::from([ChunkKey::new(0, 1, 0), ChunkKey::new(0, -1, 0)]);

    let first = stream.pop_next_request().unwrap();
    assert_eq!(first.chunk.x, 1);
    assert!(stream.retry_request_front(first).is_ok());
    assert_eq!(stream.pop_next_request().unwrap().chunk.x, 1);
    assert_eq!(stream.pop_next_request().unwrap().chunk.x, -1);
}
