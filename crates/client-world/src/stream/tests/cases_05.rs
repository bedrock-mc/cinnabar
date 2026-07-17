use super::*;

#[test]
fn phase2_gate_has_explicit_minimum_frame_and_burst_bounds() {
    let config = PublicationServiceConfig::PHASE2_GATE;
    assert_eq!(config.minimum_items_per_second, 4_096);
    assert_eq!(config.minimum_bytes_per_second, 64 * 1024 * 1024);
    assert_eq!(config.target_items_per_second, 8_192);
    assert_eq!(config.target_bytes_per_second, 128 * 1024 * 1024);
    assert_eq!(config.maximum_frame_items, 512);
    assert_eq!(config.maximum_frame_bytes, 64 * 1024 * 1024);
    assert_eq!(config.maximum_burst_items, 8_192);
    assert_eq!(config.maximum_burst_bytes, 128 * 1024 * 1024);
    assert_eq!(config.maximum_zero_byte_operations_per_frame, 256);
    assert!(config.minimum_bytes_per_second <= config.target_bytes_per_second);
    assert!(config.maximum_frame_bytes <= config.maximum_burst_bytes);
}

#[test]
fn phase2_identity_carriers_are_fixed_size_and_fully_qualified() {
    let classes = [
        RequestClass::PlayerRetry,
        RequestClass::PlayerInitial,
        RequestClass::VisibleRetry,
        RequestClass::VisibleInitial,
        RequestClass::PrefetchRetry,
        RequestClass::PrefetchInitial,
    ];
    assert_eq!(classes.len(), 6);
    assert_eq!(std::mem::size_of::<RequestClass>(), 1);
    assert_eq!(std::mem::size_of::<BuildProfileIdentity>(), 1);
    assert_eq!(std::mem::size_of::<PresentModeIdentity>(), 1);

    let cohort = CohortManifestIdentity {
        session_generation: 7,
        required_cohort_hash: 11,
        generation_manifest_hash: 13,
        entry_count: 17,
    };
    let presentation = Phase2PresentationSnapshot {
        build_profile: BuildProfileIdentity::Release,
        graphics_identity_sha256: [19; 32],
        requested_present_mode: PresentModeIdentity::Fifo,
        effective_present_mode: PresentModeIdentity::Fifo,
        assets_manifest_sha256: [23; 32],
        publisher_disk: cohort,
        resident: cohort,
        allocation: cohort,
        visible: cohort,
        submitted: cohort,
        gpu_presented: cohort,
    };
    assert_eq!(presentation.publisher_disk.session_generation, 7);
    assert_eq!(presentation.publisher_disk.required_cohort_hash, 11);
    assert_eq!(presentation.graphics_identity_sha256, [19; 32]);
    assert_eq!(presentation.assets_manifest_sha256, [23; 32]);
}

#[test]
fn publication_snapshot_separates_every_stage_and_subchunk_outcome() {
    let started = Instant::now();
    let (mut stream, keys, initial) = stream_with_unsent_sub_chunks(6);
    acknowledge_request_sent(&mut stream, &initial, started);

    let decoded = world::DecodedSubChunk::decode(
        keys[0],
        include_bytes!(concat!(
            env!("CARGO_MANIFEST_DIR"),
            "/../world/fixtures/uniform_non_air.bin"
        )),
    )
    .unwrap();
    apply_sub_chunk_result(
        &mut stream,
        keys[0],
        super::PreparedSubChunkResult::Decoded(Ok(decoded)),
    );
    apply_sub_chunk_result(&mut stream, keys[1], super::PreparedSubChunkResult::AllAir);
    apply_sub_chunk_result(
        &mut stream,
        keys[2],
        super::PreparedSubChunkResult::Unavailable(SubChunkUnavailable::Unknown(0xff)),
    );

    stream
        .requested_sub_chunks
        .get_mut(&keys[3].chunk())
        .unwrap()
        .get_mut(&keys[3].y)
        .unwrap()
        .retry_attempts = super::MAX_SUB_CHUNK_RETRIES;
    let malformed = world::DecodedSubChunk::decode(keys[3], &[0xff]).unwrap_err();
    apply_sub_chunk_result(
        &mut stream,
        keys[3],
        super::PreparedSubChunkResult::Decoded(Err(malformed)),
    );

    stream
        .requested_sub_chunks
        .get_mut(&keys[4].chunk())
        .unwrap()
        .remove(&keys[4].y);
    apply_sub_chunk_result(&mut stream, keys[4], super::PreparedSubChunkResult::AllAir);

    for attempt in 1..=super::MAX_SUB_CHUNK_RETRIES {
        let deadline = started + super::SUB_CHUNK_RESPONSE_TIMEOUT * u32::from(attempt);
        stream.expire_sub_chunk_deadlines(deadline);
        let retry = stream
            .pop_next_request()
            .expect("timeout should queue retry");
        acknowledge_request_sent(&mut stream, &retry, deadline);
    }
    stream.expire_sub_chunk_deadlines(
        started
            + super::SUB_CHUNK_RESPONSE_TIMEOUT
                * u32::from(super::MAX_SUB_CHUNK_RETRIES.saturating_add(1)),
    );

    stream.apply_immediate(
        WorldEvent::PublisherUpdate(PublisherUpdateEvent {
            center: [0, 64, 0],
            radius_blocks: 16,
        }),
        None,
    );

    let snapshot = stream.phase2_publication_snapshot(keys[0].chunk());
    assert_eq!(snapshot.session_generation, stream.actor_session_id());
    assert_eq!(snapshot.player_column, keys[0].chunk());
    assert_eq!(snapshot.publisher_radius_blocks, Some(16));
    assert_eq!(snapshot.publisher_radius_chunks, Some(1));
    assert_eq!(snapshot.required_columns, 5);
    assert_eq!(snapshot.loaded_required_columns, 1);
    assert_eq!(snapshot.outcomes.success, 1);
    assert_eq!(snapshot.outcomes.all_air, 1);
    assert_eq!(snapshot.outcomes.unavailable, 1);
    assert_eq!(snapshot.outcomes.malformed, 1);
    assert_eq!(snapshot.outcomes.stale, 1);
    assert_eq!(snapshot.outcomes.timed_out, 1);
    assert!(snapshot.stages.requests_sent <= snapshot.stages.requests_constructed);
    assert_eq!(snapshot.stages.subchunks_awaiting_response, 0);

    let other_dimension =
        stream.phase2_publication_snapshot(ChunkKey::new(1, keys[0].x, keys[0].z));
    assert_eq!(other_dimension.required_columns, 0);
    assert_ne!(
        other_dimension.required_cohort_hash,
        snapshot.required_cohort_hash
    );
}

#[test]
fn request_modes_use_vanilla_dimension_base_and_bounded_counts() {
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
    assert_eq!(overworld_requests[0].dimension, 0);
    assert_eq!(overworld_requests[0].chunk, ChunkKey::new(0, -2, 5));
    assert_eq!(overworld_requests[0].base_sub_chunk_y, -4);
    assert_eq!(overworld_requests[0].count, 24);
    stream
        .submit(
            2,
            WorldEvent::ChangeDimension(ChangeDimensionEvent {
                dimension: 1,
                position: [0.0, 80.0, 0.0],
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
    assert_eq!(requests[0].dimension, 1);
    assert_eq!(requests[0].base_sub_chunk_y, 0);
    assert_eq!(requests[0].count, 8);
}

#[test]
fn outbound_request_fifo_has_a_hard_admission_capacity() {
    let mut stream = WorldStream::new(WorldBootstrap {
        dimension: 0,
        local_player_runtime_id: 1,
        player_position: [0.0; 3],
        world_spawn_position: [0; 3],
        air_network_id: 12_530,
        block_network_ids_are_hashes: false,
    });
    for index in 0..super::OUTBOUND_REQUEST_CAPACITY {
        let x = index as i32 % 17 - 8;
        let z = index as i32 / 17 - 8;
        stream
            .submit(
                index as u64 + 1,
                WorldEvent::LevelChunk(LevelChunkEvent {
                    dimension: 0,
                    x,
                    z,
                    mode: LevelChunkMode::LimitedRequests { highest: 1 },
                    payload: biome_payload(0, 1),
                }),
            )
            .unwrap();
        if (index + 1) % super::MAX_ADMITTED_HEAVY_EVENTS == 0 {
            complete_pending_decode_jobs(&mut stream);
        }
    }
    assert_eq!(
        stream.pending_request_count(),
        super::OUTBOUND_REQUEST_CAPACITY
    );

    assert!(matches!(
        stream
            .submit(
                super::OUTBOUND_REQUEST_CAPACITY as u64 + 1,
                WorldEvent::LevelChunk(LevelChunkEvent {
                    dimension: 0,
                    x: 9,
                    z: 9,
                    mode: LevelChunkMode::LimitlessRequests,
                    payload: biome_payload(0, 1),
                }),
            )
            .unwrap_err(),
        super::WorldStreamError::OutboundFull { .. }
    ));

    let empty = ChunkKey::new(0, 9, 10);
    stream
        .submit(
            super::OUTBOUND_REQUEST_CAPACITY as u64 + 1,
            request_level_chunk_event(
                empty.dimension,
                empty.x,
                empty.z,
                LevelChunkMode::LimitedRequests { highest: 0 },
                1,
            ),
        )
        .expect("an authoritative empty column needs no outbound FIFO slot");
    complete_pending_decode_jobs(&mut stream);
    assert_eq!(
        stream.pending_request_count(),
        super::OUTBOUND_REQUEST_CAPACITY
    );
    assert!(stream.loaded_columns.contains(&empty));
    assert!(stream.store.is_chunk_loaded(empty));
}

#[test]
fn request_mode_non_air_completion_marks_collision_residency() {
    let (mut stream, key) = stream_with_one_expected_sub_chunk();
    let decoded = world::DecodedSubChunk::decode(
        key,
        include_bytes!(concat!(
            env!("CARGO_MANIFEST_DIR"),
            "/../world/fixtures/uniform_non_air.bin"
        )),
    )
    .unwrap();

    apply_sub_chunk_result(
        &mut stream,
        key,
        super::PreparedSubChunkResult::Decoded(Ok(decoded)),
    );

    assert!(stream.loaded_columns.contains(&key.chunk()));
    assert!(stream.store.is_chunk_loaded(key.chunk()));
    assert!(stream.store.sub_chunk(key).is_some());
}

#[test]
fn request_mode_all_air_completion_marks_sparse_collision_residency() {
    let (mut stream, key) = stream_with_one_expected_sub_chunk();

    apply_sub_chunk_result(&mut stream, key, super::PreparedSubChunkResult::AllAir);

    assert!(stream.loaded_columns.contains(&key.chunk()));
    assert!(stream.store.is_chunk_loaded(key.chunk()));
    assert!(stream.store.sub_chunk(key).is_none());
    stream.evict_column(key.chunk());
    assert!(!stream.store.is_chunk_loaded(key.chunk()));
}

#[test]
fn request_mode_collision_failure_latch_spans_column_and_resets_on_eviction() {
    let (mut stream, keys, _) = stream_with_unsent_sub_chunks(2);
    let chunk = keys[0].chunk();

    apply_sub_chunk_result(
        &mut stream,
        keys[0],
        super::PreparedSubChunkResult::Unavailable(SubChunkUnavailable::InvalidDimension),
    );
    assert!(stream.request_collision_failures.contains(&chunk));
    apply_sub_chunk_result(&mut stream, keys[1], super::PreparedSubChunkResult::AllAir);
    assert!(stream.loaded_columns.contains(&chunk));
    assert!(!stream.store.is_chunk_loaded(chunk));

    stream.evict_column(chunk);
    stream
        .submit(
            2,
            WorldEvent::LevelChunk(LevelChunkEvent {
                dimension: chunk.dimension,
                x: chunk.x,
                z: chunk.z,
                mode: LevelChunkMode::LimitedRequests { highest: 2 },
                payload: biome_payload(0, 1),
            }),
        )
        .unwrap();
    complete_pending_decode_jobs(&mut stream);
    assert_eq!(stream.take_requests().len(), 1);
    apply_sub_chunk_result(
        &mut stream,
        keys[0],
        super::PreparedSubChunkResult::Unavailable(SubChunkUnavailable::InvalidDimension),
    );
    assert!(stream.request_collision_failures.contains(&chunk));
    stream.evict_column(chunk);
    assert!(!stream.request_collision_failures.contains(&chunk));

    stream
        .submit(
            3,
            WorldEvent::LevelChunk(LevelChunkEvent {
                dimension: chunk.dimension,
                x: chunk.x,
                z: chunk.z,
                mode: LevelChunkMode::LimitedRequests { highest: 2 },
                payload: biome_payload(0, 1),
            }),
        )
        .unwrap();
    complete_pending_decode_jobs(&mut stream);
    assert_eq!(stream.take_requests().len(), 1);
    for key in keys {
        apply_sub_chunk_result(&mut stream, key, super::PreparedSubChunkResult::AllAir);
    }
    assert!(stream.store.is_chunk_loaded(chunk));
}

#[test]
fn omitted_sub_chunk_y_retries_at_deadline_then_completes_after_bound() {
    let started = Instant::now();
    let (mut stream, keys, initial) = stream_with_unsent_sub_chunks(1);
    let key = keys[0];
    acknowledge_request_sent(&mut stream, &initial, started);

    for attempt in 1..=super::MAX_SUB_CHUNK_RETRIES {
        let deadline = started + super::SUB_CHUNK_RESPONSE_TIMEOUT * u32::from(attempt);
        stream.expire_sub_chunk_deadlines(deadline);
        let retry = stream
            .pop_next_request()
            .expect("an omitted Y should queue the exact bounded retry");
        assert_eq!(retry.chunk, key.chunk());
        assert_eq!(retry.base_sub_chunk_y, key.y);
        assert_eq!(retry.count, 1);
        acknowledge_request_sent(&mut stream, &retry, deadline);
    }

    let terminal_deadline = started
        + super::SUB_CHUNK_RESPONSE_TIMEOUT
            * u32::from(super::MAX_SUB_CHUNK_RETRIES.saturating_add(1));
    stream.expire_sub_chunk_deadlines(terminal_deadline);

    assert!(stream.loaded_columns.contains(&key.chunk()));
    assert!(!stream.requested_sub_chunks.contains_key(&key.chunk()));
    assert!(!stream.resident.contains(&key));
    assert!(!stream.known_air.contains(&key));
    assert!(stream.sub_chunk_deadlines.is_empty());
    assert_eq!(stream.pending_request_count(), 0);
    let stats = stream.stats();
    assert_eq!(stats.awaiting_sub_chunk_responses, 0);
    assert_eq!(stats.sub_chunk_timeouts, 3);
    assert_eq!(stats.sub_chunk_retries_scheduled, 2);
    assert_eq!(stats.sub_chunk_retry_exhaustions, 1);

    let errors_before = stream.stats().normalization_errors;
    apply_sub_chunk_result(&mut stream, key, super::PreparedSubChunkResult::AllAir);
    assert_eq!(stream.stats().normalization_errors, errors_before + 1);
    assert!(!stream.resident.contains(&key));
    assert!(!stream.known_air.contains(&key));
}

#[test]
fn response_deadline_begins_only_after_successful_send() {
    let started = Instant::now();
    let (mut stream, keys, initial) = stream_with_unsent_sub_chunks(1);
    let key = keys[0];
    assert!(
        stream.retry_request_front(initial).is_ok(),
        "a failed send must restore its unsent request"
    );

    stream.expire_sub_chunk_deadlines(started + Duration::from_secs(100));
    assert_eq!(stream.stats().awaiting_sub_chunk_responses, 0);
    assert_eq!(stream.stats().sub_chunk_timeouts, 0);
    assert!(stream.requested_sub_chunks.contains_key(&key.chunk()));

    let retry = stream.pop_next_request().unwrap();
    let sent_at = started + Duration::from_secs(100);
    acknowledge_request_sent(&mut stream, &retry, sent_at);
    assert_eq!(stream.stats().awaiting_sub_chunk_responses, 1);
    stream.expire_sub_chunk_deadlines(
        sent_at + super::SUB_CHUNK_RESPONSE_TIMEOUT - Duration::from_nanos(1),
    );
    assert_eq!(stream.stats().sub_chunk_timeouts, 0);
    stream.expire_sub_chunk_deadlines(sent_at + super::SUB_CHUNK_RESPONSE_TIMEOUT);
    assert_eq!(stream.stats().sub_chunk_timeouts, 1);
}

#[test]
fn reply_from_already_sent_retry_is_not_unexpected_after_first_attempt_completes() {
    let started = Instant::now();
    let (mut stream, keys, initial) = stream_with_unsent_sub_chunks(1);
    let key = keys[0];
    acknowledge_request_sent(&mut stream, &initial, started);

    let retry_sent_at = started + super::SUB_CHUNK_RESPONSE_TIMEOUT;
    stream.expire_sub_chunk_deadlines(retry_sent_at);
    let retry = stream
        .pop_next_request()
        .expect("the expired initial attempt should queue an exact retry");
    acknowledge_request_sent(&mut stream, &retry, retry_sent_at);

    stream
        .submit(
            2,
            WorldEvent::SubChunks(SubChunkBatchEvent {
                dimension: key.dimension,
                entries: vec![SubChunkEntryEvent {
                    position: [key.x, key.y, key.z],
                    result: SubChunkResult::AllAir,
                }],
            }),
        )
        .unwrap();
    complete_pending_decode_jobs(&mut stream);
    let unexpected_before = stream.stats().normalization_reasons.unexpected_sub_chunks;
    stream
        .submit(
            3,
            WorldEvent::SubChunks(SubChunkBatchEvent {
                dimension: key.dimension,
                entries: vec![SubChunkEntryEvent {
                    position: [key.x, key.y, key.z],
                    result: SubChunkResult::AllAir,
                }],
            }),
        )
        .unwrap();
    complete_pending_decode_jobs(&mut stream);

    assert_eq!(
        stream.stats().normalization_reasons.unexpected_sub_chunks,
        unexpected_before
    );
}

#[test]
fn timely_sub_chunk_admission_disarms_and_cancels_before_decode_or_expiry() {
    let started = Instant::now();
    let (mut stream, keys, initial) = stream_with_unsent_sub_chunks(2);
    acknowledge_request_sent(&mut stream, &initial, started);

    let first_deadline = started + super::SUB_CHUNK_RESPONSE_TIMEOUT;
    stream.expire_sub_chunk_deadlines(first_deadline);
    let sent_retry = stream
        .pop_next_request()
        .expect("the first exact retry should retain FIFO order");
    acknowledge_request_sent(&mut stream, &sent_retry, first_deadline);
    assert_eq!(stream.pending_request_count(), 1);
    assert_eq!(stream.sub_chunk_deadlines.len(), 1);

    stream
        .submit(
            2,
            WorldEvent::SubChunks(SubChunkBatchEvent {
                dimension: 0,
                entries: keys
                    .iter()
                    .map(|key| SubChunkEntryEvent {
                        position: [key.x, key.y, key.z],
                        result: SubChunkResult::AllAir,
                    })
                    .collect(),
            }),
        )
        .unwrap();

    assert_eq!(stream.pending_decode.len(), 1);
    assert!(stream.sub_chunk_deadlines.is_empty());
    assert_eq!(stream.pending_request_count(), 0);
    let retry_deadline = first_deadline + super::SUB_CHUNK_RESPONSE_TIMEOUT;
    stream.expire_sub_chunk_deadlines(retry_deadline);
    assert_eq!(stream.stats().sub_chunk_timeouts, 2);
    assert_eq!(stream.outstanding_sub_chunk_count(), 2);

    stream.dispatch_decode_jobs();
    assert!(stream.pending_decode.is_empty());
    assert_eq!(stream.in_flight_decode_jobs, 1);
    stream.expire_sub_chunk_deadlines(retry_deadline);
    assert_eq!(stream.stats().sub_chunk_timeouts, 2);
    assert_eq!(stream.outstanding_sub_chunk_count(), 2);
}

#[test]
fn transport_ack_after_reply_admission_cannot_rearm_expiry_during_decode() {
    let acknowledged_at = Instant::now();
    let (mut stream, keys, initial) = stream_with_unsent_sub_chunks(1);
    let key = keys[0];
    stream.record_sub_chunk_request_transport_pending(
        initial.chunk,
        initial.base_sub_chunk_y,
        initial.count,
    );
    stream
        .submit(
            2,
            WorldEvent::SubChunks(SubChunkBatchEvent {
                dimension: key.dimension,
                entries: vec![SubChunkEntryEvent {
                    position: [key.x, key.y, key.z],
                    result: SubChunkResult::AllAir,
                }],
            }),
        )
        .unwrap();
    assert_eq!(stream.pending_decode.len(), 1);
    assert!(stream.sub_chunk_deadlines.is_empty());

    stream.acknowledge_sub_chunk_request_sent(
        initial.chunk,
        initial.base_sub_chunk_y,
        initial.count,
        acknowledged_at,
    );

    assert!(stream.sub_chunk_deadlines.is_empty());
    stream.expire_sub_chunk_deadlines(acknowledged_at + super::SUB_CHUNK_RESPONSE_TIMEOUT);
    assert_eq!(stream.stats().sub_chunk_timeouts, 0);
    assert_eq!(stream.outstanding_sub_chunk_count(), 1);
}

#[test]
fn explicit_transient_reply_disarms_old_deadline_and_preserves_retry_bound() {
    let started = Instant::now();
    let (mut stream, keys, initial) = stream_with_unsent_sub_chunks(1);
    let key = keys[0];
    acknowledge_request_sent(&mut stream, &initial, started);

    apply_sub_chunk_result(
        &mut stream,
        key,
        super::PreparedSubChunkResult::Unavailable(SubChunkUnavailable::ChunkNotFound),
    );
    assert!(stream.sub_chunk_deadlines.is_empty());
    assert_eq!(stream.stats().awaiting_sub_chunk_responses, 0);
    assert_eq!(stream.stats().sub_chunk_retries_scheduled, 1);
    stream.expire_sub_chunk_deadlines(started + super::SUB_CHUNK_RESPONSE_TIMEOUT);
    assert_eq!(stream.stats().sub_chunk_timeouts, 0);

    let first_retry = stream.pop_next_request().unwrap();
    let first_retry_sent_at = started + Duration::from_secs(1);
    acknowledge_request_sent(&mut stream, &first_retry, first_retry_sent_at);
    stream.expire_sub_chunk_deadlines(first_retry_sent_at + super::SUB_CHUNK_RESPONSE_TIMEOUT);
    let second_retry = stream.pop_next_request().unwrap();
    let second_retry_sent_at = first_retry_sent_at + super::SUB_CHUNK_RESPONSE_TIMEOUT;
    acknowledge_request_sent(&mut stream, &second_retry, second_retry_sent_at);
    stream.expire_sub_chunk_deadlines(second_retry_sent_at + super::SUB_CHUNK_RESPONSE_TIMEOUT);

    assert!(stream.loaded_columns.contains(&key.chunk()));
    assert!(!stream.known_air.contains(&key));
    let stats = stream.stats();
    assert_eq!(stats.sub_chunk_timeouts, 2);
    assert_eq!(stats.sub_chunk_retries_scheduled, 2);
    assert_eq!(stats.sub_chunk_retry_exhaustions, 1);
}

#[test]
fn explicit_transient_retry_preserves_older_deferred_fifo_when_outbound_reopens() {
    let started = Instant::now();
    let (mut stream, keys, initial) = stream_with_unsent_sub_chunks(2);
    acknowledge_request_sent(&mut stream, &initial, started);
    for sequence in 0..super::OUTBOUND_REQUEST_CAPACITY {
        stream
            .requests
            .push_back(super::OutboundRequestSlot::Reserved(sequence as u64 + 10));
    }
    apply_sub_chunk_result(
        &mut stream,
        keys[0],
        super::PreparedSubChunkResult::Unavailable(SubChunkUnavailable::ChunkNotFound),
    );
    assert_eq!(stream.deferred_retries.front(), Some(&keys[0]));
    for index in 1..super::DEFERRED_RETRY_CAPACITY {
        let key = SubChunkKey::new(0, 100 + index as i32, -4, 100);
        stream.deferred_retries.push_back(key);
        stream.deferred_retry_set.insert(key);
    }
    stream.requests.pop_front();
    let normalization_before = stream.stats().normalization_errors;

    apply_sub_chunk_result(
        &mut stream,
        keys[1],
        super::PreparedSubChunkResult::Unavailable(SubChunkUnavailable::PlayerNotFound),
    );

    let outbound_retry_y = stream.requests.iter().find_map(|slot| match slot {
        super::OutboundRequestSlot::Ready(request) => Some(request.base_sub_chunk_y),
        super::OutboundRequestSlot::Reserved(_) => None,
    });
    assert_eq!(outbound_retry_y, Some(keys[0].y));
    assert_eq!(stream.deferred_retries.back(), Some(&keys[1]));
    assert_eq!(
        stream.deferred_retries.len(),
        super::DEFERRED_RETRY_CAPACITY
    );
    assert_eq!(stream.outstanding_sub_chunk_count(), 2);
    assert_eq!(stream.stats().sub_chunk_retries_scheduled, 2);
    assert_eq!(stream.stats().normalization_errors, normalization_before);
    assert!(!stream.loaded_columns.contains(&keys[0].chunk()));
}

#[test]
fn late_success_cancels_queued_timeout_retry() {
    let started = Instant::now();
    let (mut stream, keys, initial) = stream_with_unsent_sub_chunks(2);
    acknowledge_request_sent(&mut stream, &initial, started);
    for sequence in 0..super::OUTBOUND_REQUEST_CAPACITY - 1 {
        stream
            .requests
            .push_back(super::OutboundRequestSlot::Reserved(sequence as u64 + 10));
    }
    stream.expire_sub_chunk_deadlines(started + super::SUB_CHUNK_RESPONSE_TIMEOUT);
    assert_eq!(stream.pending_request_count(), 1);
    assert_eq!(stream.deferred_retries.len(), 1);

    for key in &keys {
        apply_sub_chunk_result(&mut stream, *key, super::PreparedSubChunkResult::AllAir);
    }

    assert!(stream.loaded_columns.contains(&keys[0].chunk()));
    assert!(keys.iter().all(|key| stream.known_air.contains(key)));
    assert_eq!(stream.pending_request_count(), 0);
    assert!(stream.deferred_retries.is_empty());
    assert!(stream.sub_chunk_deadlines.is_empty());
    let stats = stream.stats();
    assert_eq!(stats.sub_chunk_timeouts, 2);
    assert_eq!(stats.sub_chunk_retries_scheduled, 2);
    assert_eq!(stats.sub_chunk_retry_exhaustions, 0);
}

#[test]
fn eviction_purges_deadlines_retries_and_late_reply_state() {
    let started = Instant::now();
    let (mut stream, keys, initial) = stream_with_unsent_sub_chunks(3);
    acknowledge_request_sent(&mut stream, &initial, started);
    for sequence in 0..super::OUTBOUND_REQUEST_CAPACITY - 2 {
        stream
            .requests
            .push_back(super::OutboundRequestSlot::Reserved(sequence as u64 + 10));
    }
    stream.expire_sub_chunk_deadlines(started + super::SUB_CHUNK_RESPONSE_TIMEOUT);
    assert_eq!(stream.pending_request_count(), 2);
    assert_eq!(stream.deferred_retries.len(), 1);
    stream
        .requests
        .retain(|slot| matches!(slot, super::OutboundRequestSlot::Ready(_)));
    let armed_retry = stream.pop_next_request().unwrap();
    acknowledge_request_sent(
        &mut stream,
        &armed_retry,
        started + super::SUB_CHUNK_RESPONSE_TIMEOUT,
    );
    assert!(!stream.sub_chunk_deadlines.is_empty());
    assert_eq!(stream.pending_request_count(), 1);
    assert_eq!(stream.deferred_retries.len(), 1);

    let chunk = keys[0].chunk();
    stream.evict_column(chunk);

    assert!(!stream.requested_sub_chunks.contains_key(&chunk));
    assert!(stream.sub_chunk_deadlines.is_empty());
    assert_eq!(stream.pending_request_count(), 0);
    assert!(stream.deferred_retries.is_empty());
    assert!(stream.deferred_retry_set.is_empty());
    assert_eq!(stream.stats().awaiting_sub_chunk_responses, 0);

    let errors_before = stream.stats().normalization_errors;
    apply_sub_chunk_result(&mut stream, keys[0], super::PreparedSubChunkResult::AllAir);
    assert_eq!(stream.stats().normalization_errors, errors_before + 1);
    assert!(!stream.loaded_columns.contains(&chunk));
    assert!(!stream.resident.contains(&keys[0]));
    assert!(!stream.known_air.contains(&keys[0]));
}

#[test]
fn expired_deadlines_obey_capacity_without_loss_or_overflow() {
    let started = Instant::now();
    let (mut stream, keys, initial) = stream_with_unsent_sub_chunks(3);
    acknowledge_request_sent(&mut stream, &initial, started);
    for sequence in 0..super::OUTBOUND_REQUEST_CAPACITY {
        stream
            .requests
            .push_back(super::OutboundRequestSlot::Reserved(sequence as u64 + 10));
    }
    apply_sub_chunk_result(
        &mut stream,
        keys[0],
        super::PreparedSubChunkResult::Unavailable(SubChunkUnavailable::ChunkNotFound),
    );
    assert_eq!(stream.deferred_retries.front(), Some(&keys[0]));
    for index in 1..super::DEFERRED_RETRY_CAPACITY {
        let key = SubChunkKey::new(0, 100 + index as i32, -4, 100);
        stream.deferred_retries.push_back(key);
        stream.deferred_retry_set.insert(key);
    }
    let normalization_before = stream.stats().normalization_errors;
    let deadline = started + super::SUB_CHUNK_RESPONSE_TIMEOUT;

    stream.expire_sub_chunk_deadlines(deadline);
    assert_eq!(stream.sub_chunk_deadlines.len(), 2);
    assert_eq!(stream.stats().sub_chunk_timeouts, 0);
    assert_eq!(stream.stats().sub_chunk_retries_scheduled, 1);
    assert_eq!(stream.stats().normalization_errors, normalization_before);

    stream.requests.pop_front();
    stream.expire_sub_chunk_deadlines(deadline);

    assert_eq!(stream.requests.len(), super::OUTBOUND_REQUEST_CAPACITY);
    let outbound_retry_y = stream.requests.iter().find_map(|slot| match slot {
        super::OutboundRequestSlot::Ready(request) => Some(request.base_sub_chunk_y),
        super::OutboundRequestSlot::Reserved(_) => None,
    });
    assert_eq!(outbound_retry_y, Some(keys[0].y));
    assert_eq!(
        stream.deferred_retries.len(),
        super::DEFERRED_RETRY_CAPACITY
    );
    assert_eq!(stream.deferred_retries.back(), Some(&keys[1]));
    assert_eq!(stream.sub_chunk_deadlines.len(), 1);
    assert!(stream.sub_chunk_deadlines.contains(&(deadline, keys[2])));
    assert_eq!(stream.outstanding_sub_chunk_count(), 3);
    assert_eq!(stream.stats().sub_chunk_timeouts, 1);
    assert_eq!(stream.stats().sub_chunk_retries_scheduled, 2);
    assert_eq!(stream.stats().normalization_errors, normalization_before);
}

#[test]
fn timeout_progress_stats_are_exact_and_deterministic() {
    let started = Instant::now();
    let (mut stream, keys, initial) = stream_with_unsent_sub_chunks(2);
    acknowledge_request_sent(&mut stream, &initial, started);
    assert_eq!(stream.stats().awaiting_sub_chunk_responses, 2);

    let first_deadline = started + super::SUB_CHUNK_RESPONSE_TIMEOUT;
    stream.expire_sub_chunk_deadlines(first_deadline);
    let stats = stream.stats();
    assert_eq!(stats.awaiting_sub_chunk_responses, 0);
    assert_eq!(stats.sub_chunk_timeouts, 2);
    assert_eq!(stats.sub_chunk_retries_scheduled, 2);
    assert_eq!(stats.sub_chunk_retry_exhaustions, 0);

    let retries = [
        stream.pop_next_request().unwrap(),
        stream.pop_next_request().unwrap(),
    ];
    for retry in &retries {
        acknowledge_request_sent(&mut stream, retry, first_deadline);
    }
    assert_eq!(stream.stats().awaiting_sub_chunk_responses, 2);
    apply_sub_chunk_result(&mut stream, keys[0], super::PreparedSubChunkResult::AllAir);
    assert_eq!(stream.stats().awaiting_sub_chunk_responses, 1);

    let second_deadline = first_deadline + super::SUB_CHUNK_RESPONSE_TIMEOUT;
    stream.expire_sub_chunk_deadlines(second_deadline);
    let final_retry = stream.pop_next_request().unwrap();
    acknowledge_request_sent(&mut stream, &final_retry, second_deadline);
    let third_deadline = second_deadline + super::SUB_CHUNK_RESPONSE_TIMEOUT;
    stream.expire_sub_chunk_deadlines(third_deadline);

    let stats = stream.stats();
    assert_eq!(stats.awaiting_sub_chunk_responses, 0);
    assert_eq!(stats.sub_chunk_timeouts, 4);
    assert_eq!(stats.sub_chunk_retries_scheduled, 3);
    assert_eq!(stats.sub_chunk_retry_exhaustions, 1);
    assert_eq!(stream.outstanding_sub_chunk_count(), 0);
}

#[test]
fn unavailable_value_is_preserved_and_y_out_of_bounds_completes_split_batch_as_air() {
    let prepared = super::prepare_sub_chunks(SubChunkBatchEvent {
        dimension: 0,
        entries: vec![SubChunkEntryEvent {
            position: [0, -4, 0],
            result: SubChunkResult::Unavailable(SubChunkUnavailable::ChunkNotFound),
        }],
    });
    assert!(matches!(
        prepared[0].result,
        super::PreparedSubChunkResult::Unavailable(SubChunkUnavailable::ChunkNotFound)
    ));

    let mut stream = WorldStream::new(WorldBootstrap {
        dimension: 0,
        local_player_runtime_id: 1,
        player_position: [0.0; 3],
        world_spawn_position: [0; 3],
        air_network_id: 12_530,
        block_network_ids_are_hashes: false,
    });
    let chunk = ChunkKey::new(0, 0, 0);
    stream.requested_sub_chunks.insert(
        chunk,
        BTreeMap::from([(-4, Default::default()), (-3, Default::default())]),
    );
    apply_sub_chunk_result(
        &mut stream,
        SubChunkKey::from_chunk(chunk, -4),
        super::PreparedSubChunkResult::AllAir,
    );
    apply_sub_chunk_result(
        &mut stream,
        SubChunkKey::from_chunk(chunk, -3),
        super::PreparedSubChunkResult::Unavailable(SubChunkUnavailable::YIndexOutOfBounds),
    );

    assert!(!stream.requested_sub_chunks.contains_key(&chunk));
    assert!(stream.loaded_columns.contains(&chunk));
    assert!(
        stream
            .known_air
            .contains(&SubChunkKey::from_chunk(chunk, -3))
    );
}

#[test]
fn transient_unavailable_results_retry_boundedly_then_complete_without_wedging() {
    for unavailable in [
        SubChunkUnavailable::ChunkNotFound,
        SubChunkUnavailable::PlayerNotFound,
    ] {
        let (mut stream, key) = stream_with_one_expected_sub_chunk();
        for attempt in 0..=super::MAX_SUB_CHUNK_RETRIES {
            apply_sub_chunk_result(
                &mut stream,
                key,
                super::PreparedSubChunkResult::Unavailable(unavailable),
            );
            if attempt < super::MAX_SUB_CHUNK_RETRIES {
                assert_eq!(stream.pending_request_count(), 1);
                assert!(stream.requested_sub_chunks.contains_key(&key.chunk()));
                stream.take_requests();
            }
        }
        assert!(!stream.requested_sub_chunks.contains_key(&key.chunk()));
        assert!(stream.loaded_columns.contains(&key.chunk()));
        assert!(!stream.store.is_chunk_loaded(key.chunk()));
        assert_eq!(stream.pending_request_count(), 0);
    }
}

#[test]
fn decode_failures_retry_boundedly_and_invalid_dimension_is_terminal_normalization() {
    let (mut stream, key) = stream_with_one_expected_sub_chunk();
    for attempt in 0..=super::MAX_SUB_CHUNK_RETRIES {
        apply_sub_chunk_result(
            &mut stream,
            key,
            super::PreparedSubChunkResult::Decoded(Err(world::DecodeError::UnsupportedVersion(
                255,
            ))),
        );
        if attempt < super::MAX_SUB_CHUNK_RETRIES {
            assert_eq!(stream.take_requests().len(), 1);
        }
    }
    assert!(stream.loaded_columns.contains(&key.chunk()));
    assert!(!stream.store.is_chunk_loaded(key.chunk()));
    assert!(!stream.requested_sub_chunks.contains_key(&key.chunk()));

    let (mut stream, key) = stream_with_one_expected_sub_chunk();
    apply_sub_chunk_result(
        &mut stream,
        key,
        super::PreparedSubChunkResult::Unavailable(SubChunkUnavailable::InvalidDimension),
    );
    assert_eq!(stream.stats().normalization_errors, 1);
    assert!(stream.loaded_columns.contains(&key.chunk()));
    assert!(!stream.store.is_chunk_loaded(key.chunk()));
    assert!(!stream.requested_sub_chunks.contains_key(&key.chunk()));
}

#[test]
fn request_mode_evicts_the_old_column_and_invalidates_its_neighbours() {
    let mut stream = WorldStream::new(WorldBootstrap {
        dimension: 0,
        local_player_runtime_id: 1,
        player_position: [0.0; 3],
        world_spawn_position: [0; 3],
        air_network_id: 12_530,
        block_network_ids_are_hashes: false,
    });
    let key = SubChunkKey::new(0, 3, -4, -2);
    let decoded = DecodedLevelChunk::decode(
        -4,
        1,
        include_bytes!(concat!(
            env!("CARGO_MANIFEST_DIR"),
            "/../world/fixtures/uniform_non_air.bin"
        )),
    )
    .unwrap();
    stream
        .store
        .commit_level_chunk(key.chunk(), decoded)
        .unwrap();
    stream.resident.insert(key);
    stream.mark_changed(key, Instant::now());
    assert_eq!(stream.dispatch_light_jobs([0.0; 3], 1), 1);
    let completion = stream
        .light_rx
        .recv_timeout(Duration::from_secs(2))
        .expect("eviction setup light completion");
    stream.accept_light_completion(completion);
    assert!(stream.light_is_current(key));
    stream.pending_mesh.clear();
    stream.revisions.entries.clear();

    stream
        .submit(
            1,
            WorldEvent::LevelChunk(LevelChunkEvent {
                dimension: 0,
                x: key.x,
                z: key.z,
                mode: LevelChunkMode::LimitedRequests { highest: 1 },
                payload: biome_payload(0, 1),
            }),
        )
        .unwrap();
    complete_pending_decode_jobs(&mut stream);

    assert!(stream.store.sub_chunk(key).is_none());
    assert!(!stream.resident.contains(&key));
    assert_eq!(stream.take_requests().len(), 1);
    let actual = stream
        .pending_mesh
        .keys()
        .copied()
        .collect::<std::collections::BTreeSet<_>>();
    let evicted_dependents = key
        .mesh_neighbourhood_dependents()
        .collect::<std::collections::BTreeSet<_>>();
    assert!(evicted_dependents.is_subset(&actual));
    for y in -3..=19 {
        let air = SubChunkKey::new(0, key.x, y, key.z);
        assert!(stream.known_air.contains(&air));
        assert!(stream.pending_light.contains_key(&air));
    }
}

#[test]
fn changed_sub_chunk_dirties_center_and_six_face_neighbours_once() {
    let mut stream = WorldStream::new(WorldBootstrap {
        dimension: 0,
        local_player_runtime_id: 1,
        player_position: [0.0; 3],
        world_spawn_position: [0; 3],
        air_network_id: 12_530,
        block_network_ids_are_hashes: false,
    });
    let key = SubChunkKey::new(0, 4, -2, 9);

    stream.mark_changed(key, Instant::now());
    let expected = key
        .mesh_dependents()
        .collect::<std::collections::BTreeSet<_>>();
    let actual = stream
        .pending_mesh
        .keys()
        .copied()
        .collect::<std::collections::BTreeSet<_>>();

    assert_eq!(actual, expected);
    assert_eq!(stream.pending_mesh.len(), 7);
}
