use super::*;

/// Makes a request-mode column with one requested section and valid biomes.
fn column(dimension: i32, x: i32) -> WorldEvent {
    WorldEvent::LevelChunk(LevelChunkEvent {
        dimension,
        x,
        z: 0,
        mode: LevelChunkMode::LimitedRequests { highest: 1 },
        payload: if vanilla_dimension_range(dimension).is_some() {
            biome_payload(dimension, 1)
        } else {
            Vec::new()
        },
    })
}

/// Starts a stream whose test block identifiers survive decoding.
fn stream() -> WorldStream {
    WorldStream::new(WorldBootstrap {
        dimension: 0,
        local_player_runtime_id: 1,
        local_player_unique_id: 1,
        player_position: [0.0; 3],
        world_spawn_position: [0; 3],
        air_network_id: RAW_IDS.air,
        block_network_ids_are_hashes: false,
    })
}

#[test]
fn request_is_ready_before_decode_and_commit_keeps_its_transport_deadline() {
    let mut stream = stream();
    stream.submit(1, column(0, 0)).unwrap();
    assert_eq!(stream.committed_sequence(), 0);
    let request = stream.pop_next_request().expect("request precedes decode");
    stream.record_sub_chunk_request_transport_pending(
        request.chunk,
        request.base_sub_chunk_y,
        request.count,
    );
    let sent = Instant::now();
    stream.acknowledge_sub_chunk_request_sent(
        request.chunk,
        request.base_sub_chunk_y,
        request.count,
        sent,
    );
    let key = SubChunkKey::from_chunk(request.chunk, request.base_sub_chunk_y);
    let deadline = stream.requests.requested[&request.chunk][&key.y].response_deadline;
    complete_pending_decode_jobs(&mut stream);
    assert_eq!(stream.committed_sequence(), 1);
    assert!(stream.take_requests().is_empty());
    assert_eq!(
        stream.requests.requested[&request.chunk][&key.y].response_deadline,
        deadline
    );
    assert_eq!(
        stream.requests.requested[&request.chunk][&key.y].confirmed_attempts,
        1
    );
}

#[test]
fn early_reply_waits_for_column_decode_then_completes_the_request() {
    let mut stream = stream();
    stream.submit(1, column(0, 0)).unwrap();
    let request = stream.take_requests().pop().unwrap();
    let key = SubChunkKey::from_chunk(request.chunk, request.base_sub_chunk_y);
    stream
        .submit(
            2,
            WorldEvent::SubChunks(SubChunkBatchEvent {
                dimension: 0,
                entries: vec![SubChunkEntryEvent {
                    position: [key.x, key.y, key.z],
                    result: SubChunkResult::AllAir,
                    diagnostics: None,
                }],
            }),
        )
        .unwrap();
    let column_job = stream.pending_decode.pop_front().unwrap();
    let reply_job = stream.pending_decode.pop_front().unwrap();
    complete_decode_job(&mut stream, reply_job);
    assert_eq!(stream.committed_sequence(), 0);
    assert!(!stream.known_air.contains(&key));
    complete_decode_job(&mut stream, column_job);
    settle_ready_commits(&mut stream);
    assert_eq!(stream.committed_sequence(), 2);
    assert!(stream.known_air.contains(&key));
    assert_eq!(stream.outstanding_sub_chunk_count(), 0);
    assert!(stream.authority.terrain().is_chunk_loaded(key.chunk()));
}

#[test]
fn resend_request_waits_for_earlier_same_column_but_other_columns_start() {
    let mut stream = stream();
    stream.submit(1, column(0, 0)).unwrap();
    stream.submit(2, column(0, 0)).unwrap();
    stream.submit(3, column(0, 1)).unwrap();
    assert_eq!(stream.pending_request_count(), 2);
    let requests = stream.take_requests();
    assert_eq!(
        requests.len(),
        1,
        "the resend reservation fences later ready requests"
    );
    assert_eq!(requests[0].chunk.x, 0);
    let first = stream.pending_decode.pop_front().unwrap();
    complete_decode_job(&mut stream, first);
    settle_ready_commits(&mut stream);
    let requests = stream.take_requests();
    assert_eq!(requests.len(), 2);
    assert_eq!(stream.committed_sequence(), 1);
}

#[test]
fn eviction_cancels_early_requests_without_decode_resurrecting_them() {
    let mut stream = stream();
    let chunk = ChunkKey::new(0, 0, 0);
    stream.submit(1, column(0, 0)).unwrap();
    stream.evict_column(chunk);
    assert!(stream.take_requests().is_empty());
    complete_pending_decode_jobs(&mut stream);
    assert!(stream.take_requests().is_empty());
    assert_eq!(stream.outstanding_sub_chunk_count(), 0);
    assert!(stream.authority.terrain().chunk(chunk).is_none());
    assert!(stream.resident.column(chunk).next().is_none());
    assert!(stream.known_air.column(chunk).next().is_none());
}

#[test]
fn retention_cancellation_discards_decode_despite_publisher_interest() {
    let chunk = ChunkKey::new(0, 0, 0);
    for serviced in [false, true] {
        for with_terrain in [false, true] {
            let mut stream = stream();
            stream.submit(1, WorldEvent::ChunkRadiusUpdated(2)).unwrap();
            stream.publisher.center = Some([0; 3]);
            let sequence = if with_terrain {
                stream.submit(2, inline_air_event(chunk.x)).unwrap();
                complete_pending_decode_jobs(&mut stream);
                3
            } else {
                2
            };
            stream.submit(sequence, column(0, chunk.x)).unwrap();
            assert_eq!(stream.pending_request_count(), 1);
            stream.between_frames_service = serviced;

            assert!(stream.retain_local([320.0, 0.0, 0.0]));
            assert!(stream.column_is_active(chunk));
            assert!(stream.take_requests().is_empty());
            assert_eq!(stream.outstanding_sub_chunk_count(), 0);

            complete_pending_decode_jobs(&mut stream);
            assert_eq!(stream.committed_sequence(), sequence);
            assert!(stream.authority.terrain().chunk(chunk).is_none());
            assert!(stream.resident.column(chunk).next().is_none());
            assert!(stream.known_air.column(chunk).next().is_none());
            assert!(stream.take_requests().is_empty());
            assert_eq!(stream.pending_request_work_count(), 0);
            assert_eq!(stream.outstanding_sub_chunk_count(), 0);
        }
    }
}

#[test]
fn retention_cancellation_survives_reentry_and_preserves_a_waiting_resend() {
    let mut stream = stream();
    let chunk = ChunkKey::new(0, 0, 0);
    stream.submit(1, WorldEvent::ChunkRadiusUpdated(2)).unwrap();
    stream.publisher.center = Some([320, 0, 0]);
    stream.submit(2, column(0, chunk.x)).unwrap();
    stream.submit(3, column(0, chunk.x)).unwrap();
    let mut requests = stream.take_requests();
    assert_eq!(requests.len(), 1);
    let canceled = requests.pop().unwrap();
    acknowledge_request_sent(&mut stream, &canceled, Instant::now());
    assert!(stream.retain_local([320.0, 0.0, 0.0]));
    assert!(!stream.column_is_data_interesting(chunk));
    assert!(stream.retain_local([0.0; 3]));
    assert!(stream.column_is_data_interesting(chunk));
    assert!(stream.take_requests().is_empty());
    assert!(stream.requests.deadlines.is_empty());

    let canceled_job = stream.pending_decode.pop_front().unwrap();
    complete_decode_job(&mut stream, canceled_job);
    settle_ready_commits(&mut stream);
    assert_eq!(stream.committed_sequence(), 2);
    assert!(stream.authority.terrain().chunk(chunk).is_none());
    assert!(stream.known_air.column(chunk).next().is_none());
    let mut requests = stream.take_requests();
    assert_eq!(requests.len(), 1, "the resend is still live");
    let replacement = requests.pop().unwrap();
    let key = SubChunkKey::from_chunk(chunk, replacement.base_sub_chunk_y);
    acknowledge_request_sent(&mut stream, &replacement, Instant::now());

    complete_pending_decode_jobs(&mut stream);
    assert_eq!(stream.committed_sequence(), 3);
    assert!(stream.take_requests().is_empty());
    assert!(stream.requests.is_expected(key));
    assert!(
        stream.requests.requested[&chunk][&key.y]
            .response_deadline
            .is_some()
    );
    stream
        .submit(
            4,
            WorldEvent::SubChunks(SubChunkBatchEvent {
                dimension: chunk.dimension,
                entries: vec![SubChunkEntryEvent {
                    position: [key.x, key.y, key.z],
                    result: SubChunkResult::AllAir,
                    diagnostics: None,
                }],
            }),
        )
        .unwrap();
    complete_pending_decode_jobs(&mut stream);
    assert_eq!(stream.committed_sequence(), 4);
    assert!(stream.known_air.contains(&key));
    assert!(stream.authority.terrain().is_chunk_loaded(chunk));
    assert_eq!(stream.outstanding_sub_chunk_count(), 0);
    assert!(stream.requests.deadlines.is_empty());
}

#[test]
fn dimension_change_fences_requests_until_it_commits() {
    let mut stream = stream();
    stream.submit(1, column(0, 0)).unwrap();
    stream
        .submit(
            2,
            WorldEvent::ChangeDimension(ChangeDimensionEvent {
                dimension: 1,
                position: [0.0, 80.0, 0.0],
                ..Default::default()
            }),
        )
        .unwrap();
    stream.submit(3, column(1, 0)).unwrap();
    assert_eq!(stream.take_requests()[0].dimension, 0);
    let first = stream.pending_decode.pop_front().unwrap();
    complete_decode_job(&mut stream, first);
    settle_ready_commits(&mut stream);
    assert_eq!(stream.committed_sequence(), 2);
    let requests = stream.take_requests();
    assert_eq!(requests.len(), 1);
    assert_eq!(requests[0].dimension, 1);
    assert_eq!(requests[0].base_sub_chunk_y, 0);
}

#[test]
fn unsupported_dimension_and_zero_count_do_not_issue_early_requests() {
    let mut stream = stream();
    stream.submit(1, column(i32::MAX, 0)).unwrap();
    stream
        .submit(
            2,
            WorldEvent::LevelChunk(LevelChunkEvent {
                mode: LevelChunkMode::LimitedRequests { highest: 0 },
                dimension: 0,
                x: 0,
                z: 0,
                payload: biome_payload(0, 1),
            }),
        )
        .unwrap();
    assert!(stream.take_requests().is_empty());
    complete_pending_decode_jobs(&mut stream);
    assert_eq!(stream.pending_request_work_count(), 0);
    assert!(stream.requests.level_chunks.is_empty());
}

#[test]
fn request_admission_does_not_cross_missing_sequences_or_inventory_barriers() {
    for barrier in [false, true] {
        let mut stream = stream();
        stream.submit(1, column(0, 0)).unwrap();
        if barrier {
            stream.commit_barrier(2).unwrap();
        }
        stream.submit(3, column(0, 1)).unwrap();
        assert_eq!(stream.take_requests().len(), 1);
        assert_eq!(stream.pending_request_count(), 0);
        let first = stream.pending_decode.pop_front().unwrap();
        complete_decode_job(&mut stream, first);
        settle_ready_commits(&mut stream);
        assert_eq!(stream.pending_request_count(), usize::from(barrier));
        assert_eq!(stream.committed_sequence(), if barrier { 2 } else { 1 });
    }
}
