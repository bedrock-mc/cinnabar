use super::*;

fn stream() -> WorldStream {
    WorldStream::new(WorldBootstrap {
        local_player_unique_id: 1,
        dimension: 0,
        local_player_runtime_id: 1,
        player_position: [0.0; 3],
        world_spawn_position: [0; 3],
        air_network_id: 0,
        block_network_ids_are_hashes: false,
    })
}

/// Prepares four air columns: two ready commits and two completions awaiting the poll.
fn stream_with_completed_columns() -> WorldStream {
    let mut stream = stream();
    let range = protocol::vanilla_dimension_range(0).unwrap();
    let mut payload = vec![1, 0];
    payload.extend(std::iter::repeat_n(0xff, range.sub_chunk_count - 1));
    payload.push(0);
    for x in 0..4 {
        stream
            .submit(
                x as u64 + 1,
                WorldEvent::LevelChunk(LevelChunkEvent {
                    dimension: 0,
                    x,
                    z: 0,
                    mode: LevelChunkMode::Inline { count: 0 },
                    payload: payload.clone(),
                }),
            )
            .unwrap();
    }
    while let Some(queued) = stream.pending_decode.pop_front() {
        stream.in_flight_decode_jobs += 1;
        stream
            .decode_tx
            .send(queued.job.run(queued.queued_at))
            .unwrap();
    }
    for _ in 0..2 {
        let completion = stream.decode_rx.try_recv().unwrap();
        stream.accept_decode_completion(completion);
    }
    stream
}

/// Reclaims a loan with injected poll work and an older first-poll timestamp, without sleeping.
fn reclaim_after_poll_work(stream: WorldStream, busy: Duration) -> ServicedStream {
    let mut service = WorldStreamService::spawn().unwrap();
    let shared = Arc::clone(&service.shared);
    let turns = shared.turns.lock().unwrap();
    let since = Instant::now() - stream.poll_budget - busy;
    service.launch(stream, [0.0; 3], 0);
    {
        let mut slot = shared.slot();
        let lent = slot.lent.as_mut().unwrap();
        lent.serving_since = Some(since);
        lent.busy = busy;
    }
    let serviced = service
        .reclaim()
        .expect("the loan returns without a service turn");
    drop(turns);
    serviced
}

/// Idle ownership cannot take away the frame's bounded progress on already decoded terrain.
#[test]
fn idle_service_time_does_not_suppress_ready_foreground_commits() {
    let budget = commit_budget::WORLD_POLL_BUDGET;
    for busy in [
        Duration::ZERO,
        SERVICE_SLICE,
        budget - Duration::from_nanos(1),
        budget,
    ] {
        let serviced = reclaim_after_poll_work(stream_with_completed_columns(), busy);
        assert_eq!(serviced.busy, busy);
        assert_eq!(serviced.report, WorldStreamPoll::default());
        let mut stream = serviced.stream;
        stream.frame_deadline = Some(Instant::now());
        let report = stream.poll([0.0; 3], 0);
        let expected = usize::from(busy < budget);
        assert_eq!(
            report.decoded_results, 1,
            "the expired decode lane remains bounded"
        );
        assert_eq!(
            stream.committed_sequence(),
            expected as u64,
            "busy={busy:?}"
        );
        assert_eq!(stream.loaded_column_count(), expected, "busy={busy:?}");
        assert_eq!(
            report.commit_steps, expected,
            "only the guaranteed commit may run"
        );
        assert_eq!(
            stream.decode_rx.len(),
            1,
            "the other decode waits for another poll"
        );
        assert!(!stream.known_air.is_empty() || expected == 0);
    }
}

/// A failed service poll drops its stream and resumes the same panic on the reclaiming thread.
#[test]
fn reclaim_resumes_a_poll_panic_without_returning_the_stream() {
    let mut stream = stream_with_completed_columns();
    let witness = Arc::clone(&stream.admitted_mesh_jobs);
    stream.observe_commits(|_| panic!("service commit failed"));
    let mut service = WorldStreamService::spawn().unwrap();
    service.launch(stream, [0.0; 3], 0);
    let shared = Arc::clone(&service.shared);
    test_time::eventually("the service catches its poll panic", || {
        shared.slot().panic.is_some()
    });
    let result = catch_unwind(AssertUnwindSafe(|| service.reclaim()));
    let Err(payload) = result else {
        panic!("reclaim must resume the poll panic");
    };
    assert_eq!(
        payload.downcast_ref::<&str>(),
        Some(&"service commit failed")
    );
    assert!(!service.is_servicing());
    assert!(shared.slot().lent.is_none());
    assert_eq!(Arc::strong_count(&witness), 1);
    assert!(service.reclaim().is_none());
}

/// A service thread that cannot be scheduled must not hold the frame thread at reclaim.
#[test]
fn reclaim_does_not_wait_for_a_service_thread_that_cannot_run() {
    let mut service = WorldStreamService::spawn().unwrap();
    let shared = Arc::clone(&service.shared);
    let turns = shared.turns.lock().unwrap();
    service.launch(stream(), [0.0; 3], 0);
    let serviced = service.reclaim().expect("the lent stream comes back");
    assert_eq!(serviced.report, WorldStreamPoll::default());
    assert_eq!(serviced.held, Duration::ZERO);
    assert!(!service.is_servicing());
    drop(turns);
}

/// A lent stream is polled on the service thread without the frame thread driving it.
#[test]
fn the_service_polls_a_lent_stream_until_reclaimed() {
    let mut service = WorldStreamService::spawn().unwrap();
    service.launch(stream(), [0.0; 3], 0);
    let shared = Arc::clone(&service.shared);
    test_time::eventually("the service polls the lent stream", || {
        shared
            .slot()
            .lent
            .as_ref()
            .is_some_and(|lent| lent.serving_since.is_some())
    });
    let serviced = service.reclaim().expect("the lent stream comes back");
    assert!(serviced.stream.between_frames_service);
    service.launch(serviced.stream, [0.0; 3], 0);
    assert!(service.reclaim().is_some());
}

/// Dropping a service that holds a stream releases it and stops the thread.
#[test]
fn dropping_a_servicing_service_returns() {
    let mut service = WorldStreamService::spawn().unwrap();
    service.launch(stream(), [0.0; 3], 0);
    let shared = Arc::clone(&service.shared);
    drop(service);
    assert!(shared.slot().lent.is_none());
    assert!(shared.slot().shutdown);
}

/// A wake is consumed by the idle wait that observes it, so it cannot keep later waits spinning.
#[test]
fn idle_wait_consumes_the_wake_it_observes() {
    let stream = stream();
    let (sent, wake) = bounded(1);
    sent.send(()).unwrap();
    stream.idle_wait().wait(&wake, false, Duration::ZERO);
    assert!(wake.is_empty());
}
