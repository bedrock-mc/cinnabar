use super::*;

#[test]
fn light_jobs_are_nearest_first_deduplicated_and_worker_bounded() {
    let mut stream = lit_stream(0);
    let keys = (0..6)
        .map(|x| SubChunkKey::new(0, x, 0, 0))
        .collect::<Vec<_>>();
    for key in &keys {
        stream.record_known_air(*key);
    }
    stream.mark_light_changed_sources(keys.iter().copied());
    let latest = stream.lighting.jobs.pending[&keys[5]].revision;
    stream.mark_light_dirty_exact(keys[5]);
    assert_eq!(stream.lighting.jobs.pending.len(), keys.len());
    assert_eq!(stream.lighting.jobs.pending[&keys[5]].revision, latest);

    let expected = effective_light_job_cap().min(3);
    assert_eq!(
        stream.dispatch_light_jobs([8.0, 8.0, 8.0], usize::MAX),
        expected
    );
    assert_eq!(stream.lighting.jobs.in_flight.len(), expected);
    assert_eq!(
        stream
            .lighting
            .jobs
            .in_flight
            .keys()
            .copied()
            .collect::<BTreeSet<_>>(),
        [keys[0], keys[2], keys[4]]
            .into_iter()
            .take(expected)
            .collect()
    );
}

#[test]
fn light_worker_cap_retains_dependency_progress_on_small_pools() {
    assert_eq!(super::super::light_job_cap_for_threads(1), 2);
    assert_eq!(super::super::light_job_cap_for_threads(4), 2);
    assert_eq!(super::super::light_job_cap_for_threads(6), 3);
    assert_eq!(super::super::light_job_cap_for_threads(12), 6);
    assert_eq!(
        super::super::light_job_cap_for_threads(usize::MAX),
        super::super::MAX_IN_FLIGHT_LIGHT_JOBS
    );
}

/// Light caps read the world pool's light workers, not whichever rayon pool the caller runs in.
#[test]
fn light_caps_read_the_world_pool() {
    let light_workers = super::super::workers::WORKERS.size().background;
    let foreign = rayon::ThreadPoolBuilder::new()
        .num_threads(37)
        .build()
        .unwrap();
    let caps = foreign.install(|| {
        (
            super::super::effective_light_job_cap(),
            super::super::initial_light_job_cap(),
        )
    });
    assert_eq!(
        caps.0,
        super::super::light_job_cap_for_threads(light_workers)
    );
    assert_eq!(
        caps.1,
        light_workers.clamp(2, super::super::MAX_IN_FLIGHT_LIGHT_JOBS)
    );
}

#[test]
fn light_worker_dispatch_is_capped_and_pending_work_progresses() {
    let mut stream = lit_stream(1);
    let capacity = super::super::effective_light_job_cap();
    assert!((1..=super::super::MAX_IN_FLIGHT_LIGHT_JOBS).contains(&capacity));
    let radius = super::super::MAX_VIEW_RADIUS_CHUNKS;
    let keys = (-radius..=radius)
        .flat_map(|x| (-radius..=radius).map(move |z| (x, z)))
        .filter(|(x, z)| (x + z).rem_euclid(2) == 0)
        .map(|(x, z)| SubChunkKey::new(1, x, 0, z))
        .take(capacity + 1)
        .collect::<Vec<_>>();
    assert_eq!(keys.len(), capacity + 1);
    for key in &keys {
        stream.record_known_air(*key);
    }
    stream.mark_light_changed_sources(keys.iter().copied());

    assert_eq!(
        stream.dispatch_light_jobs([8.0, 8.0, 8.0], usize::MAX),
        capacity
    );
    assert_eq!(stream.lighting.jobs.in_flight.len(), capacity);
    assert_eq!(stream.lighting.jobs.pending.len(), 1);
    assert_eq!(stream.dispatch_light_jobs([8.0, 8.0, 8.0], usize::MAX), 0);
    assert_eq!(stream.lighting.jobs.in_flight.len(), capacity);
    assert_eq!(stream.lighting.jobs.pending.len(), 1);
    let completion = stream
        .lighting
        .rx
        .recv_timeout(Duration::from_secs(5))
        .expect("independent light completion");
    stream.accept_light_completion(completion);
    assert_eq!(stream.lighting.jobs.in_flight.len(), capacity - 1);
    assert_eq!(stream.lighting.jobs.pending.len(), 1);
    // Result delivery can precede the worker guard's final drop.
    let deadline = Instant::now() + Duration::from_secs(5);
    while stream.dispatch_light_jobs([8.0; 3], usize::MAX) == 0 {
        assert!(Instant::now() < deadline, "pending light work stalled");
        std::thread::yield_now();
    }
    assert_eq!(stream.lighting.jobs.in_flight.len(), capacity);
    assert!(stream.lighting.jobs.pending.is_empty());
}
