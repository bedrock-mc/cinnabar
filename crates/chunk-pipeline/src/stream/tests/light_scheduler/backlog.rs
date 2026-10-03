use super::*;

/// Empty removals drain without consuming geometry worker capacity or exceeding their limit.
#[test]
fn empty_removals_have_separate_bounded_service() {
    let mut stream = lit_stream(1);
    let removal_budget = 64;
    for x in 0..removal_budget + 1 {
        let key = SubChunkKey::new(1, x as i32, 0, 0);
        stream.record_known_air(key);
        stream.mark_dirty_exact(key, Instant::now());
    }
    assert_eq!(
        stream.dispatch_mesh_jobs_with_limits([0.0; 3], 0, removal_budget),
        0
    );
    assert_eq!(stream.mesh_changes.len(), removal_budget);
    assert_eq!(stream.pending_mesh.len(), 1);
    assert!(stream.in_flight.is_empty());
}

/// Times a mesh-heavy transfer after lighting converges, including frame-paced acceptance.
#[test]
fn ready_mesh_backlog_drains() {
    let mut stream = lit_stream(1);
    let count = 2_048;
    for index in 0..count {
        let key = SubChunkKey::new(1, (index % 64) * 4, 0, (index / 64) * 4);
        stream
            .authority
            .commit_sub_chunk(key, super::uniform_sub_chunk(2))
            .unwrap();
        install_current_light(&mut stream, key, 0, 0, false);
        stream.mark_dirty_exact(key, Instant::now());
    }
    let started = Instant::now();
    while !stream.pending_mesh.is_empty() || !stream.in_flight.is_empty() {
        stream.poll([0.0; 3], 64);
        acknowledge_mesh_changes(&mut stream);
        assert!(
            started.elapsed() < Duration::from_secs(30),
            "ready mesh backlog stalled"
        );
        std::thread::sleep(Duration::from_millis(8));
    }
    eprintln!(
        "ready_mesh_backlog count={count} drain_ms={} wait_ms={}",
        started.elapsed().as_millis(),
        stream.stats.max_mesh_queue_wait.as_millis()
    );
    assert_eq!(stream.stats.phase2_stages.mesh_jobs_completed, count as u64);
    assert_eq!(stream.stats.stale_mesh_jobs, 0);
}

/// A ready backlog supplies every worker, with room for results awaiting the next poll.
#[test]
fn mesh_admission_has_a_full_worker_wave() {
    let workers = 12;
    let pool = rayon::ThreadPoolBuilder::new()
        .num_threads(workers)
        .build()
        .unwrap();
    let mut stream = lit_stream(1);
    for x in 0..workers * 3 {
        let key = SubChunkKey::new(1, x as i32 * 4, 0, 0);
        stream
            .authority
            .commit_sub_chunk(key, super::uniform_sub_chunk(2))
            .unwrap();
        install_current_light(&mut stream, key, 0, 0, false);
        stream.mark_dirty_exact(key, Instant::now());
    }
    let dispatched = pool.install(|| stream.dispatch_mesh_jobs([0.0; 3], usize::MAX));
    assert!(
        dispatched >= workers,
        "only {dispatched} jobs admitted for {workers} workers"
    );
    for _ in 0..dispatched {
        stream.accept_mesh_completion(stream.mesh_rx.recv_timeout(Duration::from_secs(5)).unwrap());
    }
    assert_eq!(stream.admitted_mesh_jobs.load(Ordering::Acquire), 0);
}

/// Repeated light changes keep one successor while the cancelled predecessor retires.
#[test]
fn light_churn_supersedes_pending_mesh_in_place() {
    let mut stream = lit_stream(1);
    let key = SubChunkKey::new(1, 0, 0, 0);
    stream
        .authority
        .commit_sub_chunk(key, super::uniform_sub_chunk(2))
        .unwrap();
    install_current_light(&mut stream, key, 0, 0, false);
    let revision = stream.mark_dirty_exact(key, Instant::now());
    stream.pending_mesh.remove(&key);
    stream.pending_mesh_scan.clear();
    stream.in_flight.insert(key, revision);
    let cancelled = Arc::new(AtomicBool::new(false));
    stream
        .mesh_cancellations
        .insert(key, Arc::clone(&cancelled));
    stream.mark_changed_light_mesh_dependents(key, [true; 6], Instant::now(), false);
    let successor = stream.pending_mesh[&key];
    for _ in 0..100 {
        stream.mark_changed_light_mesh_dependents(key, [true; 6], Instant::now(), true);
    }
    assert!(cancelled.load(Ordering::Acquire));
    assert_ne!(successor.revision, revision);
    assert_eq!(stream.pending_mesh[&key].revision, successor.revision);
    assert_eq!(stream.pending_mesh[&key].queued_at, successor.queued_at);
    assert!(stream.pending_mesh[&key].urgent);
    assert!(stream.pending_mesh_scan.len() <= 2);

    stream.in_flight.remove(&key);
    stream.mark_light_dirty_exact(key).unwrap();
    assert_eq!(stream.dispatch_mesh_jobs([0.0; 3], 1), 0);
    complete_one_light(&mut stream, [0.0; 3]);
    assert_eq!(stream.dispatch_mesh_jobs([0.0; 3], 1), 1);
    let completion = stream.mesh_rx.recv_timeout(Duration::from_secs(5)).unwrap();
    assert_eq!(completion.revision, successor.revision);
    stream.accept_mesh_completion(completion);
    assert_eq!(stream.stats.stale_mesh_jobs, 0);
    assert!(stream.pending_mesh.is_empty());
}

/// Exercises thousands of resident sub-chunks with roofs, emitters and repeated relighting.
#[test]
fn large_lighting_backlog_drains() {
    let mut stream = lit_stream(0);
    let range = vanilla_dimension_range(0).unwrap();
    let mut keys = Vec::new();
    for x in -6..=6 {
        for z in -6..=6 {
            for offset in 0..range.sub_chunk_count {
                let y = range.base_sub_chunk_y + offset as i32;
                let key = SubChunkKey::new(0, x, y, z);
                let id = if y == 19 && (x + z) % 2 == 0 {
                    Some(2)
                } else if y == 0 && (x + z) % 3 == 0 {
                    Some(1)
                } else if y == 8 {
                    Some(3)
                } else {
                    None
                };
                if let Some(id) = id {
                    stream
                        .authority
                        .commit_sub_chunk(key, super::uniform_sub_chunk(id))
                        .unwrap();
                    stream.resident.insert(key);
                } else {
                    stream.record_known_air(key);
                }
                keys.push(key);
            }
        }
    }
    stream.mark_changed_sources(keys.iter().copied(), Instant::now());
    let started = Instant::now();
    let mut polls = Vec::new();
    for frame in 0_u64.. {
        if frame == 8 || frame == 16 {
            stream.mark_light_changed_sources(keys.iter().copied());
        }
        let poll = Instant::now();
        stream.poll([8.0, 81.62, 8.0], 64);
        polls.push(poll.elapsed().as_micros());
        acknowledge_mesh_changes(&mut stream);
        if frame % 100 == 0 {
            eprintln!(
                "backlog ms={} pending_mesh={} flight_mesh={} pending_light={} flight_light={} accepted={} stale_mesh={}",
                started.elapsed().as_millis(),
                stream.pending_mesh.len(),
                stream.in_flight.len(),
                stream.pending_light.len(),
                stream.in_flight_light.len(),
                stream.stats.accepted_light_jobs,
                stream.stats.stale_mesh_jobs
            );
        }
        if frame > 16
            && stream.pending_mesh.is_empty()
            && stream.in_flight.is_empty()
            && stream.pending_light.is_empty()
            && stream.in_flight_light.is_empty()
            && stream.staged_mesh_completions.is_empty()
        {
            polls.sort_unstable();
            eprintln!(
                "backlog drained subchunks={} ms={} poll_p99_us={} max_us={} stats={:?}",
                keys.len(),
                started.elapsed().as_millis(),
                polls[polls.len() * 99 / 100],
                polls.last().unwrap(),
                stream.stats()
            );
            assert!(keys.iter().all(|key| stream.light_is_current(*key)));
            return;
        }
        assert!(
            started.elapsed() < Duration::from_secs(60),
            "backlog failed to drain: {:?}",
            stream.stats()
        );
        std::thread::sleep(Duration::from_millis(2));
    }
}

/// Retires publications just as a completed upload acknowledgement does.
pub(super) fn acknowledge_mesh_changes(stream: &mut WorldStream) {
    for change in stream.take_mesh_changes() {
        match change {
            WorldMeshChange::Upsert {
                key,
                generation,
                dirty_since,
                ..
            }
            | WorldMeshChange::Remove {
                key,
                generation,
                dirty_since,
                ..
            } => {
                stream.acknowledge_mesh_upload(key, generation, dirty_since, Instant::now());
            }
        }
    }
}

/// Spending ingress's slice still leaves a full ready worker wave for this frame.
#[test]
fn ingress_cannot_spend_the_mesh_service_slice() {
    let mut stream = lit_stream(1);
    for x in 0..32 {
        let key = SubChunkKey::new(1, x * 4, 0, 0);
        stream
            .authority
            .commit_sub_chunk(key, super::uniform_sub_chunk(2))
            .unwrap();
        install_current_light(&mut stream, key, 0, 0, false);
        stream.mark_dirty_exact(key, Instant::now());
    }
    stream.begin_frame_work();
    stream.frame_deadline = Some(Instant::now() + Duration::from_secs(1));
    stream.poll_deadline = Some(Instant::now());
    let report = stream.poll([0.0; 3], 32);
    assert!(
        report.mesh_jobs_dispatched > 1,
        "ingress starved ready meshes: {report:?}"
    );
}

/// A burst touching an already pending snapshot cannot grow revision or scan history.
#[test]
fn arriving_neighbours_coalesce_before_mesh_snapshot() {
    let mut stream = lit_stream(1);
    let key = SubChunkKey::new(1, 0, 0, 0);
    stream
        .authority
        .commit_sub_chunk(key, super::uniform_sub_chunk(2))
        .unwrap();
    install_current_light(&mut stream, key, 0, 0, false);
    stream.mark_changed(key, Instant::now());
    let pending = stream.pending_mesh[&key];
    let queued = stream.pending_mesh_scan.len();
    for _ in 0..100 {
        stream.mark_changed(key, Instant::now());
    }
    assert_eq!(stream.pending_mesh[&key].revision, pending.revision);
    assert_eq!(stream.pending_mesh[&key].queued_at, pending.queued_at);
    assert_eq!(stream.pending_mesh_scan.len(), queued);
}
