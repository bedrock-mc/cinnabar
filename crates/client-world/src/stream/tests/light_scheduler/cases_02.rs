use super::*;

#[test]
#[ignore = "release-only Phase 2 full-view lighting completion gate"]
fn release_full_view_known_air_lighting_completes_within_two_seconds() {
    let mut stream = lit_stream(0);
    let radius = super::super::PHASE0_MAX_VIEW_RADIUS_CHUNKS;
    let keys = (-radius..=radius)
        .flat_map(|x| {
            (-radius..=radius)
                .flat_map(move |z| (-4..20).map(move |y| SubChunkKey::new(0, x, y, z)))
        })
        .collect::<Vec<_>>();
    assert_eq!(keys.len(), 33 * 33 * 24);
    for key in &keys {
        stream.record_known_air(*key);
    }
    stream.mark_light_changed_sources(keys.iter().copied());

    let started = Instant::now();
    let mut completions = 0_usize;
    while !stream.pending_light.is_empty() || !stream.in_flight_light.is_empty() {
        stream.dispatch_light_jobs([8.0, 80.0, 8.0], usize::MAX);
        if stream.in_flight_light.is_empty() {
            panic!("full-view lighting made no progress without an in-flight solve");
        }
        let completion = stream
            .light_rx
            .recv_timeout(Duration::from_secs(5))
            .expect("full-view light completion");
        stream.accept_light_completion(completion);
        completions += 1;
        while let Ok(completion) = stream.light_rx.try_recv() {
            stream.accept_light_completion(completion);
            completions += 1;
        }
    }
    let elapsed = started.elapsed();
    eprintln!(
        "full-view light benchmark: resident={} completions={} fast_path={} stale={} elapsed_ms={}",
        keys.len(),
        completions,
        stream.stats().light_uniform_fast_path_jobs,
        stream.stats().stale_light_jobs,
        elapsed.as_millis()
    );

    assert!(stream.light_waiters.is_empty());
    assert!(keys.iter().all(|key| stream.light_is_current(*key)));
    assert_eq!(
        stream.stats().light_uniform_fast_path_jobs as usize,
        completions
    );
    assert!(
        elapsed <= Duration::from_secs(2),
        "completed full-view known-air lighting in {elapsed:?}, above the binding two-second gate"
    );
}

#[test]
fn overworld_initial_sky_work_waits_for_the_current_upper_subchunk() {
    let mut stream = lit_stream(0);
    let top = SubChunkKey::new(0, 0, 19, 0);
    let below = SubChunkKey::new(0, 0, 18, 0);
    stream.record_known_air(top);
    stream.record_known_air(below);
    stream.mark_light_changed_sources([top, below]);

    assert_eq!(stream.dispatch_light_jobs([8.0, 296.0, 8.0], 4), 1);
    assert!(stream.in_flight_light.contains_key(&top));
    assert!(!stream.in_flight_light.contains_key(&below));
}

#[test]
fn deterministic_solver_failure_terminalizes_only_that_generation() {
    let mut stream = lit_stream(1);
    let failed = SubChunkKey::new(1, 0, 0, 0);
    let waiter = SubChunkKey::new(1, 1, 0, 0);
    stream.record_known_air(failed);
    stream.record_known_air(waiter);
    stream.mark_light_changed_sources([failed, waiter]);
    assert_eq!(stream.dispatch_light_jobs([8.0; 3], 2), 2);

    let mut completions = (0..2)
        .map(|_| {
            stream
                .light_rx
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

    assert!(stream.in_flight_light.is_empty());
    assert!(stream.pending_light.is_empty());
    assert!(stream.light_waiters.is_empty());
    assert!(!stream.light_is_current(failed));
    assert!(!stream.light_is_current(waiter));
    assert!(!stream.light_ownership.contains_key(&waiter));
    assert_eq!(stream.dispatch_light_jobs([8.0; 3], usize::MAX), 0);
}

#[test]
fn redirtying_a_dependency_preserves_targets_waiting_for_it() {
    let mut stream = lit_stream(1);
    let dependency = SubChunkKey::new(1, 0, 0, 0);
    let target = SubChunkKey::new(1, 1, 0, 0);
    stream
        .store
        .commit_sub_chunk(dependency, super::uniform_sub_chunk(1))
        .unwrap();
    stream.resident.insert(dependency);
    stream.mark_changed(dependency, Instant::now());
    complete_one_light(&mut stream, [8.0, 8.0, 8.0]);
    stream.record_known_air(target);
    stream.mark_changed(target, Instant::now());
    assert_eq!(stream.dispatch_light_jobs([24.0, 8.0, 8.0], 1), 1);
    assert!(stream.light_waiters[&dependency].contains(&target));

    stream.mark_light_dirty_exact(dependency);

    assert!(stream.light_waiters[&dependency].contains(&target));
}

#[test]
fn adjacent_concurrent_completions_converge_in_both_acceptance_orders() {
    for reverse in [false, true] {
        let mut stream = lit_stream(1);
        let emitter = SubChunkKey::new(1, 0, 0, 0);
        let air = SubChunkKey::new(1, 1, 0, 0);
        stream
            .store
            .commit_sub_chunk(emitter, super::uniform_sub_chunk(1))
            .unwrap();
        stream.resident.insert(emitter);
        stream.record_known_air(air);
        stream.mark_light_changed_sources([emitter, air]);
        assert_eq!(stream.dispatch_light_jobs([16.0, 8.0, 8.0], 2), 2);

        let mut completions = (0..2)
            .map(|_| {
                stream
                    .light_rx
                    .recv_timeout(Duration::from_secs(2))
                    .unwrap()
            })
            .collect::<Vec<_>>();
        completions.sort_by_key(|completion| completion.key);
        if reverse {
            completions.reverse();
        }
        for completion in completions {
            stream.accept_light_completion(completion);
        }
        settle_light(&mut stream, [16.0, 8.0, 8.0]);

        assert_eq!(
            stream
                .light_store
                .light(air)
                .unwrap()
                .get(LightChannel::Block, 0, 0, 0),
            Some(14)
        );
        assert!(stream.light_is_current(emitter));
        assert!(stream.light_is_current(air));
        assert!(stream.light_waiters.is_empty());
    }
}

#[test]
fn mid_flight_block_replacement_rejects_both_adjacent_old_completions() {
    let mut stream = lit_stream(1);
    let changed = SubChunkKey::new(1, 0, 0, 0);
    let neighbour = SubChunkKey::new(1, 1, 0, 0);
    stream
        .store
        .commit_sub_chunk(changed, super::uniform_sub_chunk(1))
        .unwrap();
    stream.resident.insert(changed);
    stream.record_known_air(neighbour);
    stream.mark_light_changed_sources([changed, neighbour]);
    assert_eq!(stream.dispatch_light_jobs([16.0, 8.0, 8.0], 2), 2);
    let mut completions = (0..2)
        .map(|_| {
            stream
                .light_rx
                .recv_timeout(Duration::from_secs(2))
                .unwrap()
        })
        .collect::<Vec<_>>();

    stream
        .store
        .commit_sub_chunk(changed, super::uniform_sub_chunk(2))
        .unwrap();
    stream.mark_changed(changed, Instant::now());
    completions.sort_by_key(|completion| std::cmp::Reverse(completion.key));
    for completion in completions {
        stream.accept_light_completion(completion);
    }

    assert_eq!(stream.stats().stale_light_jobs, 2);
    assert!(!stream.light_ownership.contains_key(&changed));
    assert!(!stream.light_ownership.contains_key(&neighbour));
    assert!(stream.pending_light.contains_key(&changed));
    assert!(stream.pending_light.contains_key(&neighbour));
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
    assert_eq!(stream.dispatch_light_jobs([16.0, 8.0, 8.0], 2), 2);
    let completions = (0..2)
        .map(|_| {
            stream
                .light_rx
                .recv_timeout(Duration::from_secs(2))
                .unwrap()
        })
        .collect::<Vec<_>>();

    stream.evict_column(evicted.chunk());
    for completion in completions.into_iter().rev() {
        stream.accept_light_completion(completion);
    }
    settle_light(&mut stream, [24.0, 8.0, 8.0]);

    assert_eq!(
        stream.light_store.kind(evicted),
        world::LightSubChunkKind::Unknown
    );
    assert!(!stream.resident.contains(&evicted));
    assert!(stream.light_is_current(neighbour));
    assert!(stream.light_waiters.is_empty());
}

#[test]
fn eviction_purges_light_ownership_and_stale_completion_cannot_restore_it() {
    let mut stream = lit_stream(0);
    let key = SubChunkKey::new(0, 0, 0, 0);
    stream
        .store
        .commit_sub_chunk(key, super::uniform_sub_chunk(1))
        .unwrap();
    stream.resident.insert(key);
    stream.mark_changed(key, Instant::now());
    assert_eq!(stream.dispatch_light_jobs([8.0; 3], 1), 1);
    let completion = stream
        .light_rx
        .recv_timeout(Duration::from_secs(2))
        .unwrap();

    stream.evict_column(key.chunk());
    stream.accept_light_completion(completion);

    assert_eq!(
        stream.light_store.kind(key),
        world::LightSubChunkKind::Unknown
    );
    assert!(!stream.block_generations.contains_key(&key));
    assert!(!stream.light_ownership.contains_key(&key));
    assert!(!stream.direct_sky.contains_key(&key));
    assert!(!stream.pending_light.contains_key(&key));
    assert_eq!(stream.stats().stale_light_jobs, 1);
}
