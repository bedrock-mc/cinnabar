use super::*;

#[test]
fn light_waits_for_requested_above_during_partial_column_commit() {
    let mut stream = lit_stream(0);
    let key = SubChunkKey::new(0, 0, 4, 0);
    let above = SubChunkKey::new(0, 0, 5, 0);
    assert!(stream.light_dispatch_ready(key));
    stream
        .requests
        .requested
        .entry(key.chunk())
        .or_default()
        .insert(above.y, Default::default());
    assert!(!stream.light_dispatch_ready(key));
    install_current_light(&mut stream, above, 0, 15, true);
    assert!(stream.light_dispatch_ready(key));
    stream.mark_light_dirty_exact(above).unwrap();
    assert!(!stream.light_dispatch_ready(key));
}

/// A cancelled snapshot cannot publish even when its worker wins the cancellation race.
#[test]
fn superseded_mesh_completion_cannot_publish_geometry() {
    let mut stream = lit_stream(1);
    let key = SubChunkKey::new(1, 0, 0, 0);
    stream
        .authority
        .commit_sub_chunk(key, super::uniform_sub_chunk(2))
        .unwrap();
    install_current_light(&mut stream, key, 0, 0, false);
    stream.mark_dirty_exact(key, Instant::now());
    assert_eq!(stream.dispatch_mesh_jobs([0.0; 3], 1), 1);
    let cancelled = Arc::clone(&stream.mesh_cancellations[&key]);
    stream.mark_dirty_exact(key, Instant::now());
    assert!(cancelled.load(Ordering::Acquire));
    let completion = stream.mesh_rx.recv_timeout(Duration::from_secs(2)).unwrap();
    assert_eq!(stream.admitted_mesh_jobs.load(Ordering::Acquire), 1);
    stream.accept_mesh_completion(completion);
    assert_eq!(stream.admitted_mesh_jobs.load(Ordering::Acquire), 0);
    assert!(stream.mesh_changes.is_empty());
    assert_eq!(stream.stats().stale_mesh_jobs, 1);
}

/// Times a turn while a large mesh backlog is waiting for source readiness.
#[test]
#[ignore = "offline scheduler timing fixture"]
fn scheduler_turn_timing() {
    let mut stream = lit_stream(1);
    for x in 0..20_000 {
        let key = SubChunkKey::new(1, x, 0, 0);
        stream.resident.insert(key);
        stream.mark_dirty_exact(key, Instant::now());
    }
    let mut samples = Vec::new();
    for x in 0..31 {
        let started = Instant::now();
        assert_eq!(stream.dispatch_mesh_jobs([x as f32 * 16.0, 0.0, 0.0], 1), 0);
        samples.push(started.elapsed().as_micros());
    }
    samples.remove(0);
    samples.sort_unstable();
    println!(
        "scheduler_turn pending={} median_us={} p95_us={}",
        stream.mesh_jobs.pending.len(),
        samples[15],
        samples[28]
    );
}

#[test]
fn output_credit_survives_a_publication_without_gpu_allowance() {
    let mut stream = lit_stream(1);
    let key = SubChunkKey::new(1, 0, 0, 0);
    stream
        .authority
        .commit_sub_chunk(key, super::uniform_sub_chunk(2))
        .unwrap();
    install_current_light(&mut stream, key, 0, 0, false);
    stream.mark_dirty_exact(key, Instant::now());
    assert_eq!(stream.dispatch_mesh_jobs([0.0; 3], 1), 1);
    let completion = stream.mesh_rx.recv_timeout(Duration::from_secs(2)).unwrap();
    assert!(stream.mesh_memory.retained.load(Ordering::Acquire) > 0);
    stream.accept_mesh_completion(completion);
    let change = stream.pop_mesh_change().unwrap();
    assert!(stream.mesh_memory.retained.load(Ordering::Acquire) > 0);
    drop(change);
    assert_eq!(stream.mesh_memory.retained.load(Ordering::Acquire), 0);
}

#[test]
fn expired_poll_still_dispatches_one_ready_mesh() {
    let mut stream = lit_stream(1);
    for x in [0, 4] {
        let key = SubChunkKey::new(1, x, 0, 0);
        stream
            .authority
            .commit_sub_chunk(key, super::uniform_sub_chunk(2))
            .unwrap();
        install_current_light(&mut stream, key, 0, 0, false);
        stream.mark_dirty_exact(key, Instant::now());
    }
    stream.poll_deadline = Some(Instant::now() - Duration::from_secs(1));
    assert_eq!(stream.dispatch_mesh_jobs([0.0; 3], usize::MAX), 1);
    assert_eq!(stream.mesh_jobs.pending.len(), 1);
    let completion = stream.mesh_rx.recv_timeout(Duration::from_secs(2)).unwrap();
    stream.accept_mesh_completion(completion);
}

/// An expired slice advances one blocked candidate instead of checking the whole window.
#[test]
fn expired_mesh_slice_bounds_blocked_readiness_work() {
    let mut stream = lit_stream(1);
    let view = SchedulerView {
        position: [0.0; 3],
        forward: None,
    };
    stream
        .mesh_jobs
        .refresh
        .refresh(view, &mut stream.mesh_jobs.lanes, None, |_, _| true);
    for x in 10..42 {
        let key = SubChunkKey::new(1, x, 0, 0);
        stream
            .authority
            .commit_sub_chunk(key, super::uniform_sub_chunk(2))
            .unwrap();
        stream.resident.insert(key);
        let revision = stream.mark_dirty_exact(key, Instant::now());
        stream.mesh_jobs.lanes[RESIDENT_MESH_LANE]
            .ready
            .push(PendingSchedulerCandidate::new(key, revision, view, false));
    }
    stream.mesh_jobs.scan.clear();
    stream.poll_deadline = Some(Instant::now());
    assert_eq!(stream.dispatch_mesh_jobs(view.position, 1), 0);
    assert_eq!(stream.mesh_jobs.lanes[RESIDENT_MESH_LANE].deferred.len(), 1);
    assert_eq!(stream.mesh_jobs.lanes[RESIDENT_MESH_LANE].ready.len(), 31);
}

#[test]
fn expired_reversed_camera_reaches_near_work_before_old_backlog() {
    let mut stream = lit_stream(1);
    let old_view = SchedulerView {
        position: [8.0; 3],
        forward: None,
    };
    let destination = SubChunkKey::new(1, 4_095, 0, 0);
    for x in 0..=destination.x {
        let key = SubChunkKey::new(1, x, 0, 0);
        let revision = stream.mark_dirty_exact(key, Instant::now());
        stream.mesh_jobs.lanes[RESIDENT_MESH_LANE]
            .ready
            .push(PendingSchedulerCandidate::new(
                key, revision, old_view, false,
            ));
    }
    stream.mesh_jobs.scan.clear();
    for key in [SubChunkKey::new(1, 0, 0, 0), destination] {
        stream
            .authority
            .commit_sub_chunk(key, super::uniform_sub_chunk(2))
            .unwrap();
        install_current_light(&mut stream, key, 0, 0, false);
    }
    stream
        .mesh_jobs
        .refresh
        .refresh(old_view, &mut stream.mesh_jobs.lanes, None, |_, _| true);
    stream.poll_deadline = Some(Instant::now() - Duration::from_secs(1));
    assert_eq!(
        stream.dispatch_mesh_jobs([destination.x as f32 * 16.0 + 8.0, 8.0, 8.0], 1),
        1
    );
    assert!(stream.mesh_jobs.in_flight.contains_key(&destination));
    let completion = stream.mesh_rx.recv_timeout(Duration::from_secs(2)).unwrap();
    assert_eq!(completion.key, destination);
    stream.accept_mesh_completion(completion);
}

#[test]
fn near_light_column_keeps_its_nearest_members_priority_for_high_dependencies() {
    for z in [0, scheduler::NEAR_CAMERA_RADIUS + 1] {
        let mut stream = lit_stream(0);
        let range = vanilla_dimension_range(0).unwrap();
        let near = SubChunkKey::new(0, 0, 5, z);
        let top = SubChunkKey::new(
            0,
            0,
            range.base_sub_chunk_y + range.sub_chunk_count as i32 - 1,
            z,
        );
        let far = SubChunkKey::new(0, scheduler::NEAR_CAMERA_RADIUS * 2, 5, 0);
        for key in [near, top, far] {
            stream
                .authority
                .commit_sub_chunk(key, super::uniform_sub_chunk(2))
                .unwrap();
            install_current_light(&mut stream, key, 0, 0, false);
            stream.mark_light_dirty_exact(key).unwrap();
        }
        let view = SchedulerView {
            position: [8.0, 81.62, 8.0],
            forward: None,
        };
        stream.lighting.jobs.scan.clear();
        stream.lighting.jobs.lanes[0]
            .ready
            .push(PendingSchedulerCandidate::new(
                far,
                stream.lighting.jobs.pending[&far].revision,
                view,
                false,
            ));
        assert!(view.rank(top) > view.rank(far));
        assert!(view.rank(near) < view.rank(far));
        assert_eq!(stream.dispatch_light_jobs(view.position, 1), 1);
        assert!(stream.lighting.jobs.in_flight.contains_key(&top));
        assert!(!stream.lighting.jobs.in_flight.contains_key(&far));
    }
}
