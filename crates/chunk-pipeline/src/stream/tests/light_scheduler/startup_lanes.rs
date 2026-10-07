use super::*;

fn stationary_lighting(startup: bool) -> (WorldStream, SchedulerView) {
    let mut stream = lit_stream(1);
    stream.set_startup_priority(startup);
    let view = stream.scheduler_view([8.0, 80.0, 8.0]);
    stream
        .lighting
        .jobs
        .refresh
        .refresh(view, &mut stream.lighting.jobs.lanes, None, |_, _| true);
    (stream, view)
}

fn queue_light(stream: &mut WorldStream, view: SchedulerView, key: SubChunkKey, ready: bool) {
    install_current_light(stream, key, 0, 0, false);
    let revision = stream.mark_light_dirty_exact(key).unwrap();
    let candidate = PendingSchedulerCandidate::new(key, revision, view, false);
    let lane = &mut stream.lighting.jobs.lanes[0];
    if ready {
        lane.ready.push(candidate);
    } else {
        lane.deferred.push(candidate);
    }
    stream.lighting.jobs.scan.clear();
}

#[test]
fn startup_deferred_spawn_lighting_precedes_continuous_ready_distant_work() {
    let (mut stream, view) = stationary_lighting(true);
    let spawn = [SubChunkKey::new(1, 0, 19, 0), SubChunkKey::new(1, 1, 19, 0)];
    for key in spawn {
        queue_light(&mut stream, view, key, false);
    }
    for (index, expected) in spawn.into_iter().enumerate() {
        let distant = SubChunkKey::new(1, 8 + index as i32, 5, 0);
        queue_light(&mut stream, view, distant, true);
        assert_eq!(stream.dispatch_light_jobs(view.position, 1), 1);
        assert!(stream.lighting.jobs.in_flight.contains_key(&expected));
        assert!(!stream.lighting.jobs.in_flight.contains_key(&distant));
        assert!(stream.lighting.jobs.pending.contains_key(&distant));
        let completion = stream
            .lighting
            .rx
            .recv_timeout(Duration::from_secs(5))
            .unwrap();
        stream.accept_light_completion(completion);
        // Acceptance may wake the second key; the deferred pending record remains authoritative.
        stream.lighting.jobs.scan.clear();
    }
}

#[test]
fn ordinary_stationary_lighting_keeps_ready_lane_first() {
    let (mut stream, view) = stationary_lighting(false);
    let near = SubChunkKey::new(1, 0, 5, 0);
    let distant = SubChunkKey::new(1, 8, 5, 0);
    queue_light(&mut stream, view, near, false);
    queue_light(&mut stream, view, distant, true);
    assert_eq!(stream.dispatch_light_jobs(view.position, 1), 1);
    assert!(stream.lighting.jobs.in_flight.contains_key(&distant));
    assert!(stream.lighting.jobs.pending.contains_key(&near));
    let completion = stream
        .lighting
        .rx
        .recv_timeout(Duration::from_secs(5))
        .unwrap();
    stream.accept_light_completion(completion);
}

#[test]
fn startup_deferred_lane_discards_superseded_revision_and_bounds_work() {
    let (mut stream, view) = stationary_lighting(true);
    let distant = SubChunkKey::new(1, 8, 5, 0);
    queue_light(&mut stream, view, distant, true);
    let count = MAX_PENDING_SCHEDULER_SCANS_PER_POLL * 3;
    for y in 0..count as i32 {
        let key = SubChunkKey::new(1, 0, y, 0);
        let pending = PendingLight {
            revision: 2,
            urgent: false,
            queued_at: Instant::now(),
        };
        stream.lighting.jobs.pending.insert(key, pending);
        stream.lighting.jobs.lanes[0]
            .deferred
            .push(PendingSchedulerCandidate::new(key, 1, view, false));
    }
    assert!(
        !stream
            .lighting
            .jobs
            .ingress(view, None, |_, _, _| (0, false))
    );
    assert_eq!(stream.lighting.jobs.lanes[0].ready.len(), 1);
    assert_eq!(
        stream.lighting.jobs.lanes[0].deferred.len(),
        count - MAX_PENDING_SCHEDULER_SCANS_PER_POLL
    );
    let near = SubChunkKey::new(1, 0, 5, 0);
    stream.lighting.jobs.lanes[0]
        .deferred
        .push(PendingSchedulerCandidate::new(near, 2, view, false));
    stream
        .lighting
        .jobs
        .ingress(view, None, |_, _, _| (0, false));
    assert_eq!(
        stream.lighting.jobs.lanes[0].ready.peek().unwrap().key,
        near
    );
    assert_eq!(
        stream.lighting.jobs.lanes[0].ready.peek().unwrap().revision,
        2
    );
}

#[test]
fn startup_deferred_priority_keeps_urgent_ready_work_first() {
    let (mut stream, view) = stationary_lighting(true);
    let near = SubChunkKey::new(1, 0, 19, 0);
    let urgent = SubChunkKey::new(1, 8, 5, 0);
    queue_light(&mut stream, view, near, false);
    queue_light(&mut stream, view, urgent, true);
    stream.lighting.jobs.lanes[0].ready.clear();
    let revision = stream
        .mark_light_dirty_exact_with_priority(urgent, true)
        .unwrap();
    stream.lighting.jobs.lanes[0]
        .ready
        .push(PendingSchedulerCandidate::new(urgent, revision, view, true));
    stream.lighting.jobs.scan.clear();
    assert_eq!(stream.dispatch_light_jobs(view.position, 1), 1);
    assert!(stream.lighting.jobs.in_flight.contains_key(&urgent));
    assert!(stream.lighting.jobs.pending.contains_key(&near));
    let completion = stream
        .lighting
        .rx
        .recv_timeout(Duration::from_secs(5))
        .unwrap();
    stream.accept_light_completion(completion);
}

#[test]
fn startup_deferred_spawn_mesh_precedes_ready_distant_geometry() {
    let mut stream = lit_stream(1);
    stream.set_startup_priority(true);
    let view = stream.scheduler_view([8.0, 80.0, 8.0]);
    stream
        .mesh_jobs
        .refresh
        .refresh(view, &mut stream.mesh_jobs.lanes, None, |_, _| true);
    let spawn = SubChunkKey::new(1, 0, 0, 0);
    let distant = SubChunkKey::new(1, 8, 5, 0);
    for (key, ready) in [(spawn, false), (distant, true)] {
        stream
            .authority
            .commit_sub_chunk(key, super::uniform_sub_chunk(2))
            .unwrap();
        install_current_light(&mut stream, key, 0, 0, false);
        stream.mark_dirty_exact(key, Instant::now());
        let candidate = PendingSchedulerCandidate::new(
            key,
            stream.mesh_jobs.pending[&key].revision,
            view,
            false,
        );
        let lane = &mut stream.mesh_jobs.lanes[RESIDENT_MESH_LANE];
        if ready {
            lane.ready.push(candidate);
        } else {
            lane.deferred.push(candidate);
        }
    }
    stream.mesh_jobs.scan.clear();
    assert_eq!(stream.dispatch_mesh_jobs(view.position, 1), 1);
    assert!(stream.mesh_jobs.in_flight.contains_key(&spawn));
    assert!(stream.mesh_jobs.pending.contains_key(&distant));
    let completion = stream.mesh_rx.recv_timeout(Duration::from_secs(5)).unwrap();
    stream.accept_mesh_completion(completion);
}

#[test]
fn startup_priority_holds_until_local_terrain_is_ready() {
    let mut stream = lit_stream(1);
    stream.set_startup_priority(true);
    assert!(!stream.finish_startup_priority());
    assert!(stream.scheduler_view([0.0; 3]).startup_center.is_some());
    let dimension = stream.authority.current_dimension();
    let position = stream.resolved_server_position().position;
    let (x, z) = (
        floor_to_i32(position[0]).div_euclid(16),
        floor_to_i32(position[2]).div_euclid(16),
    );
    for dx in -1..=1 {
        for dz in -1..=1 {
            stream
                .loaded_columns
                .insert(ChunkKey::new(dimension, x + dx, z + dz));
        }
    }
    assert!(stream.finish_startup_priority());
    assert!(stream.scheduler_view([0.0; 3]).startup_center.is_none());
    assert!(!stream.is_startup_dependency(SubChunkKey::new(dimension, x, 0, z)));
}

/// A spawn mesh blocked on its light halo cannot claim every expired slice ahead of ready terrain.
#[test]
fn blocked_spawn_mesh_cannot_starve_ready_geometry_in_expired_polls() {
    let mut stream = lit_stream(1);
    stream.set_startup_priority(true);
    let view = stream.scheduler_view([8.0, 80.0, 8.0]);
    stream
        .mesh_jobs
        .refresh
        .refresh(view, &mut stream.mesh_jobs.lanes, None, |_, _| true);
    let spawn = SubChunkKey::new(1, 0, 0, 0);
    let distant = SubChunkKey::new(1, 8, 5, 0);
    for key in [spawn, distant] {
        stream
            .authority
            .commit_sub_chunk(key, super::uniform_sub_chunk(2))
            .unwrap();
    }
    install_current_light(&mut stream, distant, 0, 0, false);
    install_current_light(&mut stream, spawn, 0, 0, false);
    stream.mark_light_dirty_exact(spawn).unwrap();
    for (key, ready) in [(spawn, false), (distant, true)] {
        stream.mark_dirty_exact(key, Instant::now());
        let candidate = PendingSchedulerCandidate::new(
            key,
            stream.mesh_jobs.pending[&key].revision,
            view,
            false,
        );
        let lane = &mut stream.mesh_jobs.lanes[RESIDENT_MESH_LANE];
        if ready {
            lane.ready.push(candidate);
        } else {
            lane.deferred.push(candidate);
        }
    }
    stream.mesh_jobs.scan.clear();
    let mut dispatched = false;
    for _ in 0..4 {
        stream.poll_deadline = Some(Instant::now());
        if stream.dispatch_mesh_jobs(view.position, 1) == 1 {
            dispatched = true;
            break;
        }
    }
    assert!(dispatched && stream.mesh_jobs.in_flight.contains_key(&distant));
    assert!(stream.mesh_jobs.pending.contains_key(&spawn));
    let completion = stream.mesh_rx.recv_timeout(Duration::from_secs(5)).unwrap();
    stream.accept_mesh_completion(completion);
}

/// Startup priority never queues ahead of an urgent live mutation, even one poll at a time.
#[test]
fn urgent_mutation_ingresses_before_startup_dependencies_in_expired_polls() {
    let (mut stream, view) = stationary_lighting(true);
    let urgent = SubChunkKey::new(1, 8, 5, 0);
    let spawn: Vec<_> = (16..20).map(|y| SubChunkKey::new(1, 0, y, 0)).collect();
    for key in spawn.iter().chain([&urgent]) {
        install_current_light(&mut stream, *key, 0, 0, false);
    }
    stream
        .mark_light_dirty_exact_with_priority(urgent, true)
        .unwrap();
    for key in &spawn {
        stream.mark_light_dirty_exact(*key).unwrap();
    }
    stream
        .lighting
        .jobs
        .ingress(view, Some(Instant::now()), |_, _, _| (0, true));
    let lane = &stream.lighting.jobs.lanes[0];
    assert_eq!(lane.ready.len() + lane.deferred.len(), 1);
    assert_eq!(
        lane.ready.peek().map(|candidate| candidate.key),
        Some(urgent)
    );
}

/// A superseded scan head cannot hide the live urgent record behind it from ingress.
#[test]
fn stale_scan_head_does_not_hide_urgent_work_behind_startup_ingress() {
    let (mut stream, view) = stationary_lighting(true);
    let urgent = SubChunkKey::new(1, 8, 5, 0);
    let stale = SubChunkKey::new(1, 9, 5, 0);
    let spawn: Vec<_> = (16..20).map(|y| SubChunkKey::new(1, 0, y, 0)).collect();
    for key in spawn.iter().chain([&urgent, &stale]) {
        install_current_light(&mut stream, *key, 0, 0, false);
    }
    stream
        .mark_light_dirty_exact_with_priority(urgent, true)
        .unwrap();
    for key in &spawn {
        stream.mark_light_dirty_exact(*key).unwrap();
    }
    stream.lighting.jobs.scan.push_front((stale, u64::MAX));
    stream
        .lighting
        .jobs
        .ingress(view, Some(Instant::now()), |_, _, _| (0, true));
    let lane = &stream.lighting.jobs.lanes[0];
    assert_eq!(
        lane.ready.peek().map(|candidate| candidate.key),
        Some(urgent)
    );
}
