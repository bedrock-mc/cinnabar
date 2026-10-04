use super::*;

/// Measures whether unrelated data extends a missing neighbour's elapsed wait.
#[test]
#[ignore = "release neighbour-wait timing; run with --ignored --nocapture"]
fn missing_neighbour_trickle_timing() {
    let (mut stream, key) = stream_with_one_expected_sub_chunk();
    let missing = SubChunkKey::new(key.dimension, key.x + 1, key.y, key.z);
    assert!(stream.sub_chunk_is_due(missing, Instant::now()));
    let started = Instant::now();
    std::thread::sleep(UNSENT_COLUMN_GRACE + Duration::from_millis(10));
    stream
        .submit(
            2,
            request_level_chunk_event(0, 8, 8, LevelChunkMode::LimitedRequests { highest: 0 }, 0),
        )
        .unwrap();
    complete_pending_decode_jobs(&mut stream);
    println!(
        "missing_neighbour elapsed_ms={} renewed_by_unrelated={}",
        started.elapsed().as_millis(),
        stream.sub_chunk_is_due(missing, Instant::now())
    );
    assert!(!stream.sub_chunk_is_due(missing, Instant::now()));
}

/// Data outside a missing neighbour's local cohort cannot renew its deadline.
#[test]
fn unrelated_arrivals_do_not_renew_neighbour_deadlines() {
    let (mut stream, key) = stream_with_one_expected_sub_chunk();
    let missing = SubChunkKey::new(key.dimension, key.x + 1, key.y, key.z);
    let deadline = stream.unsent_column_deadlines[&missing.chunk()];
    stream.record_column_arrival(ChunkKey::new(0, 8, 8), deadline);
    assert!(!stream.sub_chunk_is_due(missing, deadline));
    stream.record_column_arrival(key.chunk(), deadline);
    assert!(!stream.sub_chunk_is_due(missing, deadline));
    assert!(
        stream.sub_chunk_is_due(key, deadline),
        "an explicit request still blocks"
    );
}

/// New cohort data keeps slow delivery coherent; duplicates and foreign data cannot renew it.
#[test]
fn cohort_deadline_counts_only_new_local_progress() {
    let (mut stream, key) = stream_with_one_expected_sub_chunk();
    stream.publisher.cohort = Some(ViewCohort::from_publisher(0, [0, 0, 0], 64));
    let now = Instant::now();
    let missing = SubChunkKey::new(0, 1, key.y, 0);
    stream.record_column_arrival(key.chunk(), now);
    let first_deadline = now + UNSENT_COLUMN_GRACE;
    assert!(!stream.sub_chunk_is_due(missing, first_deadline));
    stream.record_column_arrival(ChunkKey::new(0, 2, 2), first_deadline);
    assert!(stream.sub_chunk_is_due(missing, first_deadline));
    let quiet = first_deadline + UNSENT_COLUMN_GRACE;
    stream.record_column_arrival(ChunkKey::new(0, 2, 2), quiet);
    stream.record_column_arrival(ChunkKey::new(0, 8, 8), quiet);
    assert!(!stream.sub_chunk_is_due(missing, quiet));
    let slot = SubChunkKey::new(0, 2, key.y, 2);
    stream.record_sub_chunk_arrival(slot, quiet);
    assert!(stream.sub_chunk_is_due(missing, quiet));
    let expired = quiet + UNSENT_COLUMN_GRACE;
    stream.record_sub_chunk_arrival(slot, expired);
    assert!(!stream.sub_chunk_is_due(missing, expired));
    stream.publisher.epoch += 1;
    assert!(!stream.sub_chunk_is_due(missing, quiet));
}
