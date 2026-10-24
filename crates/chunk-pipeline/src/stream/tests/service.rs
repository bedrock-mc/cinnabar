use super::*;

/// Queues `count` inline air columns with their decodes already completed.
fn stream_with_decoded_columns(count: u64) -> WorldStream {
    let mut stream = block_entity_visual_stream();
    for sequence in 1..=count {
        stream
            .submit(sequence, inline_air_event(sequence as i32))
            .unwrap();
    }
    while let Some(queued) = stream.pending_decode.pop_front() {
        stream.in_flight_decode_jobs += 1;
        let completion = queued.job.run(queued.queued_at);
        stream.decode_tx.send(completion).unwrap();
    }
    stream
}

/// A reclaim request stops a poll before its next heavy commit even with a whole slice left.
#[test]
fn raised_service_yield_stops_before_the_next_heavy_commit() {
    let mut stream = stream_with_decoded_columns(8);
    while let Ok(completion) = stream.decode_rx.try_recv() {
        stream.accept_decode_completion(completion);
    }
    stream.service_yield = Some(Arc::new(AtomicBool::new(true)));
    stream.frame_deadline = Some(Instant::now() + Duration::from_secs(60));
    stream.poll([0.0; 3], 0);
    assert_eq!(stream.committed_sequence(), 0);

    stream.service_yield = Some(Arc::new(AtomicBool::new(false)));
    stream.frame_deadline = Some(Instant::now() + Duration::from_secs(60));
    let report = stream.poll([0.0; 3], 0);
    assert_eq!(
        stream.committed_sequence(),
        8,
        "an unraised flag leaves the slice intact"
    );
    assert_eq!(report.commit_steps, 8, "the report counts the progress");
}

/// After a full allocation of service polling, the frame accepts decode results but leaves
/// chunk-data commits to the service. Less polling keeps the frame's usual guaranteed commit,
/// so a service starved of execution time never stalls terrain.
#[test]
fn frame_poll_leaves_chunk_data_after_a_full_service_poll_allocation() {
    let mut stream = stream_with_decoded_columns(4);
    stream.between_frames_service = true;
    stream.service_poll_time = stream.poll_budget;
    stream.begin_frame_work();
    let report = stream.poll([0.0; 3], 0);
    assert!(report.decoded_results > 0);
    assert_eq!(stream.committed_sequence(), 0);

    stream.service_poll_time = Duration::ZERO;
    stream.frame_deadline = Some(Instant::now());
    stream.poll([0.0; 3], 0);
    assert_eq!(
        stream.committed_sequence(),
        1,
        "the frame's guaranteed heavy commit"
    );
}

/// With no frame-thread poll at all, the service commits decoded terrain between frames and
/// hands the stream back with that work recorded.
#[test]
fn service_commits_decoded_terrain_without_a_frame_poll() {
    let mut service = WorldStreamService::spawn().unwrap();
    let mut stream = stream_with_decoded_columns(4);
    let mut decoded = 0;
    let started = Instant::now();
    while stream.committed_sequence() < 4 {
        assert!(
            started.elapsed() < Duration::from_secs(10),
            "service made no progress"
        );
        service.launch(stream, [0.0; 3], 0);
        assert!(service.is_servicing());
        test_time::idle();
        let serviced = service.reclaim().expect("the service holds the stream");
        assert!(!service.is_servicing());
        assert!(serviced.held >= serviced.busy);
        decoded += serviced.report.decoded_results;
        stream = serviced.stream;
    }
    assert_eq!(decoded, 4);
    assert!(
        service.reclaim().is_none(),
        "nothing is held once reclaimed"
    );
}

/// Dropping the service while it holds a stream drops that stream before returning, rather
/// than leaving it on a detached thread past application shutdown.
#[test]
fn dropping_a_servicing_service_drops_its_stream() {
    let mut service = WorldStreamService::spawn().unwrap();
    let stream = stream_with_decoded_columns(2);
    let witness = Arc::clone(&stream.admitted_mesh_jobs);
    service.launch(stream, [0.0; 3], 0);
    drop(service);
    assert_eq!(Arc::strong_count(&witness), 1);
}

/// Offloaded foreground polling reserves lighting and meshing time inside its smaller limit,
/// even when ingress already started the normal frame allocation.
#[test]
fn service_poll_clamps_the_commit_deadline_after_frame_work_started() {
    let mut stream = block_entity_visual_stream();
    stream.between_frames_service = true;
    stream.service_poll_time = stream.poll_budget;
    stream.begin_frame_work();
    let now = stream.frame_deadline.unwrap() - stream.poll_budget;
    let frame_deadline = stream.begin_poll_work(now);
    let allocation = super::super::commit_budget::WORLD_POLL_BUDGET_FLOOR;
    assert_eq!(frame_deadline, now + allocation);
    assert_eq!(stream.poll_deadline, Some(now + allocation / 2));
}

/// Starting the next poll never grants more time to an already exhausted commit lane.
#[test]
fn service_poll_preserves_an_earlier_commit_deadline() {
    let mut stream = block_entity_visual_stream();
    stream.between_frames_service = true;
    stream.service_poll_time = stream.poll_budget;
    let now = Instant::now();
    let expired = now - Duration::from_millis(1);
    stream.frame_deadline = Some(now + stream.poll_budget);
    stream.poll_deadline = Some(expired);
    stream.begin_poll_work(now);
    assert_eq!(stream.poll_deadline, Some(expired));
}

/// A removal-only slice makes progress even without worker results or dispatched jobs; the
/// service must keep polling the remaining removals instead of treating that slice as idle.
#[test]
fn service_poll_reports_removal_progress_with_more_removals_ready() {
    let mut stream = block_entity_visual_stream();
    for x in 0..4 {
        stream.mark_dirty_exact(SubChunkKey::new(0, x, 0, 0), Instant::now());
    }
    stream.frame_deadline = Some(Instant::now());
    let report = stream.poll([0.0; 3], 4);
    assert_eq!(stream.mesh_changes.len(), 1);
    assert_eq!(stream.mesh_jobs.pending.len(), 3);
    assert!(stream.mesh_jobs.in_flight.is_empty());
    assert_eq!(report.mesh_changes_queued, 1);
    assert_ne!(report, WorldStreamPoll::default());
}

/// Reclaim stops independent control commits as well as terrain commits.
#[test]
fn raised_service_yield_stops_light_commits() {
    let mut stream = block_entity_visual_stream();
    stream.service_yield = Some(Arc::new(AtomicBool::new(true)));
    stream.commit(1).unwrap();
    assert_eq!(stream.committed_sequence(), 0);
    stream.service_yield = None;
    stream.apply_ready();
    assert_eq!(stream.committed_sequence(), 1);
}

/// An expired poll cannot retire an entire teleported view before checking its budget.
#[test]
fn expired_poll_resumes_retention_before_later_commits() {
    let mut stream = block_entity_visual_stream();
    stream
        .submit(1, WorldEvent::ChunkRadiusUpdated(16))
        .unwrap();
    for x in -16..=16 {
        let column = ChunkKey::new(0, x, 0);
        stream.loaded_columns.insert(column);
        stream.record_known_air(SubChunkKey::from_chunk(column, 0));
    }
    stream.between_frames_service = true;
    stream.retain_local([3200.0, 0.0, 0.0]);
    stream.frame_deadline = Some(Instant::now());
    stream.poll([3200.0, 0.0, 0.0], 0);
    assert!(
        stream.loaded_column_count() > 0,
        "retention yields between columns"
    );
    assert!(
        stream.loaded_column_count() < 33,
        "retention makes progress"
    );

    stream.service_yield = Some(Arc::new(AtomicBool::new(true)));
    stream.commit(2).unwrap();
    assert_eq!(
        stream.committed_sequence(),
        1,
        "retention precedes the later commit"
    );
    let remaining = stream.loaded_column_count();
    stream.poll([3200.0, 0.0, 0.0], 0);
    assert_eq!(
        stream.loaded_column_count(),
        remaining,
        "reclaim performs no new item"
    );

    stream.service_yield = None;
    test_time::eventually("retention and its following commit", || {
        stream.poll([3200.0, 0.0, 0.0], 0);
        stream.committed_sequence() == 2
    });
    assert_eq!(stream.loaded_column_count(), 0);
}

/// Repeated ingress calls share one maintenance guarantee and cannot drain a paused view.
#[test]
fn expired_frame_ingress_keeps_retention_bounded() {
    let mut stream = block_entity_visual_stream();
    stream
        .submit(1, WorldEvent::ChunkRadiusUpdated(16))
        .unwrap();
    for x in -16..=16 {
        let column = ChunkKey::new(0, x, 0);
        stream.loaded_columns.insert(column);
        stream.record_known_air(SubChunkKey::from_chunk(column, 0));
    }
    stream.between_frames_service = true;
    stream.retain_local([3200.0, 0.0, 0.0]);
    stream.begin_frame_work();
    stream.poll_deadline = Some(Instant::now());
    stream.commit(2).unwrap();
    assert_eq!(stream.loaded_column_count(), 32);
    for _ in 0..16 {
        stream.apply_ready();
    }
    assert_eq!(stream.loaded_column_count(), 32, "one shared progress item");
    assert_eq!(
        stream.committed_sequence(),
        1,
        "later ingress remains fenced"
    );
}

/// A server teleport does not retire the whole source view inside its control commit.
#[test]
fn server_teleport_retirement_shares_the_frame_allocation() {
    for confirmed in [false, true] {
        let mut stream = block_entity_visual_stream();
        if confirmed {
            stream.chunk_radius = Some(16);
            stream.reevaluate_chunk_retention();
        }
        for x in -16..=16 {
            let column = ChunkKey::new(0, x, 0);
            stream.loaded_columns.insert(column);
            stream.record_known_air(SubChunkKey::from_chunk(column, 0));
        }
        stream.between_frames_service = true;
        stream.begin_frame_work();
        stream.poll_deadline = Some(Instant::now());
        stream
            .submit(
                1,
                WorldEvent::MovePlayer(MovePlayerEvent {
                    runtime_id: stream.authority.local_player_runtime_id(),
                    position: [3200.0, 0.0, 0.0],
                    mode: MovePlayerMode::Teleport,
                    ..Default::default()
                }),
            )
            .unwrap();
        assert_eq!(stream.loaded_column_count(), 32, "confirmed: {confirmed}");
        stream.commit(2).unwrap();
        assert_eq!(stream.committed_sequence(), 1);
        test_time::eventually("server teleport retirement", || {
            stream.poll([3200.0, 0.0, 0.0], 0);
            stream.committed_sequence() == 2
        });
        assert_eq!(stream.loaded_column_count(), 0);
    }
}
