use super::*;

#[test]
fn removing_waiter_target_has_face_bounded_work_and_exact_graph_effect() {
    for target in [
        SubChunkKey::new(1, 10, 20, 30),
        SubChunkKey::new(1, i32::MIN, i32::MAX, i32::MIN),
    ] {
        let mut stream = lit_stream(target.dimension);
        let adjacent_sources = target
            .mesh_dependents()
            .filter(|source| *source != target)
            .collect::<BTreeSet<_>>();
        for source in &adjacent_sources {
            let retained_waiter = source
                .mesh_dependents()
                .find(|waiter| *waiter != *source && *waiter != target)
                .unwrap();
            stream
                .lighting
                .waiters
                .entry(*source)
                .or_default()
                .extend([target, retained_waiter]);
        }
        for offset in 0..256 {
            let unrelated_source = SubChunkKey::new(
                2,
                offset,
                offset.saturating_mul(2),
                offset.saturating_mul(3),
            );
            let unrelated_waiter =
                SubChunkKey::new(2, offset.saturating_add(1), offset * 2, offset * 3);
            stream
                .lighting
                .waiters
                .entry(unrelated_source)
                .or_default()
                .insert(unrelated_waiter);
        }
        let before = stream.lighting.waiters.clone();

        let probes = stream.lighting.remove_waiter_target(target);

        assert!(probes <= 6, "target removal probed {probes} waiter sources");
        let mut expected = before;
        for source in adjacent_sources {
            let waiters = expected.get_mut(&source).unwrap();
            waiters.remove(&target);
            if waiters.is_empty() {
                expected.remove(&source);
            }
        }
        assert_eq!(stream.lighting.waiters, expected);
    }

    let mut stream = lit_stream(0);
    let target = SubChunkKey::new(0, 0, 0, 0);
    for source in target.mesh_dependents().filter(|source| *source != target) {
        stream
            .lighting
            .waiters
            .entry(source)
            .or_default()
            .insert(target);
    }
    assert!(stream.lighting.remove_waiter_target(target) <= 6);
    assert!(stream.lighting.waiters.is_empty());
}

#[test]
fn unchanged_uniform_light_completion_preserves_mesh_currentness_and_waiters() {
    let mut stream = lit_stream(1);
    let key = SubChunkKey::new(1, 0, 0, 0);
    let waiter = SubChunkKey::new(1, 2, 0, 0);
    stream
        .authority
        .commit_sub_chunk(key, super::uniform_sub_chunk(2))
        .unwrap();
    install_current_light(&mut stream, key, 0, 0, false);
    install_current_light(&mut stream, waiter, 0, 0, false);
    let original_light = Arc::clone(stream.lighting.store.light(key).unwrap());
    let original_direct = Arc::clone(&stream.lighting.direct_sky[&key].mask);
    let original_ownership = stream.lighting.ownership[&key];

    let mesh_revision = stream.mark_dirty_exact(key, Instant::now());
    assert_eq!(stream.dispatch_mesh_jobs([0.0; 3], 1), 1);
    let mesh_completion = stream
        .mesh_rx
        .recv_timeout(Duration::from_secs(2))
        .expect("mesh worker completion");
    stream
        .lighting
        .waiters
        .entry(key)
        .or_default()
        .insert(waiter);
    stream.mark_light_dirty_exact(key).unwrap();
    complete_one_light(&mut stream, [0.0; 3]);

    assert!(Arc::ptr_eq(
        stream.lighting.store.light(key).unwrap(),
        &original_light
    ));
    assert!(Arc::ptr_eq(
        &stream.lighting.direct_sky[&key].mask,
        &original_direct
    ));
    assert_eq!(stream.lighting.ownership[&key], original_ownership);
    assert!(!stream.mesh_jobs.pending.contains_key(&key));
    assert_eq!(
        stream.revisions.dirty(key).map(|dirty| dirty.revision),
        Some(mesh_revision)
    );
    assert_eq!(stream.mesh_jobs.in_flight.get(&key), Some(&mesh_revision));
    assert!(!stream.lighting.waiters.contains_key(&key));
    assert!(stream.lighting.jobs.pending.contains_key(&waiter));

    stream.accept_mesh_completion(mesh_completion);
    assert_eq!(stream.stats().stale_mesh_jobs, 0);
    assert_eq!(stream.take_mesh_changes().len(), 1);
    assert_eq!(stream.stats().accepted_light_jobs, 1);
    assert_eq!(stream.stats().noop_light_jobs, 1);
    assert_eq!(stream.stats().value_changed_light_jobs, 0);
    assert_eq!(stream.stats().provenance_only_light_jobs, 0);
    assert_eq!(stream.stats().light_mesh_invalidations, 0);
}

#[test]
fn deterministic_solver_failure_terminalizes_only_that_generation() {
    let mut stream = lit_stream(1);
    let failed = SubChunkKey::new(1, 0, 0, 0);
    let waiter = SubChunkKey::new(1, 2, 0, 0);
    stream.record_known_air(failed);
    stream.record_known_air(waiter);
    stream.mark_light_changed_sources([failed, waiter]);
    assert_eq!(stream.dispatch_light_jobs([8.0; 3], 2), 2);

    let mut completions = (0..2)
        .map(|_| {
            stream
                .lighting
                .rx
                .recv_timeout(Duration::from_secs(2))
                .unwrap()
        })
        .collect::<Vec<_>>();
    let failed_completion = completions
        .iter_mut()
        .find(|completion| completion.key == failed)
        .unwrap();
    failed_completion.result = Err(super::super::LightJobError::Solve(
        LightSolveError::QueueLimitExceeded { max: 0 },
    ));
    let failed_index = completions
        .iter()
        .position(|completion| completion.key == failed)
        .unwrap();
    let failed_completion = completions.swap_remove(failed_index);
    stream.accept_light_completion(failed_completion);

    assert_eq!(
        stream.take_fatal_error(),
        Some(super::super::WorldStreamFatalError::LightSolve {
            key: failed,
            error: LightSolveError::QueueLimitExceeded { max: 0 },
        })
    );
    stream.accept_light_completion(completions.pop().unwrap());

    assert!(stream.lighting.jobs.in_flight.is_empty());
    assert!(stream.lighting.jobs.pending.is_empty());
    assert!(stream.lighting.waiters.is_empty());
    assert!(!stream.light_is_current(failed));
    assert!(!stream.light_is_current(waiter));
    assert!(!stream.lighting.ownership.contains_key(&waiter));
    assert_eq!(stream.dispatch_light_jobs([8.0; 3], usize::MAX), 0);
}

#[test]
fn redirtying_a_dependency_preserves_targets_waiting_for_it() {
    let mut stream = lit_stream(1);
    let dependency = SubChunkKey::new(1, 0, 0, 0);
    let target = SubChunkKey::new(1, 1, 0, 0);
    stream
        .authority
        .commit_sub_chunk(dependency, super::uniform_sub_chunk(1))
        .unwrap();
    stream.resident.insert(dependency);
    stream.mark_changed(dependency, Instant::now());
    complete_one_light(&mut stream, [8.0, 8.0, 8.0]);
    stream.record_known_air(target);
    stream.mark_changed(target, Instant::now());
    assert_eq!(stream.dispatch_light_jobs([24.0, 8.0, 8.0], 1), 1);
    assert!(stream.lighting.waiters[&dependency].contains(&target));

    stream.mark_light_dirty_exact(dependency);

    assert!(stream.lighting.waiters[&dependency].contains(&target));
}

#[test]
fn settled_dependency_wakeup_preserves_an_already_pending_target_revision() {
    for pending_urgent in [false, true] {
        for source_urgent in [false, true] {
            let mut stream = lit_stream(1);
            let source = SubChunkKey::new(1, 0, 0, 0);
            let target = SubChunkKey::new(1, 1, 0, 0);
            install_current_light(&mut stream, source, 15, 0, false);
            install_current_light(&mut stream, target, 14, 0, false);
            let pending_revision = stream
                .mark_light_dirty_exact_with_priority(target, pending_urgent)
                .unwrap();
            let queued_at = stream.lighting.jobs.pending[&target].queued_at;
            stream.lighting.jobs.scan.clear();
            stream
                .lighting
                .waiters
                .entry(source)
                .or_default()
                .insert(target);
            let direct_sky = stream.lighting.direct_sky[&source].clone();

            stream.finish_accepted_light_completion(
                source,
                1,
                &direct_sky,
                [false; 6],
                source_urgent,
            );

            assert_eq!(
                stream.lighting.jobs.pending[&target].revision,
                pending_revision
            );
            assert_eq!(stream.lighting.jobs.pending[&target].queued_at, queued_at);
            let effective_urgent = pending_urgent || source_urgent;
            assert_eq!(
                stream.lighting.jobs.pending[&target].urgent,
                effective_urgent
            );
            assert_eq!(
                stream.lighting.priority_wakeups.get(&target),
                Some(&pending_revision)
            );
            assert!(!stream.lighting.waiters.contains_key(&source));
            let queued = if effective_urgent {
                stream.lighting.jobs.scan.front()
            } else {
                stream.lighting.jobs.scan.back()
            };
            assert_eq!(queued, Some(&(target, pending_revision)));
            assert_eq!(stream.lighting.jobs.scan.len(), 1);
        }
    }
}

#[test]
fn ordinary_dependency_wakeup_requeues_a_consumed_pending_candidate() {
    let mut stream = lit_stream(0);
    let top = SubChunkKey::new(0, 0, 19, 0);
    let below = SubChunkKey::new(0, 0, 18, 0);
    install_current_light(&mut stream, top, 0, 0, false);
    install_current_light(&mut stream, below, 0, 0, false);
    stream.mark_light_dirty_exact(top).unwrap();
    let camera = [8.0, 296.0, 8.0];
    assert_eq!(stream.dispatch_light_jobs(camera, 1), 1);
    let top_completion = stream
        .lighting
        .rx
        .recv_timeout(Duration::from_secs(2))
        .expect("upper light completion");

    let below_revision = stream.mark_light_dirty_exact(below).unwrap();
    let below_queued_at = stream.lighting.jobs.pending[&below].queued_at;
    assert_eq!(stream.dispatch_light_jobs(camera, 1), 0);
    assert!(stream.lighting.waiters[&top].contains(&below));
    assert!(stream.lighting.jobs.scan.is_empty());

    stream.accept_light_completion(top_completion);

    assert_eq!(
        stream.lighting.jobs.pending[&below].revision,
        below_revision
    );
    assert_eq!(
        stream.lighting.jobs.pending[&below].queued_at,
        below_queued_at
    );
    assert_eq!(stream.dispatch_light_jobs(camera, 1), 1);
    settle_light(&mut stream, camera);
    assert!(stream.light_is_current(top));
    assert!(stream.light_is_current(below));
    assert!(stream.lighting.jobs.pending.is_empty());
    assert!(stream.lighting.jobs.in_flight.is_empty());
    assert!(stream.lighting.waiters.is_empty());
}

#[test]
fn ordinary_dependency_wakeup_restores_a_consumed_urgent_candidate_to_the_front() {
    let mut stream = lit_stream(0);
    let top = SubChunkKey::new(0, 0, 19, 0);
    let below = SubChunkKey::new(0, 0, 18, 0);
    let ordinary = SubChunkKey::new(0, 2, 19, 0);
    for key in [top, below, ordinary] {
        install_current_light(&mut stream, key, 0, 0, false);
    }
    stream.mark_light_dirty_exact(top).unwrap();
    let camera = [8.0, 296.0, 8.0];
    assert_eq!(stream.dispatch_light_jobs(camera, 1), 1);
    let top_completion = stream
        .lighting
        .rx
        .recv_timeout(Duration::from_secs(2))
        .expect("upper light completion");

    let below_revision = stream
        .mark_light_dirty_exact_with_priority(below, true)
        .unwrap();
    let below_queued_at = stream.lighting.jobs.pending[&below].queued_at;
    assert_eq!(stream.dispatch_light_jobs(camera, 1), 0);
    assert!(stream.lighting.waiters[&top].contains(&below));
    assert!(stream.lighting.jobs.scan.is_empty());
    let ordinary_revision = stream.mark_light_dirty_exact(ordinary).unwrap();
    assert_eq!(
        stream.lighting.jobs.scan.front(),
        Some(&(ordinary, ordinary_revision))
    );

    stream.accept_light_completion(top_completion);

    assert_eq!(
        stream.lighting.jobs.pending[&below].revision,
        below_revision
    );
    assert_eq!(
        stream.lighting.jobs.pending[&below].queued_at,
        below_queued_at
    );
    assert!(stream.lighting.jobs.pending[&below].urgent);
    assert_eq!(
        stream.lighting.jobs.scan.front(),
        Some(&(below, below_revision))
    );
    assert_eq!(
        stream.lighting.jobs.scan.back(),
        Some(&(ordinary, ordinary_revision))
    );
    assert_eq!(stream.dispatch_light_jobs(camera, 1), 1);
    assert!(stream.lighting.jobs.in_flight.contains_key(&below));
    settle_light(&mut stream, camera);
    assert!(
        [top, below, ordinary]
            .into_iter()
            .all(|key| stream.light_is_current(key))
    );
    assert!(stream.lighting.jobs.pending.is_empty());
    assert!(stream.lighting.jobs.in_flight.is_empty());
    assert!(stream.lighting.waiters.is_empty());
}

#[test]
fn settled_dependency_wakeup_still_invalidates_an_in_flight_target() {
    let mut stream = lit_stream(1);
    let source = SubChunkKey::new(1, 0, 0, 0);
    let target = SubChunkKey::new(1, 1, 0, 0);
    install_current_light(&mut stream, source, 15, 0, false);
    install_current_light(&mut stream, target, 14, 0, false);
    let dispatched_revision = stream.mark_light_dirty_exact(target).unwrap();
    let identity = LightJobIdentity {
        revision: dispatched_revision,
        block_generation: stream.lighting.block_generations[&target],
        previous_light_generation: stream
            .lighting
            .store
            .light(target)
            .map(|light| light.generation()),
        batch_id: 41,
        urgent: false,
    };
    stream.lighting.jobs.pending.remove(&target);
    stream.lighting.jobs.in_flight.insert(target, identity);
    stream
        .lighting
        .waiters
        .entry(source)
        .or_default()
        .insert(target);
    let direct_sky = stream.lighting.direct_sky[&source].clone();

    stream.finish_accepted_light_completion(source, 1, &direct_sky, [false; 6], true);

    assert_eq!(stream.lighting.jobs.in_flight.get(&target), Some(&identity));
    assert!(stream.lighting.jobs.pending[&target].revision > dispatched_revision);
    assert!(stream.lighting.jobs.pending[&target].urgent);
    assert!(
        !stream
            .lighting
            .revisions
            .is_current(target, dispatched_revision)
    );
}

#[test]
fn settled_dependency_wakeup_preserves_existing_in_flight_replacement() {
    let mut stream = lit_stream(1);
    let source = SubChunkKey::new(1, 0, 0, 0);
    let target = SubChunkKey::new(1, 1, 0, 0);
    install_current_light(&mut stream, source, 15, 0, false);
    install_current_light(&mut stream, target, 14, 0, false);
    let dispatched_revision = stream.mark_light_dirty_exact(target).unwrap();
    let identity = LightJobIdentity {
        revision: dispatched_revision,
        block_generation: stream.lighting.block_generations[&target],
        previous_light_generation: stream
            .lighting
            .store
            .light(target)
            .map(|light| light.generation()),
        batch_id: 42,
        urgent: false,
    };
    stream.lighting.jobs.pending.remove(&target);
    stream.lighting.jobs.in_flight.insert(target, identity);
    let replacement_revision = stream.mark_light_dirty_exact(target).unwrap();
    let replacement_queued_at = stream.lighting.jobs.pending[&target].queued_at;
    stream
        .lighting
        .waiters
        .entry(source)
        .or_default()
        .insert(target);
    let direct_sky = stream.lighting.direct_sky[&source].clone();

    stream.finish_accepted_light_completion(source, 1, &direct_sky, [false; 6], true);

    assert_eq!(stream.lighting.jobs.in_flight.get(&target), Some(&identity));
    assert_eq!(
        stream.lighting.jobs.pending[&target].revision,
        replacement_revision
    );
    assert_eq!(
        stream.lighting.jobs.pending[&target].queued_at,
        replacement_queued_at
    );
    assert!(stream.lighting.jobs.pending[&target].urgent);
    assert!(
        !stream
            .lighting
            .revisions
            .is_current(target, dispatched_revision)
    );
}

#[test]
fn mid_flight_block_replacement_rejects_old_completion_and_preserves_adjacent_pending() {
    let mut stream = lit_stream(1);
    let changed = SubChunkKey::new(1, 0, 0, 0);
    let neighbour = SubChunkKey::new(1, 1, 0, 0);
    stream
        .authority
        .commit_sub_chunk(changed, super::uniform_sub_chunk(1))
        .unwrap();
    stream.resident.insert(changed);
    stream.record_known_air(neighbour);
    stream.mark_light_changed_sources([changed, neighbour]);
    assert_eq!(stream.dispatch_light_jobs([16.0, 8.0, 8.0], 2), 1);
    let completion = stream
        .lighting
        .rx
        .recv_timeout(Duration::from_secs(2))
        .unwrap();

    stream
        .authority
        .commit_sub_chunk(changed, super::uniform_sub_chunk(2))
        .unwrap();
    stream.mark_changed(changed, Instant::now());
    stream.accept_light_completion(completion);

    assert_eq!(stream.stats().stale_light_jobs, 1);
    assert!(!stream.lighting.ownership.contains_key(&changed));
    assert!(!stream.lighting.ownership.contains_key(&neighbour));
    assert!(stream.lighting.jobs.pending.contains_key(&changed));
    assert!(stream.lighting.jobs.pending.contains_key(&neighbour));
    settle_light(&mut stream, [16.0, 8.0, 8.0]);
    assert!(stream.light_is_current(changed));
    assert!(stream.light_is_current(neighbour));
}

#[test]
fn mid_flight_eviction_cannot_restore_source_or_strand_neighbour_waiters() {
    let mut stream = lit_stream(1);
    let evicted = SubChunkKey::new(1, 0, 0, 0);
    let neighbour = SubChunkKey::new(1, 1, 0, 0);
    stream.record_known_air(evicted);
    stream.record_known_air(neighbour);
    stream.mark_light_changed_sources([evicted, neighbour]);
    assert_eq!(stream.dispatch_light_jobs([16.0, 8.0, 8.0], 2), 1);
    let completion = stream
        .lighting
        .rx
        .recv_timeout(Duration::from_secs(2))
        .unwrap();

    stream.evict_column(evicted.chunk());
    stream.accept_light_completion(completion);
    settle_light(&mut stream, [24.0, 8.0, 8.0]);

    assert_eq!(
        stream.lighting.store.kind(evicted),
        world::LightSubChunkKind::Unknown
    );
    assert!(!stream.resident.contains(&evicted));
    assert!(stream.light_is_current(neighbour));
    assert!(stream.lighting.waiters.is_empty());
}

#[test]
fn eviction_purges_light_ownership_and_stale_completion_cannot_restore_it() {
    let mut stream = lit_stream(0);
    let key = SubChunkKey::new(0, 0, 0, 0);
    stream
        .authority
        .commit_sub_chunk(key, super::uniform_sub_chunk(1))
        .unwrap();
    stream.resident.insert(key);
    stream.mark_changed(key, Instant::now());
    assert_eq!(stream.dispatch_light_jobs([8.0; 3], 1), 1);
    let completion = stream
        .lighting
        .rx
        .recv_timeout(Duration::from_secs(2))
        .unwrap();

    stream.evict_column(key.chunk());
    stream.accept_light_completion(completion);

    assert_eq!(
        stream.lighting.store.kind(key),
        world::LightSubChunkKind::Unknown
    );
    assert!(!stream.lighting.block_generations.contains_key(&key));
    assert!(!stream.lighting.ownership.contains_key(&key));
    assert!(!stream.lighting.direct_sky.contains_key(&key));
    assert!(!stream.lighting.jobs.pending.contains_key(&key));
    assert_eq!(stream.stats().stale_light_jobs, 1);
}
