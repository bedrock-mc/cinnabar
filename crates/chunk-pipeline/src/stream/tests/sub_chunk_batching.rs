use super::*;

/// Creates a request owner without worker or transport activity.
fn request_stream() -> WorldStream {
    WorldStream::new(WorldBootstrap {
        local_player_unique_id: 1,
        dimension: 0,
        local_player_runtime_id: 1,
        player_position: [0.0; 3],
        world_spawn_position: [0; 3],
        air_network_id: protocol_ids::SEQUENTIAL_AIR_NETWORK_ID,
        block_network_ids_are_hashes: false,
    })
}

/// Reads the same immutable positions that the packet encoder consumes.
fn positions(batch: &SubChunkRequestBatch) -> Vec<[i32; 3]> {
    batch
        .receipt
        .members
        .iter()
        .map(|member| member.position)
        .collect()
}

#[test]
fn grouped_sparse_members_keep_column_order_and_stop_before_reservation() {
    let mut stream = request_stream();
    let first = ChunkKey::new(0, 8, -2);
    stream.enqueue_request(first, -4, 3, None);
    stream.complete_requested_sub_chunk(SubChunkKey::from_chunk(first, -3), true);
    stream.enqueue_request(ChunkKey::new(0, 0, 1), -2, 1, None);
    stream.requests.queue.reserve(99);
    stream.enqueue_request(ChunkKey::new(0, 1, 1), -1, 1, None);

    let batch = stream.prepare_sub_chunk_request([2, 0, -1], 8).unwrap();
    assert_eq!(positions(&batch), [[8, -4, -2], [8, -2, -2], [0, -2, 1]]);
    assert!(stream.prepare_sub_chunk_request([2, 0, -1], 8).is_none());
    stream.record_sub_chunk_request_transport_pending(&batch);
    assert_eq!(stream.requests.transport.len(), 1);
    assert!(stream.requests.deadlines.is_empty());
    let sent_at = Instant::now();
    stream.acknowledge_sub_chunk_request_sent(&batch.receipt, sent_at);
    assert_eq!(stream.requests.deadlines.len(), 3);
    assert!(!stream.requests.deadlines.iter().any(|(_, key)| key.y == -3));
    stream.acknowledge_sub_chunk_request_sent(&batch.receipt, sent_at + Duration::from_secs(1));
    assert_eq!(stream.stats.publication_stages.requests_sent, 1);
    assert!(
        stream
            .requests
            .deadlines
            .iter()
            .all(|(deadline, _)| *deadline == sent_at + SUB_CHUNK_RESPONSE_TIMEOUT)
    );
}

#[test]
fn work_budget_counts_intents_and_restoration_cannot_be_displaced_by_deferred_work() {
    let mut stream = request_stream();
    for x in 0..OUTBOUND_REQUEST_CAPACITY as i32 {
        stream.enqueue_request(ChunkKey::new(0, x, 0), -4, 2, None);
    }
    let deferred = PendingSubChunkRequest::new(ChunkKey::new(0, 90, 0), -4, 1, 999).unwrap();
    stream.requests.deferred_recovery.push_back(deferred);
    assert!(stream.prepare_sub_chunk_request([0; 3], 0).is_none());
    assert_eq!(stream.pending_request_count(), OUTBOUND_REQUEST_CAPACITY);
    let batch = stream.prepare_sub_chunk_request([0; 3], 3).unwrap();
    assert_eq!(positions(&batch).len(), 6);
    assert_eq!(
        stream.pending_request_count(),
        OUTBOUND_REQUEST_CAPACITY - 3
    );
    stream.retry_sub_chunk_request_front(batch).unwrap();
    let retried = stream.prepare_sub_chunk_request([1, 0, 1], 3).unwrap();
    assert_eq!(
        positions(&retried),
        [
            [0, -4, 0],
            [0, -3, 0],
            [1, -4, 0],
            [1, -3, 0],
            [2, -4, 0],
            [2, -3, 0]
        ]
    );
}

#[test]
fn maximum_intent_budget_produces_one_bounded_packet() {
    let mut stream = request_stream();
    for x in 0..OUTBOUND_REQUEST_CAPACITY as i32 {
        stream.enqueue_request(ChunkKey::new(0, x, 0), 0, MAX_SUB_CHUNK_COLUMN_HEIGHT, None);
    }
    let batch = stream
        .prepare_sub_chunk_request([0; 3], usize::MAX)
        .unwrap();
    assert_eq!(batch.receipt.members.len(), MAX_SUB_CHUNK_REQUESTS);
    stream.record_sub_chunk_request_transport_pending(&batch);
    assert_eq!(stream.requests.transport.len(), 1);
    assert!(
        stream
            .prepare_sub_chunk_request([0; 3], usize::MAX)
            .is_none()
    );
}

#[test]
fn overlapping_range_and_exact_retry_encode_one_attempt_per_current_slot() {
    let mut stream = request_stream();
    let chunk = ChunkKey::new(0, 0, 0);
    stream.enqueue_request(chunk, -4, 2, None);
    assert!(stream.enqueue_exact_retry(SubChunkKey::from_chunk(chunk, -4)));
    let batch = stream.prepare_sub_chunk_request([0; 3], 2).unwrap();
    assert_eq!(positions(&batch), [[0, -4, 0], [0, -3, 0]]);
    stream.record_sub_chunk_request_transport_pending(&batch);
    assert_eq!(
        stream.requests.requested[&chunk][&-4].pending_transport_attempts,
        1
    );
}

#[test]
fn restoration_keeps_surviving_sparse_demands_ahead_of_new_work() {
    let mut stream = request_stream();
    let a = ChunkKey::new(0, 0, 0);
    let b = ChunkKey::new(0, 1, 0);
    stream.enqueue_request(a, -4, 2, None);
    stream.enqueue_request(b, -4, 1, None);
    let batch = stream.prepare_sub_chunk_request([0; 3], 2).unwrap();
    stream.requests.purge_columns(&BTreeSet::from([b]));
    stream.complete_requested_sub_chunk(SubChunkKey::from_chunk(a, -4), true);
    stream.enqueue_request(b, -4, 1, None);
    stream.retry_sub_chunk_request_front(batch).unwrap();
    let retried = stream.prepare_sub_chunk_request([1, 0, 0], 2).unwrap();
    assert_eq!(positions(&retried), [[0, -3, 0], [1, -4, 0]]);
    assert!(stream.requests.transport.is_empty());
    assert!(stream.requests.deadlines.is_empty());
}

#[test]
fn unencodable_slots_retire_without_terrain_success_and_do_not_block_later_members() {
    let mut stream = request_stream();
    let rejected = [
        ChunkKey::new(0, -128, 0),
        ChunkKey::new(0, 128, 0),
        ChunkKey::new(1, 0, 0),
    ];
    for chunk in rejected {
        stream.enqueue_request(chunk, -4, 1, None);
    }
    let accepted = ChunkKey::new(0, 127, -127);
    stream.enqueue_request(accepted, -128, 2, None);
    let batch = stream.prepare_sub_chunk_request([0; 3], 8).unwrap();
    assert_eq!(positions(&batch), [[127, -127, -127]]);
    assert_eq!(stream.outstanding_sub_chunk_count(), 1);
    assert!(stream.loaded_columns.is_empty());
    assert!(stream.known_air.is_empty());
    assert!(stream.resident.is_empty());
    assert!(stream.requests.recoveries.is_empty());
    assert!(
        rejected
            .iter()
            .all(|chunk| !stream.requests.requested.contains_key(chunk))
    );
    assert!(
        !stream
            .requests
            .is_expected(SubChunkKey::from_chunk(accepted, -128))
    );
}

#[test]
fn unencodable_members_keep_columns_incomplete_after_surviving_replies() {
    let mut stream = request_stream();
    let chunk = ChunkKey::new(0, 0, 0);
    let rejected = SubChunkKey::from_chunk(chunk, -4);
    let accepted = SubChunkKey::from_chunk(chunk, -3);
    stream.enqueue_request(chunk, rejected.y, 2, None);
    let batch = stream.prepare_sub_chunk_request([0, 124, 0], 1).unwrap();
    assert_eq!(positions(&batch), [[accepted.x, accepted.y, accepted.z]]);

    apply_sub_chunk_result(&mut stream, accepted, super::PreparedSubChunkResult::AllAir);

    assert_eq!(stream.outstanding_sub_chunk_count(), 0);
    assert!(!stream.loaded_columns.contains(&chunk));
    assert!(!stream.authority.terrain().is_chunk_loaded(chunk));
    assert!(!stream.authority.terrain().is_sub_chunk_loaded(rejected));
    assert!(stream.authority.terrain().is_sub_chunk_loaded(accepted));
    assert!(!stream.known_air.contains(&rejected));
    assert!(stream.requests.recoveries.is_empty());

    stream.enqueue_request(chunk, accepted.y, 0, None);
    assert!(!stream.authority.terrain().is_sub_chunk_loaded(rejected));
    stream.enqueue_request(chunk, accepted.y, 1, None);
    apply_sub_chunk_result(&mut stream, accepted, super::PreparedSubChunkResult::AllAir);
    assert!(!stream.authority.terrain().is_sub_chunk_loaded(rejected));

    stream.evict_column(chunk);
    stream.enqueue_request(chunk, accepted.y, 1, None);
    let fresh = stream.prepare_sub_chunk_request([0; 3], 1).unwrap();
    assert_eq!(positions(&fresh), [[accepted.x, accepted.y, accepted.z]]);
    apply_sub_chunk_result(&mut stream, accepted, super::PreparedSubChunkResult::AllAir);
    assert!(stream.loaded_columns.contains(&chunk));
    assert!(stream.authority.terrain().is_chunk_loaded(chunk));
}

#[test]
fn recovering_surviving_members_does_not_complete_unencodable_heights() {
    let mut stream = request_stream();
    let chunk = ChunkKey::new(0, 0, 0);
    let rejected = SubChunkKey::from_chunk(chunk, -4);
    let accepted = SubChunkKey::from_chunk(chunk, -3);
    stream.enqueue_request(chunk, rejected.y, 2, None);
    let batch = stream.prepare_sub_chunk_request([0, 124, 0], 1).unwrap();
    assert_eq!(positions(&batch), [[accepted.x, accepted.y, accepted.z]]);
    stream.complete_requested_sub_chunk(accepted, false);
    let range = stream.authority.dimension_range(chunk.dimension).unwrap();
    stream.enqueue_exact_recovery_requests(chunk, range, &[accepted.y], None);
    let retry = stream.prepare_sub_chunk_request([0, 124, 0], 1).unwrap();
    assert_eq!(positions(&retry), [[accepted.x, accepted.y, accepted.z]]);

    apply_sub_chunk_result(&mut stream, accepted, super::PreparedSubChunkResult::AllAir);

    assert!(stream.requests.recoveries.is_empty());
    assert_eq!(stream.outstanding_sub_chunk_count(), 0);
    assert!(!stream.loaded_columns.contains(&chunk));
    assert!(!stream.authority.terrain().is_chunk_loaded(chunk));
    assert!(!stream.authority.terrain().is_sub_chunk_loaded(rejected));
    assert!(stream.authority.terrain().is_sub_chunk_loaded(accepted));

    stream.enqueue_exact_recovery_requests(chunk, range, &[rejected.y], None);
    let recovered = stream.prepare_sub_chunk_request([0; 3], 1).unwrap();
    assert_eq!(
        positions(&recovered),
        [[rejected.x, rejected.y, rejected.z]]
    );
    apply_sub_chunk_result(&mut stream, rejected, super::PreparedSubChunkResult::AllAir);
    assert!(stream.loaded_columns.contains(&chunk));
    assert!(stream.authority.terrain().is_chunk_loaded(chunk));
}

#[test]
fn old_receipt_cannot_arm_a_recreated_slot_or_consume_a_new_packet() {
    let mut stream = request_stream();
    let chunk = ChunkKey::new(0, 0, 0);
    stream.enqueue_request(chunk, -4, 1, None);
    let old = stream.prepare_sub_chunk_request([0; 3], 1).unwrap();
    stream.record_sub_chunk_request_transport_pending(&old);
    stream.requests.purge_columns(&BTreeSet::from([chunk]));
    stream.enqueue_request(chunk, -4, 1, None);
    let current = stream.prepare_sub_chunk_request([0; 3], 1).unwrap();
    stream.record_sub_chunk_request_transport_pending(&current);
    assert_ne!(
        old.receipt.members[0].demand,
        current.receipt.members[0].demand
    );
    let now = Instant::now();
    stream.acknowledge_sub_chunk_request_sent(&old.receipt, now);
    assert!(stream.requests.deadlines.is_empty());
    assert_eq!(stream.requests.transport.len(), 1);
    let mut altered = current.receipt.clone();
    altered.members = old.receipt.members.clone();
    stream.acknowledge_sub_chunk_request_sent(&altered, now);
    assert_eq!(stream.requests.transport.len(), 1);
    stream.acknowledge_sub_chunk_request_sent(&current.receipt, now);
    assert_eq!(stream.requests.deadlines.len(), 1);
    assert_eq!(stream.requests.requested[&chunk][&-4].confirmed_attempts, 1);
}

#[test]
fn early_reply_and_member_retirement_do_not_rearm_completed_positions() {
    let mut stream = request_stream();
    let a = ChunkKey::new(0, 0, 0);
    let b = ChunkKey::new(0, 1, 0);
    stream.enqueue_request(a, -4, 2, None);
    stream.enqueue_request(b, -4, 1, None);
    let batch = stream.prepare_sub_chunk_request([0; 3], 2).unwrap();
    stream.record_sub_chunk_request_transport_pending(&batch);
    let admitted = SubChunkKey::from_chunk(a, -4);
    stream.record_sub_chunk_reply_admission(&protocol::SubChunkReplyAdmissionEvent {
        dimension: 0,
        positions: vec![[admitted.x, admitted.y, admitted.z]],
    });
    stream.complete_requested_sub_chunk(SubChunkKey::from_chunk(a, -3), true);
    stream.requests.purge_columns(&BTreeSet::from([b]));
    stream.acknowledge_sub_chunk_request_sent(&batch.receipt, Instant::now());
    assert!(stream.requests.deadlines.is_empty());
    assert_eq!(
        stream
            .requests
            .correlated_attempts
            .get(&SubChunkKey::from_chunk(a, -3))
            .unwrap()
            .confirmed_attempts,
        1
    );
    assert!(!stream.requests.requested.contains_key(&b));
    assert_eq!(stream.requests.transport.len(), 0);
}

#[test]
fn transient_reply_before_write_confirmation_does_not_arm_the_answered_attempt() {
    let mut stream = request_stream();
    let chunk = ChunkKey::new(0, 0, 0);
    let key = SubChunkKey::from_chunk(chunk, -4);
    stream.enqueue_request(chunk, -4, 1, None);
    let initial = stream.prepare_sub_chunk_request([0; 3], 1).unwrap();
    stream.record_sub_chunk_request_transport_pending(&initial);
    apply_sub_chunk_result(
        &mut stream,
        key,
        PreparedSubChunkResult::Unavailable(SubChunkUnavailable::ChunkNotFound),
    );
    let retry = stream.prepare_sub_chunk_request([0; 3], 1).unwrap();
    stream.record_sub_chunk_request_transport_pending(&retry);
    let now = Instant::now();
    stream.acknowledge_sub_chunk_request_sent(&initial.receipt, now);
    assert!(stream.requests.deadlines.is_empty());
    stream.acknowledge_sub_chunk_request_sent(&retry.receipt, now);
    assert_eq!(stream.requests.deadlines.len(), 1);
    assert_eq!(stream.requests.requested[&chunk][&-4].confirmed_attempts, 1);
}

/// Coordinate-only replies consume a confirmed attempt before an unconfirmed retry.
#[test]
fn mixed_confirmed_timeout_and_pending_retry_keep_local_reply_correlation() {
    let mut stream = request_stream();
    let chunk = ChunkKey::new(0, 0, 0);
    let key = SubChunkKey::from_chunk(chunk, -4);
    stream.enqueue_request(chunk, key.y, 1, None);
    let initial = stream.prepare_sub_chunk_request([0; 3], 1).unwrap();
    stream.record_sub_chunk_request_transport_pending(&initial);
    let sent_at = Instant::now();
    stream.acknowledge_sub_chunk_request_sent(&initial.receipt, sent_at);
    let expired_at = sent_at + SUB_CHUNK_RESPONSE_TIMEOUT;
    stream.expire_sub_chunk_deadlines(expired_at);
    let retry = stream.prepare_sub_chunk_request([0; 3], 1).unwrap();
    stream.record_sub_chunk_request_transport_pending(&retry);
    let pending = stream.requests.requested[&chunk][&key.y];
    assert_eq!(pending.confirmed_attempts, 1);
    assert_eq!(pending.pending_transport_attempts, 1);

    stream.record_sub_chunk_reply_admission(&protocol::SubChunkReplyAdmissionEvent {
        dimension: key.dimension,
        positions: vec![[key.x, key.y, key.z]],
    });
    apply_sub_chunk_result(
        &mut stream,
        key,
        PreparedSubChunkResult::Unavailable(SubChunkUnavailable::ChunkNotFound),
    );
    let pending = stream.requests.requested[&chunk][&key.y];
    assert_eq!(pending.confirmed_attempts, 0);
    assert_eq!(pending.pending_transport_attempts, 1);
    assert_eq!(pending.answered_transport_packet, 0);
    assert!(stream.requests.deadlines.is_empty());

    let confirmed_at = expired_at + Duration::from_secs(1);
    stream.acknowledge_sub_chunk_request_sent(&retry.receipt, confirmed_at);
    let confirmed = stream.requests.requested[&chunk][&key.y];
    assert_eq!(confirmed.confirmed_attempts, 1);
    assert_eq!(confirmed.pending_transport_attempts, 0);
    assert_eq!(
        confirmed.response_deadline,
        Some(confirmed_at + SUB_CHUNK_RESPONSE_TIMEOUT)
    );
    let packets_sent = stream.stats.publication_stages.requests_sent;
    stream
        .acknowledge_sub_chunk_request_sent(&retry.receipt, confirmed_at + Duration::from_secs(1));
    assert_eq!(stream.requests.requested[&chunk][&key.y], confirmed);
    assert_eq!(stream.stats.publication_stages.requests_sent, packets_sent);
    assert!(stream.requests.transport.is_empty());
}

#[test]
fn dimension_reset_and_replacement_stream_reject_old_receipts_and_rollback() {
    let mut stream = request_stream();
    let chunk = ChunkKey::new(0, 0, 0);
    stream.enqueue_request(chunk, -4, 1, None);
    let old = stream.prepare_sub_chunk_request([0; 3], 1).unwrap();
    stream.record_sub_chunk_request_transport_pending(&old);
    stream
        .submit(
            1,
            WorldEvent::ChangeDimension(ChangeDimensionEvent {
                dimension: 1,
                position: [0.0; 3],
                ..Default::default()
            }),
        )
        .unwrap();
    stream.acknowledge_sub_chunk_request_sent(&old.receipt, Instant::now());
    assert!(stream.requests.deadlines.is_empty());
    assert_eq!(stream.requests.transport.len(), 0);
    let old_receipt = old.receipt.clone();
    stream.retry_sub_chunk_request_front(old).unwrap();
    assert_eq!(stream.pending_request_count(), 0);

    let mut replacement = request_stream();
    replacement.enqueue_request(chunk, -4, 1, None);
    let packet = replacement.prepare_sub_chunk_request([0; 3], 1).unwrap();
    replacement.record_sub_chunk_request_transport_pending(&packet);
    replacement.acknowledge_sub_chunk_request_sent(&old_receipt, Instant::now());
    assert!(replacement.requests.deadlines.is_empty());
    assert_eq!(replacement.requests.transport.len(), 1);
}

#[test]
fn disjoint_local_teleport_accepts_destination_chunks_before_publisher_update() {
    let mut stream = WorldStream::new(WorldBootstrap {
        local_player_unique_id: 1,
        dimension: 0,
        local_player_runtime_id: 1,
        player_position: [0.5, 70.0, 0.5],
        world_spawn_position: [0, 70, 0],
        air_network_id: protocol_ids::SEQUENTIAL_AIR_NETWORK_ID,
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
    let receipt = record_request_transport(&mut stream, &source_request);
    stream.acknowledge_sub_chunk_request_sent(&receipt, sent_at);
    assert!(stream.tracked_columns().contains(&source));
    assert!(!stream.requests.deadlines.is_empty());

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

    assert_eq!(stream.publisher.center, Some([1_040, 70, 1_040]));
    assert_eq!(stream.committed_view_cohort(), None);
    assert!(!stream.tracked_columns().contains(&source));
    assert!(stream.requests.deadlines.is_empty());
    assert_eq!(stream.outstanding_sub_chunk_count(), 0);
    let armed = stream.publication_snapshot(destination);
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
    assert!(stream.requests.requested.contains_key(&destination));
    assert!(stream.publisher.required_columns.contains(&destination));

    stream
        .submit(
            6,
            WorldEvent::PublisherUpdate(PublisherUpdateEvent {
                center: [1_040, 70, 1_040],
                radius_blocks: 128,
            }),
        )
        .unwrap();

    assert!(stream.requests.requested.contains_key(&destination));
    assert!(stream.publisher.required_columns.contains(&destination));
    assert_eq!(stream.publisher.epoch, 2);
    assert_eq!(stream.committed_view_cohort().unwrap().center, [65, 65]);
    let consumed = stream.publication_snapshot(destination);
    assert!(!consumed.local_reset_armed);
    assert_eq!(consumed.local_resets_armed, 1);
    assert_eq!(consumed.local_resets_consumed, 1);
    assert!(consumed.player_column_required);

    stream.poll([1_040.5, 70.0, 1_040.5], 0);
    let request = stream.pop_next_request().unwrap();
    assert_eq!(request.chunk, destination);
    assert_eq!(
        stream
            .publication_snapshot(destination)
            .local_reset_dispatch_total,
        0,
        "selection alone is not successful-send evidence",
    );
    record_request_transport(&mut stream, &request);
    let dispatched = stream.publication_snapshot(destination);
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
                ..Default::default()
            }),
        )
        .unwrap();
    let changed_dimension = stream.publication_snapshot(ChunkKey::new(1, 0, 0));
    assert_eq!(changed_dimension.local_resets_armed, 0);
    assert_eq!(changed_dimension.local_resets_consumed, 0);
    assert_eq!(changed_dimension.local_reset_dispatch_count, 0);
    assert_eq!(changed_dimension.local_reset_dispatch_total, 0);
}

#[test]
fn request_modes_use_vanilla_dimension_base_and_bounded_counts() {
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
                x: -2,
                z: 5,
                mode: LevelChunkMode::LimitedRequests { highest: u16::MAX },
                payload: biome_payload(0, 1),
            }),
        )
        .unwrap();
    complete_pending_decode_jobs(&mut stream);
    let overworld_requests = stream.take_requests();
    assert_eq!(overworld_requests.len(), 1);
    assert_eq!(overworld_requests[0].chunk.dimension, 0);
    assert_eq!(overworld_requests[0].chunk, ChunkKey::new(0, -2, 5));
    assert_eq!(overworld_requests[0].base_sub_chunk_y, -4);
    assert_eq!(overworld_requests[0].count, 24);
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
    stream
        .submit(
            3,
            WorldEvent::LevelChunk(LevelChunkEvent {
                dimension: 1,
                x: 7,
                z: -9,
                mode: LevelChunkMode::LimitlessRequests,
                payload: biome_payload(1, 1),
            }),
        )
        .unwrap();
    complete_pending_decode_jobs(&mut stream);

    let requests = stream.take_requests();
    assert_eq!(requests.len(), 1);
    assert_eq!(requests[0].chunk.dimension, 1);
    assert_eq!(requests[0].base_sub_chunk_y, 0);
    assert_eq!(requests[0].count, 8);
}

#[test]
fn publication_distinguishes_ready_from_transport_pending_requests() {
    let mut stream = WorldStream::new(WorldBootstrap {
        local_player_unique_id: 1,
        dimension: 0,
        local_player_runtime_id: 1,
        player_position: [0.0; 3],
        world_spawn_position: [0; 3],
        air_network_id: protocol_ids::SEQUENTIAL_AIR_NETWORK_ID,
        block_network_ids_are_hashes: false,
    });
    let chunk = ChunkKey::new(0, 2, -3);
    stream
        .submit(
            1,
            WorldEvent::LevelChunk(LevelChunkEvent {
                dimension: chunk.dimension,
                x: chunk.x,
                z: chunk.z,
                mode: LevelChunkMode::LimitedRequests { highest: u16::MAX },
                payload: biome_payload(0, 1),
            }),
        )
        .unwrap();
    complete_pending_decode_jobs(&mut stream);

    let ready = stream.publication_snapshot(chunk).stages;
    assert_eq!(ready.requests_constructed, 1);
    assert_eq!(ready.requests_ready, 1);
    assert_eq!(ready.requests_transport_pending, 0);
    assert_eq!(ready.requests_sent, 0);

    let request = stream.pop_next_request().unwrap();
    let receipt = record_request_transport(&mut stream, &request);
    let pending = stream.publication_snapshot(chunk).stages;
    assert_eq!(pending.requests_constructed, 1);
    assert_eq!(pending.requests_ready, 0);
    assert_eq!(pending.requests_transport_pending, 1);
    assert_eq!(pending.requests_sent, 0);

    stream.acknowledge_sub_chunk_request_sent(&receipt, Instant::now());
    let sent = stream.publication_snapshot(chunk).stages;
    assert_eq!(sent.requests_constructed, 1);
    assert_eq!(sent.requests_ready, 0);
    assert_eq!(sent.requests_transport_pending, 0);
    assert_eq!(sent.requests_sent, 1);
}
