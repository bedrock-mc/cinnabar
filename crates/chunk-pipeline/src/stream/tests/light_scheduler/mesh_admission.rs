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
        startup_center: None,
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
        startup_center: None,
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
            startup_center: None,
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

#[test]
fn startup_meshes_finish_tall_spawn_columns_before_distant_terrain() {
    for startup in [false, true] {
        let mut stream = lit_stream(1);
        let spawn = SubChunkKey::new(1, 1, 0, 0);
        let distant = SubChunkKey::new(1, 3, 5, 0);
        for key in [spawn, distant] {
            stream
                .authority
                .commit_sub_chunk(key, super::uniform_sub_chunk(2))
                .unwrap();
            install_current_light(&mut stream, key, 0, 0, false);
            stream.mark_dirty_exact(key, Instant::now());
        }
        stream.set_startup_priority(startup);
        assert_eq!(stream.dispatch_mesh_jobs([8.0, 80.0, 8.0], 1), 1);
        let expected = if startup { spawn } else { distant };
        assert!(stream.mesh_jobs.in_flight.contains_key(&expected));
        let completion = stream.mesh_rx.recv_timeout(Duration::from_secs(2)).unwrap();
        stream.accept_mesh_completion(completion);
    }
}

#[test]
fn startup_lighting_prioritizes_spawn_columns_without_a_near_height_member() {
    let mut stream = lit_stream(0);
    let range = vanilla_dimension_range(0).unwrap();
    let spawn = SubChunkKey::new(
        0,
        1,
        range.base_sub_chunk_y + range.sub_chunk_count as i32 - 1,
        0,
    );
    let distant = SubChunkKey::new(0, 4, 5, 0);
    for key in [spawn, distant] {
        stream
            .authority
            .commit_sub_chunk(key, super::uniform_sub_chunk(2))
            .unwrap();
        install_current_light(&mut stream, key, 0, 0, false);
        stream.mark_light_dirty_exact(key).unwrap();
    }
    stream.set_startup_priority(true);
    assert_eq!(stream.dispatch_light_jobs([8.0, 80.0, 8.0], 1), 1);
    assert!(stream.lighting.jobs.in_flight.contains_key(&spawn));
    assert!(!stream.lighting.jobs.in_flight.contains_key(&distant));
    let completion = stream
        .lighting
        .rx
        .recv_timeout(Duration::from_secs(5))
        .unwrap();
    stream.accept_light_completion(completion);
}

#[test]
fn late_spawn_section_bypasses_the_distant_ingress_backlog() {
    let mut stream = lit_stream(1);
    stream.set_startup_priority(true);
    let position = [8.0, 80.0, 8.0];
    let view = stream.scheduler_view(position);
    stream
        .mesh_jobs
        .refresh
        .refresh(view, &mut stream.mesh_jobs.lanes, None, |_, _| true);
    for x in 10..10 + MAX_PENDING_MESH_QUEUE_WORK_PER_POLL as i32 * 2 {
        let key = SubChunkKey::new(1, x, 5, 0);
        stream
            .authority
            .commit_sub_chunk(key, super::uniform_sub_chunk(2))
            .unwrap();
        install_current_light(&mut stream, key, 0, 0, false);
        stream.mark_dirty_exact(key, Instant::now());
    }
    let spawn = SubChunkKey::new(1, 1, 0, 0);
    stream
        .authority
        .commit_sub_chunk(spawn, super::uniform_sub_chunk(2))
        .unwrap();
    install_current_light(&mut stream, spawn, 0, 0, false);
    stream.mark_dirty_exact(spawn, Instant::now());
    assert_eq!(stream.dispatch_mesh_jobs(position, 1), 1);
    assert!(stream.mesh_jobs.in_flight.contains_key(&spawn));
    assert!(stream.mesh_jobs.scan.len() > MAX_PENDING_MESH_QUEUE_WORK_PER_POLL);
    assert!(!stream.mesh_jobs.pending[&SubChunkKey::new(1, 10, 5, 0)].urgent);
    let completion = stream.mesh_rx.recv_timeout(Duration::from_secs(2)).unwrap();
    stream.accept_mesh_completion(completion);
}

/// A nearby column arriving after the camera settles cannot wait behind distant propagation.
#[test]
fn stationary_spawn_light_competes_with_far_propagation_wakeups() {
    let mut stream = lit_stream(0);
    let view = SchedulerView {
        position: [8.0, 81.62, 8.0],
        forward: None,
        startup_center: None,
    };
    stream
        .lighting
        .jobs
        .refresh
        .refresh(view, &mut stream.lighting.jobs.lanes, None, |_, _| true);
    let range = vanilla_dimension_range(0).unwrap();
    let near = SubChunkKey::new(0, 0, 5, 0);
    let top = SubChunkKey::new(
        0,
        0,
        range.base_sub_chunk_y + range.sub_chunk_count as i32 - 1,
        0,
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
    stream
        .lighting
        .priority_wakeups
        .insert(far, stream.lighting.jobs.pending[&far].revision);
    assert_eq!(stream.dispatch_light_jobs(view.position, 1), 1);
    assert!(
        stream.lighting.jobs.in_flight.contains_key(&top),
        "the nearby column's skylight dependency must run before a distant wakeup"
    );
    let completion = stream
        .lighting
        .rx
        .recv_timeout(Duration::from_secs(2))
        .unwrap();
    assert_eq!(completion.key, top);
    stream.accept_light_completion(completion);
}

/// Distant meshes already queued ready, at a settled camera, with an expired slice.
fn stationary_stream_with_blocked_far_meshes() -> (WorldStream, SchedulerView) {
    let mut stream = lit_stream(1);
    let view = SchedulerView {
        position: [8.0; 3],
        forward: None,
        startup_center: None,
    };
    stream
        .mesh_jobs
        .refresh
        .refresh(view, &mut stream.mesh_jobs.lanes, None, |_, _| true);
    for x in [
        scheduler::NEAR_CAMERA_RADIUS + 1,
        scheduler::NEAR_CAMERA_RADIUS + 2,
    ] {
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
    stream.poll_deadline = Some(Instant::now() - Duration::from_secs(1));
    (stream, view)
}

fn finish_mesh(stream: &mut WorldStream, key: SubChunkKey) {
    assert!(stream.mesh_jobs.in_flight.contains_key(&key));
    let completion = stream.mesh_rx.recv_timeout(Duration::from_secs(2)).unwrap();
    assert_eq!(completion.key, key);
    stream.accept_mesh_completion(completion);
}

/// Fresh nearby geometry must dispatch ahead of blocked distant work once the camera settles.
#[test]
fn stationary_spawn_mesh_competes_with_blocked_far_work() {
    let (mut stream, view) = stationary_stream_with_blocked_far_meshes();
    let near = SubChunkKey::new(1, 0, 0, 0);
    stream
        .authority
        .commit_sub_chunk(near, super::uniform_sub_chunk(2))
        .unwrap();
    install_current_light(&mut stream, near, 0, 0, false);
    stream.mark_dirty_exact(near, Instant::now());
    assert_eq!(stream.dispatch_mesh_jobs(view.position, 1), 1);
    finish_mesh(&mut stream, near);
}

/// A nearby mesh deferred for its missing light, with no new scan record pending.
fn deferred_stationary_spawn_mesh() -> (WorldStream, SchedulerView, SubChunkKey, u64) {
    let (mut stream, view) = stationary_stream_with_blocked_far_meshes();
    let near = SubChunkKey::new(1, 0, 0, 0);
    stream
        .authority
        .commit_sub_chunk(near, super::uniform_sub_chunk(2))
        .unwrap();
    stream.resident.insert(near);
    let revision = stream.mark_dirty_exact(near, Instant::now());
    assert_eq!(stream.dispatch_mesh_jobs(view.position, 1), 0);
    assert!(stream.mesh_jobs.scan.is_empty());
    (stream, view, near, revision)
}

/// A coalesced light invalidation must wake the deferred mesh, even past a full worker pool.
#[test]
fn stationary_spawn_mesh_wakes_when_coalesced_light_becomes_ready() {
    let (mut stream, view, near, revision) = deferred_stationary_spawn_mesh();
    install_current_light(&mut stream, near, 0, 0, false);
    stream.mark_changed_light_mesh_dependents(near, [false; 6], Instant::now(), false);
    assert_eq!(stream.mesh_jobs.pending[&near].revision, revision);
    assert!(stream.mesh_jobs.scan.is_empty());
    stream
        .admitted_mesh_jobs
        .store(usize::MAX, Ordering::Release);
    assert_eq!(stream.dispatch_mesh_jobs(view.position, 1), 0);
    stream.admitted_mesh_jobs.store(0, Ordering::Release);
    assert_eq!(stream.dispatch_mesh_jobs(view.position, 1), 1);
    finish_mesh(&mut stream, near);
}

/// Light that completes unchanged invalidates nothing yet must still wake the deferred mesh.
#[test]
fn stationary_spawn_mesh_wakes_after_unchanged_light_completion() {
    let (mut stream, view, near, revision) = deferred_stationary_spawn_mesh();
    install_current_light(&mut stream, near, 0, 0, false);
    let direct = stream.lighting.direct_sky[&near].clone();
    stream.finish_accepted_light_completion(near, 0, &direct, [false; 6], false);
    assert_eq!(stream.mesh_jobs.pending[&near].revision, revision);
    assert!(stream.mesh_jobs.scan.is_empty());
    assert_eq!(stream.dispatch_mesh_jobs(view.position, 1), 1);
    finish_mesh(&mut stream, near);
}

/// Unrelated nearby wakeups must not re-offer a blocked nearest mesh ahead of ready geometry.
#[test]
fn blocked_nearest_mesh_stays_deferred_through_unrelated_wakeups() {
    let mut stream = lit_stream(1);
    let view = SchedulerView {
        position: [8.0; 3],
        forward: None,
        startup_center: None,
    };
    stream
        .mesh_jobs
        .refresh
        .refresh(view, &mut stream.mesh_jobs.lanes, None, |_, _| true);
    let blocked = SubChunkKey::new(1, 0, 0, 0);
    let ready = SubChunkKey::new(1, 2, 0, 0);
    let unrelated = SubChunkKey::new(1, -3, 0, 0);
    for key in [blocked, ready, unrelated] {
        stream
            .authority
            .commit_sub_chunk(key, super::uniform_sub_chunk(2))
            .unwrap();
        stream.resident.insert(key);
    }
    install_current_light(&mut stream, ready, 0, 0, false);
    for key in [blocked, ready, unrelated] {
        stream.mark_dirty_exact(key, Instant::now());
    }
    stream.poll_deadline = Some(Instant::now() - Duration::from_secs(1));
    for _ in 0..4 {
        stream.mark_changed_light_mesh_dependents(unrelated, [false; 6], Instant::now(), false);
        stream.dispatch_mesh_jobs(view.position, 1);
        if stream.mesh_jobs.in_flight.contains_key(&ready) {
            break;
        }
    }
    finish_mesh(&mut stream, ready);
}

/// A settled startup camera with a blocked spawn mesh ahead of ready distant geometry.
fn stationary_stream_with_blocked_spawn_mesh() -> (WorldStream, SchedulerView, SubChunkKey) {
    let mut stream = lit_stream(1);
    stream.set_startup_priority(true);
    let view = stream.scheduler_view([8.0, 80.0, 8.0]);
    stream
        .mesh_jobs
        .refresh
        .refresh(view, &mut stream.mesh_jobs.lanes, None, |_, _| true);
    let blocked = SubChunkKey::new(1, 0, 5, 0);
    let distant = SubChunkKey::new(1, scheduler::NEAR_CAMERA_RADIUS * 2, 5, 0);
    for (key, ready) in [(blocked, false), (distant, true)] {
        stream
            .authority
            .commit_sub_chunk(key, super::uniform_sub_chunk(2))
            .unwrap();
        install_current_light(&mut stream, key, 0, 0, false);
        let revision = stream.mark_dirty_exact(key, Instant::now());
        let candidate = PendingSchedulerCandidate::new(key, revision, view, false);
        let lane = &mut stream.mesh_jobs.lanes[RESIDENT_MESH_LANE];
        if ready {
            lane.ready.push(candidate);
        } else {
            lane.deferred.push(candidate);
        }
    }
    stream.mark_light_dirty_exact(blocked).unwrap();
    stream.mesh_jobs.scan.clear();
    stream.mesh_jobs.startup_scan.clear();
    stream.poll_deadline = Some(Instant::now() - Duration::from_secs(1));
    (stream, view, blocked)
}

/// Repeated light wakeups cannot make an unrelated blocked mesh bypass its startup backoff.
#[test]
fn stationary_mesh_light_wakeups_preserve_unrelated_backoff() {
    for changed in [false, true] {
        let (mut stream, view, blocked) = stationary_stream_with_blocked_spawn_mesh();
        let near = SubChunkKey::new(1, scheduler::NEAR_CAMERA_RADIUS, 5, 0);
        stream
            .authority
            .commit_sub_chunk(near, super::uniform_sub_chunk(2))
            .unwrap();
        install_current_light(&mut stream, near, 0, 0, false);
        let revision = stream.mark_dirty_exact(near, Instant::now());
        stream.mesh_jobs.scan.clear();
        stream.mesh_jobs.lanes[RESIDENT_MESH_LANE]
            .deferred
            .push(PendingSchedulerCandidate::new(near, revision, view, false));
        let mut dispatched = false;
        for _ in 0..4 {
            if changed {
                stream.mark_changed_light_mesh_dependents(near, [false; 6], Instant::now(), false);
            } else {
                let direct = stream.lighting.direct_sky[&near].clone();
                stream.finish_accepted_light_completion(near, 0, &direct, [false; 6], false);
            }
            assert_eq!(stream.mesh_jobs.pending[&near].revision, revision);
            if stream.dispatch_mesh_jobs(view.position, 1) == 1 {
                dispatched = true;
                break;
            }
        }
        assert!(dispatched && stream.mesh_jobs.in_flight.contains_key(&near));
        assert!(stream.mesh_jobs.pending.contains_key(&blocked));
        finish_mesh(&mut stream, near);
    }
}

/// Fresh nearby ingress cannot repeatedly probe an unrelated mesh that is cooling down.
#[test]
fn stationary_mesh_ingress_preserves_unrelated_backoff() {
    let (mut stream, view, blocked) = stationary_stream_with_blocked_spawn_mesh();
    let mut dispatched = None;
    for z in 0..4 {
        let near = SubChunkKey::new(1, scheduler::NEAR_CAMERA_RADIUS, 5, z);
        stream
            .authority
            .commit_sub_chunk(near, super::uniform_sub_chunk(2))
            .unwrap();
        install_current_light(&mut stream, near, 0, 0, false);
        stream.mark_dirty_exact(near, Instant::now());
        if stream.dispatch_mesh_jobs(view.position, 1) == 1 {
            dispatched = (0..=z)
                .map(|z| SubChunkKey::new(1, scheduler::NEAR_CAMERA_RADIUS, 5, z))
                .find(|key| stream.mesh_jobs.in_flight.contains_key(key));
            assert!(dispatched.is_some(), "ready nearby geometry must dispatch");
            break;
        }
    }
    let near = dispatched.expect("nearby ingress must dispatch while the blocked mesh backs off");
    assert!(stream.mesh_jobs.pending.contains_key(&blocked));
    finish_mesh(&mut stream, near);
}

/// A woken mesh the deadline cuts off must stay offered rather than fall behind distant work.
#[test]
fn deadline_keeps_an_unexamined_woken_mesh_for_the_next_poll() {
    let (mut stream, view) = stationary_stream_with_blocked_far_meshes();
    let blocked = SubChunkKey::new(1, 0, 0, 0);
    let ready = [
        SubChunkKey::new(1, -3, 0, 0),
        SubChunkKey::new(1, 0, 0, scheduler::NEAR_CAMERA_RADIUS),
    ];
    for key in std::iter::once(blocked).chain(ready) {
        stream
            .authority
            .commit_sub_chunk(key, super::uniform_sub_chunk(2))
            .unwrap();
        stream.resident.insert(key);
    }
    for key in ready {
        install_current_light(&mut stream, key, 0, 0, false);
    }
    for key in std::iter::once(blocked).chain(ready) {
        let revision = stream.mark_dirty_exact(key, Instant::now());
        stream.mesh_jobs.lanes[RESIDENT_MESH_LANE]
            .deferred
            .push(PendingSchedulerCandidate::new(key, revision, view, false));
    }
    stream.mesh_jobs.scan.clear();
    for key in std::iter::once(blocked).chain(ready) {
        stream.mark_changed_light_mesh_dependents(key, [false; 6], Instant::now(), false);
    }
    assert_eq!(
        stream.dispatch_mesh_jobs(view.position, 1),
        0,
        "the blocked nearest mesh spends the expired slice"
    );
    for key in ready {
        assert_eq!(stream.dispatch_mesh_jobs(view.position, 1), 1);
        finish_mesh(&mut stream, key);
    }
}

/// Wakeups beyond the probe limit must survive until higher-ranked work retires.
#[test]
fn truncation_keeps_unexamined_woken_meshes_for_later_polls() {
    let (mut stream, view) = stationary_stream_with_blocked_far_meshes();
    let radius = scheduler::NEAR_CAMERA_RADIUS;
    let ready = SubChunkKey::new(1, radius, radius, radius);
    let earlier: Vec<_> = scheduler::near_camera_keys(view, ready.dimension)
        .filter(|key| view.rank(*key) < view.rank(ready))
        .take(MAX_PENDING_SCHEDULER_SCANS_PER_POLL)
        .collect();
    assert_eq!(earlier.len(), MAX_PENDING_SCHEDULER_SCANS_PER_POLL);
    for key in earlier.iter().copied().chain([ready]) {
        stream
            .authority
            .commit_sub_chunk(key, super::uniform_sub_chunk(2))
            .unwrap();
        install_current_light(&mut stream, key, 0, 0, false);
        let revision = stream.mark_dirty_exact(key, Instant::now());
        stream.mesh_jobs.lanes[RESIDENT_MESH_LANE]
            .deferred
            .push(PendingSchedulerCandidate::new(key, revision, view, false));
        stream.mesh_jobs.refresh.wake_near(key);
    }
    stream.mesh_jobs.scan.clear();
    stream
        .admitted_mesh_jobs
        .store(usize::MAX, Ordering::Release);
    assert_eq!(stream.dispatch_mesh_jobs(view.position, 1), 0);
    stream.mesh_jobs.pending.retain(|key, _| *key == ready);
    stream.admitted_mesh_jobs.store(0, Ordering::Release);
    assert_eq!(stream.dispatch_mesh_jobs(view.position, 1), 1);
    finish_mesh(&mut stream, ready);
}

/// A camera move cannot discard an already woken mesh while worker capacity is exhausted.
#[test]
fn capacity_retry_keeps_a_woken_mesh_after_the_camera_moves() {
    let (mut stream, view) = stationary_stream_with_blocked_far_meshes();
    let earlier: Vec<_> = stream.mesh_jobs.pending.keys().copied().collect();
    let ready = SubChunkKey::new(1, -scheduler::NEAR_CAMERA_RADIUS, 0, 0);
    stream
        .authority
        .commit_sub_chunk(ready, super::uniform_sub_chunk(2))
        .unwrap();
    install_current_light(&mut stream, ready, 0, 0, false);
    let revision = stream.mark_dirty_exact(ready, Instant::now());
    stream.mesh_jobs.scan.clear();
    stream.mesh_jobs.lanes[RESIDENT_MESH_LANE]
        .deferred
        .push(PendingSchedulerCandidate::new(ready, revision, view, false));
    stream.mesh_jobs.refresh.wake_near(ready);
    let moved = stream.scheduler_view([view.position[0] + 16.0, 8.0, 8.0]);
    assert!(!scheduler::mesh_is_near(moved, ready));
    stream
        .admitted_mesh_jobs
        .store(usize::MAX, Ordering::Release);
    assert_eq!(stream.dispatch_mesh_jobs(moved.position, 1), 0);
    for key in earlier {
        stream.mesh_jobs.pending.remove(&key);
    }
    stream.admitted_mesh_jobs.store(0, Ordering::Release);
    assert_eq!(stream.dispatch_mesh_jobs(moved.position, 1), 1);
    finish_mesh(&mut stream, ready);
}

/// Commits `light` as `key`'s next light revision without touching its blocks, as a relight does.
fn relight_in_place(stream: &mut WorldStream, key: SubChunkKey, light: SubChunkLight) {
    let ownership = stream.lighting.ownership[&key];
    let light_revision = ownership.light_revision + 1;
    let light = light.with_generation(light_revision);
    if stream.authority.terrain().sub_chunk(key).is_some() {
        stream.lighting.store.insert_resident(key, light);
    } else {
        stream.lighting.store.insert_known_air(key, light);
    }
    stream.lighting.ownership.insert(
        key,
        LightOwnership {
            light_revision,
            ..ownership
        },
    );
    stream
        .lighting
        .direct_sky
        .get_mut(&key)
        .expect("installed direct sky")
        .light_revision = light_revision;
}

/// Meshes `center` with lit face neighbour `face`, then relights one cell of `face` before the
/// completion is accepted.
fn relight_face_cell_during_mesh(cell: [u8; 3]) -> WorldStream {
    let mut stream = stream();
    let center = SubChunkKey::new(0, 0, -4, 0);
    let face = SubChunkKey::new(0, 1, -4, 0);
    let decoded = DecodedLevelChunk::decode(
        center.y,
        1,
        include_bytes!(concat!(
            env!("CARGO_MANIFEST_DIR"),
            "/../world/fixtures/uniform_non_air.bin"
        )),
        &RAW_IDS,
    );
    stream
        .authority
        .commit_level_chunk(center.chunk(), decoded)
        .unwrap();
    install_current_light(&mut stream, center, 0, 0, false);
    install_current_light(&mut stream, face, 4, 5, true);
    stream.mark_dirty_exact(center, Instant::now());
    assert_eq!(stream.dispatch_mesh_jobs([0.0; 3], 1), 1);
    let completion = stream
        .mesh_rx
        .recv_timeout(Duration::from_secs(2))
        .expect("mesh worker completion");

    let mut relit = stream.lighting.store.light(face).unwrap().as_ref().clone();
    let [x, y, z] = cell;
    relit.set(LightChannel::Block, x, y, z, 12).unwrap();
    relight_in_place(&mut stream, face, relit);
    stream.accept_mesh_completion(completion);
    stream
}

/// A neighbour relight confined to cells the mesh never samples publishes the running mesh
/// instead of meshing the section again.
#[test]
fn relight_outside_the_sampled_shell_keeps_the_running_mesh() {
    // The centre samples only the x = 0 plane of its +x neighbour.
    let mut stream = relight_face_cell_during_mesh([1, 7, 7]);
    let center = SubChunkKey::new(0, 0, -4, 0);

    assert_eq!(stream.stats().stale_mesh_jobs, 0);
    assert_eq!(stream.take_mesh_changes().len(), 1);
    assert!(!stream.mesh_jobs.pending.contains_key(&center));
}

/// A relight of a cell the mesh samples still sends the section back to be meshed.
#[test]
fn relight_inside_the_sampled_shell_requeues_the_running_mesh() {
    let mut stream = relight_face_cell_during_mesh([0, 7, 7]);
    let center = SubChunkKey::new(0, 0, -4, 0);

    assert_eq!(stream.stats().stale_mesh_jobs, 1);
    assert!(stream.take_mesh_changes().is_empty());
    assert!(stream.mesh_jobs.pending.contains_key(&center));
}

/// The shell is the facing plane, edge or corner, or the whole section for the centre itself.
#[test]
fn sampled_light_shell_covers_only_cells_beside_the_centre() {
    let cells = |toward| sampled_light_shell(toward).collect::<BTreeSet<_>>();
    assert_eq!(cells([0, 0, 0]).len(), 4096);
    let face = cells([1, 0, 0]);
    assert_eq!(face.len(), 256);
    assert!(face.iter().all(|&[x, _, _]| x == 15));
    assert_eq!(cells([0, -1, 1]), (0..16).map(|x| [x, 0, 15]).collect());
    assert_eq!(cells([-1, 1, -1]), BTreeSet::from([[0, 15, 0]]));
}
