use super::*;

/// Commits one request-mode column and returns its unfilled slot.
pub(super) fn stream_with_one_expected_sub_chunk() -> (WorldStream, SubChunkKey) {
    let mut stream = WorldStream::new(WorldBootstrap {
        local_player_unique_id: 1,
        dimension: 0,
        local_player_runtime_id: 1,
        player_position: [0.0; 3],
        world_spawn_position: [0; 3],
        air_network_id: protocol_ids::SEQUENTIAL_AIR_NETWORK_ID,
        block_network_ids_are_hashes: false,
    });
    let key = SubChunkKey::new(0, 0, -4, 0);
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
    complete_pending_decode_jobs(&mut stream);
    assert_eq!(stream.take_requests().len(), 1);
    (stream, key)
}

#[test]
fn abandoned_cached_column_recovery_enters_bounded_request_scheduler() {
    let mut stream = WorldStream::new(WorldBootstrap {
        dimension: 0,
        local_player_runtime_id: 1,
        local_player_unique_id: 1,
        player_position: [0.0; 3],
        world_spawn_position: [0; 3],
        air_network_id: protocol_ids::SEQUENTIAL_AIR_NETWORK_ID,
        block_network_ids_are_hashes: false,
    });
    stream
        .submit(
            1,
            WorldEvent::ChunkResync(ChunkResyncEvent {
                dimension: 0,
                x: 0,
                z: 0,
                requested_sub_chunks: None,
                requested_sub_chunk_ys: None,
            }),
        )
        .expect("cache recovery admission");

    let requests = stream.take_requests();
    assert_eq!(requests.len(), 1);
    assert_eq!(requests[0].chunk, ChunkKey::new(0, 0, 0));
    assert_eq!(requests[0].base_sub_chunk_y, -4);
    assert_eq!(requests[0].count, 24);
}

#[test]
fn exact_recovery_keeps_disjoint_ranges_expected_when_outbound_is_saturated() {
    let mut stream = WorldStream::new(WorldBootstrap {
        dimension: 0,
        local_player_runtime_id: 1,
        local_player_unique_id: 1,
        player_position: [0.0; 3],
        world_spawn_position: [0; 3],
        air_network_id: protocol_ids::SEQUENTIAL_AIR_NETWORK_ID,
        block_network_ids_are_hashes: false,
    });
    for index in 0..(super::OUTBOUND_REQUEST_CAPACITY - 1) {
        let chunk = ChunkKey::new(0, 20 + i32::try_from(index).unwrap(), 0);
        stream.requests.queue.push_ready(
            PendingSubChunkRequest {
                demand: 0,
                chunk,
                base_sub_chunk_y: -4,
                count: 1,
            },
            false,
        );
    }

    stream
        .submit(
            1,
            WorldEvent::ChunkResync(ChunkResyncEvent {
                dimension: 0,
                x: 0,
                z: 0,
                requested_sub_chunks: None,
                requested_sub_chunk_ys: Some(vec![-4, -2]),
            }),
        )
        .expect("the reserved first exact range has admission capacity");

    assert_eq!(
        stream.requests.queue.len(),
        super::OUTBOUND_REQUEST_CAPACITY
    );
    assert_eq!(stream.requests.deferred_recovery.len(), 1);
    assert_eq!(stream.requests.requested[&ChunkKey::new(0, 0, 0)].len(), 2);

    let requests = stream.take_requests();
    let recovery_ranges = requests
        .iter()
        .filter(|request| request.chunk == ChunkKey::new(0, 0, 0))
        .map(|request| (request.base_sub_chunk_y, request.count))
        .collect::<BTreeSet<_>>();
    assert_eq!(recovery_ranges, BTreeSet::from([(-4, 1), (-2, 1)]));
    assert!(stream.requests.deferred_recovery.is_empty());
    assert_eq!(stream.requests.requested[&ChunkKey::new(0, 0, 0)].len(), 2);
}

/// Applies a decoded result through the owning stream commit path.
pub(super) fn apply_sub_chunk_result(
    stream: &mut WorldStream,
    key: SubChunkKey,
    result: super::PreparedSubChunkResult,
) {
    stream.apply_prepared(super::PreparedWorldEvent::SubChunks {
        dimension: key.dimension,
        entries: vec![PreparedSubChunk {
            diagnostics: None,
            position: [key.x, key.y, key.z],
            result,
        }],
        duration: std::time::Duration::ZERO,
    });
}

/// Commits a column and pops its intent before any transport admission.
pub(super) fn stream_with_unsent_sub_chunks(
    count: u16,
) -> (WorldStream, Vec<SubChunkKey>, super::PendingSubChunkRequest) {
    let mut stream = WorldStream::new(WorldBootstrap {
        local_player_unique_id: 1,
        dimension: 0,
        local_player_runtime_id: 1,
        player_position: [0.0; 3],
        world_spawn_position: [0; 3],
        air_network_id: protocol_ids::SEQUENTIAL_AIR_NETWORK_ID,
        block_network_ids_are_hashes: false,
    });
    let chunk = ChunkKey::new(0, 0, 0);
    stream
        .submit(
            1,
            WorldEvent::LevelChunk(LevelChunkEvent {
                dimension: 0,
                x: chunk.x,
                z: chunk.z,
                mode: LevelChunkMode::LimitedRequests { highest: count },
                payload: biome_payload(0, 1),
            }),
        )
        .unwrap();
    complete_pending_decode_jobs(&mut stream);
    let request = stream
        .pop_next_request()
        .expect("request-mode LevelChunk should enqueue one request");
    let keys = (0..count)
        .map(|offset| SubChunkKey::from_chunk(chunk, -4 + i32::from(offset)))
        .collect();
    (stream, keys, request)
}

/// Admits an existing fixture intent through the real per-packet transport bookkeeping.
pub(super) fn record_request_transport(
    stream: &mut WorldStream,
    request: &super::PendingSubChunkRequest,
) -> SubChunkRequestReceipt {
    let receipt = SubChunkRequestReceipt {
        owner: stream.terrain_generation(),
        id: stream.requests.allocate_identity(),
        dimension: request.chunk.dimension,
        members: request
            .keys()
            .map(|key| SubChunkRequestMember {
                position: [key.x, key.y, key.z],
                demand: request.demand,
            })
            .collect::<Vec<_>>()
            .into(),
    };
    stream.record_sub_chunk_request_transport_pending(&SubChunkRequestBatch {
        receipt: receipt.clone(),
        intents: vec![*request],
    });
    receipt
}

/// Completes the real admission/write receipt pair for one fixture column intent.
pub(super) fn acknowledge_request_sent(
    stream: &mut WorldStream,
    request: &super::PendingSubChunkRequest,
    sent_at: Instant,
) {
    let receipt = record_request_transport(stream, request);
    stream.acknowledge_sub_chunk_request_sent(&receipt, sent_at);
}
