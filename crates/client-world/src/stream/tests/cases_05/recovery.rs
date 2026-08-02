use super::*;

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
fn cached_subchunk_admission_cancels_deadline_without_completing_expected_y() {
    let started = Instant::now();
    let (mut stream, keys, initial) = stream_with_unsent_sub_chunks(1);
    let key = keys[0];
    acknowledge_request_sent(&mut stream, &initial, started);

    stream
        .submit(
            2,
            WorldEvent::SubChunkReplyAdmission(SubChunkReplyAdmissionEvent {
                dimension: key.dimension,
                positions: vec![[key.x, key.y, key.z]],
            }),
        )
        .unwrap();

    assert!(stream.sub_chunk_deadlines.is_empty());
    assert!(
        stream
            .requested_sub_chunks
            .get(&key.chunk())
            .is_some_and(|column| column.contains_key(&key.y)),
        "admission must not complete or remove the expected Y"
    );
    assert_eq!(stream.outstanding_sub_chunk_count(), 1);
}

#[test]
fn abandoned_cached_subchunk_rolls_back_admission_and_queues_retry() {
    let started = Instant::now();
    let (mut stream, keys, initial) = stream_with_unsent_sub_chunks(1);
    let key = keys[0];
    acknowledge_request_sent(&mut stream, &initial, started);
    stream
        .submit(
            2,
            WorldEvent::SubChunkReplyAdmission(SubChunkReplyAdmissionEvent {
                dimension: key.dimension,
                positions: vec![[key.x, key.y, key.z]],
            }),
        )
        .unwrap();
    stream.admitted_sub_chunk_replies.insert(key, 2);

    stream
        .submit(
            3,
            WorldEvent::ChunkResync(ChunkResyncEvent {
                dimension: key.dimension,
                x: key.x,
                z: key.z,
                requested_sub_chunks: None,
                requested_sub_chunk_ys: Some(vec![key.y]),
            }),
        )
        .unwrap();
    assert!(!stream.admitted_sub_chunk_replies.contains_key(&key));
    let retry = stream
        .pop_next_request()
        .expect("abandonment must restore an exact retry");
    assert_eq!(retry.chunk, key.chunk());
    assert_eq!(retry.base_sub_chunk_y, key.y);
    assert_eq!(retry.count, 1);
}

#[test]
fn recovery_without_admission_preserves_existing_request_deadline() {
    let started = Instant::now();
    let (mut stream, keys, initial) = stream_with_unsent_sub_chunks(1);
    let key = keys[0];
    acknowledge_request_sent(&mut stream, &initial, started);

    stream
        .submit(
            2,
            WorldEvent::ChunkResync(ChunkResyncEvent {
                dimension: key.dimension,
                x: key.x,
                z: key.z,
                requested_sub_chunks: None,
                requested_sub_chunk_ys: Some(vec![key.y]),
            }),
        )
        .unwrap();

    assert!(stream.pop_next_request().is_none());
    assert_eq!(stream.sub_chunk_deadlines.len(), 1);
}

#[test]
fn recovery_clears_stale_admission_after_another_reply_completed_the_y() {
    let started = Instant::now();
    let (mut stream, keys, initial) = stream_with_unsent_sub_chunks(1);
    let key = keys[0];
    acknowledge_request_sent(&mut stream, &initial, started);
    stream
        .submit(
            2,
            WorldEvent::SubChunkReplyAdmission(SubChunkReplyAdmissionEvent {
                dimension: key.dimension,
                positions: vec![[key.x, key.y, key.z]],
            }),
        )
        .unwrap();
    stream.admitted_sub_chunk_replies.insert(key, 2);
    apply_sub_chunk_result(&mut stream, key, super::PreparedSubChunkResult::AllAir);
    assert!(!stream.is_expected_sub_chunk(key));
    assert!(stream.admitted_sub_chunk_replies.contains_key(&key));

    stream
        .submit(
            3,
            WorldEvent::ChunkResync(ChunkResyncEvent {
                dimension: key.dimension,
                x: key.x,
                z: key.z,
                requested_sub_chunks: None,
                requested_sub_chunk_ys: Some(vec![key.y]),
            }),
        )
        .unwrap();

    assert!(!stream.admitted_sub_chunk_replies.contains_key(&key));
    let retry = stream
        .pop_next_request()
        .expect("recovery must request the completed Y again");
    assert_eq!((retry.base_sub_chunk_y, retry.count), (key.y, 1));
}

#[test]
fn reconstructed_subchunk_after_original_deadline_commits_without_stale_classification() {
    let started = Instant::now();
    let (mut stream, keys, initial) = stream_with_unsent_sub_chunks(1);
    let key = keys[0];
    acknowledge_request_sent(&mut stream, &initial, started);

    stream
        .submit(
            2,
            WorldEvent::SubChunkReplyAdmission(SubChunkReplyAdmissionEvent {
                dimension: key.dimension,
                positions: vec![[key.x, key.y, key.z]],
            }),
        )
        .unwrap();
    stream.expire_sub_chunk_deadlines(started + super::SUB_CHUNK_RESPONSE_TIMEOUT);

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

    assert_eq!(stream.stats().sub_chunk_timeouts, 0);
    assert_eq!(stream.stats().phase2_outcomes.stale, 0);
    assert_eq!(stream.stats().phase2_outcomes.all_air, 1);
    assert!(stream.loaded_columns.contains(&key.chunk()));
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
