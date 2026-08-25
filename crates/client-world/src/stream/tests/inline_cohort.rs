use super::*;

/// Cohort stream with a publisher epoch over the radius-one column set
/// {(-1,0),(0,0),(1,0)} so inline admissions are cohort-gated exactly like
/// request-mode announcements.
fn publisher_cohort_stream() -> WorldStream {
    let mut stream = WorldStream::new(WorldBootstrap {
        local_player_unique_id: 1,
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
            WorldEvent::PublisherUpdate(PublisherUpdateEvent {
                center: [0, 64, 0],
                radius_blocks: 16,
            }),
        )
        .unwrap();
    stream
}

#[test]
fn inline_only_server_reaches_an_exact_required_cohort() {
    let mut stream = publisher_cohort_stream();
    // A server that streams every column fully inline must still produce the
    // frozen count/hash/presentation proof: each admitted column joins the
    // required cohort once and loads immediately.
    stream.submit(2, inline_air_event(-1)).unwrap();
    stream.submit(3, inline_air_event(0)).unwrap();
    stream.submit(4, inline_air_event(1)).unwrap();
    assert_eq!(stream.pending_decode.len(), 3);

    complete_pending_decode_jobs(&mut stream);

    let target = stream
        .committed_view_cohort
        .expect("publisher update committed a view cohort");
    let status = stream.cohort_status(target);
    assert_eq!(status.expected, 3);
    assert_eq!(status.loaded_target, 3);
    assert_eq!(status.missing_target, 0);
    assert!(
        status.is_exact(),
        "inline-only cohort must reach exact completeness: {status:?}"
    );

    // The publication snapshot consumed by live evidence must leave the
    // empty-cohort constant behind.
    let snapshot = stream.phase2_publication_snapshot(ChunkKey::new(0, 0, 0));
    assert_eq!(snapshot.required_columns, 3);
    assert_eq!(snapshot.loaded_required_columns, 3);
    assert_ne!(
        snapshot.required_cohort_hash,
        super::super::diagnostics::deterministic_chunk_key_hash(&BTreeSet::new())
    );
}

#[test]
fn failed_inline_decode_never_joins_the_required_cohort() {
    let mut stream = publisher_cohort_stream();
    let key = ChunkKey::new(0, 0, 0);

    // A structurally complete but semantically unusable inline payload fails
    // as survivable policy (established SubChunkYOverflow precedent) and must
    // stay outside readiness without ending the session.
    stream.accept_decode_completion(super::DecodeCompletion {
        sequence: 2,
        queue_wait: Duration::ZERO,
        event: super::PreparedWorldEvent::InlineLevelChunk {
            event: LevelChunkEvent {
                dimension: 0,
                x: 0,
                z: 0,
                mode: LevelChunkMode::Inline { count: 1 },
                payload: Vec::new(),
            },
            decoded: Err(world::DecodeError::SubChunkYOverflow {
                first: i32::MAX,
                offset: 1,
            }),
            duration: Duration::ZERO,
        },
    });
    stream.apply_ready();

    assert!(!stream.required_columns().contains(&key));
    assert!(!stream.loaded_columns.contains(&key));
    assert_eq!(stream.stats().decode_errors, 1);
    assert!(stream.take_fatal_error().is_none());

    // Ordering witness mirroring the request-mode contract: membership waits
    // for decode completion, never the announcement itself, and the stream
    // stays usable after the failed delivery.
    stream.submit(3, inline_air_event(0)).unwrap();
    assert!(!stream.required_columns().contains(&key));
    complete_pending_decode_jobs(&mut stream);
    assert!(stream.required_columns().contains(&key));
}

#[test]
fn inline_replacement_and_shared_eviction_invalidate_then_restore_completeness() {
    let mut stream = publisher_cohort_stream();
    let key = ChunkKey::new(0, 0, 0);
    stream.submit(2, inline_air_event(0)).unwrap();
    complete_pending_decode_jobs(&mut stream);

    let target = stream.committed_view_cohort.unwrap();
    assert!(stream.cohort_status(target).is_exact());

    // A replacement announcement for the same column counts once and keeps
    // the completed proof intact.
    stream.submit(3, inline_air_event(0)).unwrap();
    complete_pending_decode_jobs(&mut stream);
    assert_eq!(stream.required_columns().len(), 1);
    assert!(stream.cohort_status(target).is_exact());

    // Shared eviction invalidates completeness exactly as it does for
    // request-mode columns: the column stays required but stops loading.
    stream.evict_column(key);
    let evicted = stream.cohort_status(target);
    assert_eq!(evicted.expected, 1);
    assert_eq!(evicted.loaded_target, 0);
    assert_eq!(evicted.missing_target, 1);
    assert!(!evicted.is_exact());

    // Inline re-delivery restores completeness through the same admission
    // ordering as the first delivery.
    stream.submit(4, inline_air_event(0)).unwrap();
    complete_pending_decode_jobs(&mut stream);
    assert!(stream.cohort_status(target).is_exact());
}

#[test]
fn mixed_inline_and_request_mode_announcements_form_one_exact_cohort() {
    let mut stream = publisher_cohort_stream();
    // Two columns arrive fully inline while the remaining cohort column
    // arrives through the request-mode path (authoritative upper air, no
    // sub-chunk traffic).
    stream.submit(2, inline_air_event(-1)).unwrap();
    stream.submit(3, inline_air_event(1)).unwrap();
    stream
        .submit(
            4,
            request_level_chunk_event(0, 0, 0, LevelChunkMode::LimitedRequests { highest: 0 }, 7),
        )
        .unwrap();

    complete_pending_decode_jobs(&mut stream);

    let target = stream.committed_view_cohort.unwrap();
    let status = stream.cohort_status(target);
    assert_eq!(status.expected, 3);
    assert_eq!(status.loaded_target, 3);
    assert_eq!(status.missing_target, 0);
    assert!(
        status.is_exact(),
        "mixed-path cohort must count each column once: {status:?}"
    );

    // The same column announced again through the other path must not double
    // count or disturb the single coherent cohort.
    stream.submit(5, inline_air_event(0)).unwrap();
    complete_pending_decode_jobs(&mut stream);
    assert_eq!(stream.required_columns().len(), 3);
    assert_eq!(stream.loaded_column_count(), 3);
    assert!(stream.cohort_status(target).is_exact());
}
