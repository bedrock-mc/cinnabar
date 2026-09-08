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
                .light_waiters
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
                .light_waiters
                .entry(unrelated_source)
                .or_default()
                .insert(unrelated_waiter);
        }
        let before = stream.light_waiters.clone();

        let probes = stream.remove_light_waiter_target(target);

        assert!(probes <= 6, "target removal probed {probes} waiter sources");
        let mut expected = before;
        for source in adjacent_sources {
            let waiters = expected.get_mut(&source).unwrap();
            waiters.remove(&target);
            if waiters.is_empty() {
                expected.remove(&source);
            }
        }
        assert_eq!(stream.light_waiters, expected);
    }

    let mut stream = lit_stream(0);
    let target = SubChunkKey::new(0, 0, 0, 0);
    for source in target.mesh_dependents().filter(|source| *source != target) {
        stream
            .light_waiters
            .entry(source)
            .or_default()
            .insert(target);
    }
    assert!(stream.remove_light_waiter_target(target) <= 6);
    assert!(stream.light_waiters.is_empty());
}

#[test]
fn equivalent_block_light_properties_skip_relighting() {
    let mut stream = lit_stream(0);
    let opaque = super::uniform_sub_chunk(2);
    let equivalent_opaque = super::uniform_sub_chunk(4);
    let air = super::uniform_sub_chunk(0);
    let zero_light_transparent = super::uniform_sub_chunk(3);
    assert!(!stream.block_light_semantics_changed(Some(&opaque), Some(&equivalent_opaque),));
    assert!(stream.block_light_semantics_changed(Some(&air), Some(&opaque)));
    assert!(stream.block_light_semantics_changed(Some(&air), Some(&zero_light_transparent),));
    assert!(stream.block_light_semantics_changed(Some(&zero_light_transparent), Some(&air),));
    assert!(stream.block_light_semantics_changed(None, Some(&air)));
    assert!(stream.block_light_semantics_changed(Some(&air), None));

    let key = SubChunkKey::new(0, 0, 0, 0);
    stream.mark_live_mutation_changed(key, Instant::now(), false);

    assert!(stream.pending_light.is_empty());
    assert!(stream.pending_mesh.values().all(|pending| pending.urgent));
}

#[test]
fn urgent_light_completion_keeps_follow_on_work_urgent() {
    let mut stream = lit_stream(0);
    let key = SubChunkKey::new(0, 0, 0, 0);
    let neighbour = SubChunkKey::new(0, 1, 0, 0);
    install_current_light(&mut stream, key, 0, 0, false);
    install_current_light(&mut stream, neighbour, 0, 0, false);
    let ordinary_revision = stream.mark_light_dirty_exact(neighbour).unwrap();
    let mut changed_faces = [false; 6];
    let changed_face = LIGHT_NEIGHBOUR_OFFSETS
        .iter()
        .position(|&offset| offset_sub_chunk_key(key, offset) == Some(neighbour))
        .unwrap();
    changed_faces[changed_face] = true;
    let direct_sky = stream.direct_sky[&key].clone();

    stream.finish_accepted_light_completion(key, 1, &direct_sky, changed_faces, true);

    assert!(stream.pending_light[&neighbour].urgent);
    assert_eq!(stream.pending_light[&neighbour].revision, ordinary_revision);
    assert_eq!(
        stream.light_priority_wakeups.get(&neighbour),
        Some(&ordinary_revision)
    );
}
#[test]
fn unchanged_uniform_light_completion_preserves_mesh_currentness_and_waiters() {
    let mut stream = lit_stream(1);
    let key = SubChunkKey::new(1, 0, 0, 0);
    let waiter = SubChunkKey::new(1, 2, 0, 0);
    stream
        .store
        .commit_sub_chunk(key, super::uniform_sub_chunk(2))
        .unwrap();
    install_current_light(&mut stream, key, 0, 0, false);
    install_current_light(&mut stream, waiter, 0, 0, false);
    let original_light = Arc::clone(stream.light_store.light(key).unwrap());
    let original_direct = Arc::clone(&stream.direct_sky[&key].mask);
    let original_ownership = stream.light_ownership[&key];

    let mesh_revision = stream.mark_dirty_exact(key, Instant::now());
    assert_eq!(stream.dispatch_mesh_jobs([0.0; 3], 1), 1);
    let mesh_completion = stream
        .mesh_rx
        .recv_timeout(Duration::from_secs(2))
        .expect("mesh worker completion");
    stream.light_waiters.entry(key).or_default().insert(waiter);
    stream.mark_light_dirty_exact(key).unwrap();
    complete_one_light(&mut stream, [0.0; 3]);

    assert!(Arc::ptr_eq(
        stream.light_store.light(key).unwrap(),
        &original_light
    ));
    assert!(Arc::ptr_eq(&stream.direct_sky[&key].mask, &original_direct));
    assert_eq!(stream.light_ownership[&key], original_ownership);
    assert!(!stream.pending_mesh.contains_key(&key));
    assert_eq!(
        stream.revisions.dirty(key).map(|dirty| dirty.revision),
        Some(mesh_revision)
    );
    assert_eq!(stream.in_flight.get(&key), Some(&mesh_revision));
    assert!(!stream.light_waiters.contains_key(&key));
    assert!(stream.pending_light.contains_key(&waiter));

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
fn light_column_batch_inherits_any_member_urgency() {
    let mut stream = lit_stream(1);
    let top = SubChunkKey::new(1, 0, 7, 0);
    let below = SubChunkKey::new(1, 0, 6, 0);
    for key in [top, below] {
        stream.record_known_air(key);
        install_current_light(&mut stream, key, 1, 0, false);
    }
    stream.mark_light_dirty_exact_with_priority(top, true);
    stream.mark_light_dirty_exact(below);

    assert_eq!(stream.dispatch_light_jobs([8.0, 120.0, 8.0], 1), 2);
    assert!(stream.in_flight_light[&top].urgent);
    assert!(stream.in_flight_light[&below].urgent);
    stream.mark_light_dirty_exact(below);
    assert!(stream.pending_light[&below].urgent);
}

#[test]
fn ordinary_mesh_redirty_inherits_in_flight_urgency() {
    let mut stream = lit_stream(0);
    let key = SubChunkKey::new(0, 0, 0, 0);
    stream.urgent_mesh_in_flight.insert(key);

    stream.mark_dirty_exact(key, Instant::now());

    assert!(stream.pending_mesh[&key].urgent);
}

#[test]
fn forced_remesh_inherits_queued_publication_urgency() {
    let mut stream = lit_stream(0);
    let key = SubChunkKey::new(0, 0, 0, 0);
    stream.mesh_changes.push_back(WorldMeshChange::Upsert {
        key,
        mesh: ChunkMesh::default(),
        biome: PackedBiomeRecord::fallback(),
        tint_identity: ChunkBiomeTintIdentity::default(),
        generation: 1,
        dirty_since: Instant::now(),
        urgent: true,
        permit: None,
    });

    stream.mark_forced_dirty_exact(key, Instant::now());

    assert!(stream.pending_mesh[&key].urgent);
}

#[test]
fn urgent_known_air_removal_uses_reserved_permit_after_ordinary_saturation() {
    let config = PublicationServiceConfig::PHASE2_GATE;
    let allowance = PublicationAllowance::new(config);
    allowance.begin_frame(
        1,
        config.maximum_zero_byte_operations_per_frame,
        0,
        config.maximum_zero_byte_operations_per_frame,
        config.maximum_frame_items,
    );
    let ordinary = (0..config.maximum_zero_byte_operations_per_frame)
        .map(|_| allowance.try_admit_zero_byte().unwrap())
        .collect::<Vec<_>>();
    allowance.begin_frame(2, 1, 0, 1, config.maximum_frame_items);

    let mut stream = lit_stream(0);
    stream.set_publication_allowance(allowance.clone());
    let key = SubChunkKey::new(0, 0, 0, 0);
    stream.record_known_air(key);
    stream.mark_dirty_exact_with_priority(key, Instant::now(), true);

    stream.dispatch_mesh_jobs([0.0; 3], 1);

    let change = stream
        .pop_mesh_change()
        .expect("urgent known-air removal uses the reserved permit");
    assert!(matches!(
        &change,
        WorldMeshChange::Remove {
            key: removed,
            urgent: true,
            permit: Some(_),
            ..
        } if *removed == key
    ));
    assert_eq!(allowance.remaining_zero_byte_operations(), 0);
    assert_eq!(config.maximum_zero_byte_operations_per_frame, 256);
    drop(change);
    drop(ordinary);
    assert_eq!(allowance.live_permits(), 0);
}

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
    let mut stalled_polls = 0_usize;
    while !stream.pending_light.is_empty() || !stream.in_flight_light.is_empty() {
        stream.dispatch_light_jobs([8.0, 80.0, 8.0], usize::MAX);
        if stream.in_flight_light.is_empty() {
            stalled_polls += 1;
            assert!(
                stalled_polls <= 256,
                "full-view lighting made no progress after bounded scheduler scans: \
                 pending={} ready={} deferred={} waiters={}",
                stream.pending_light.len(),
                stream.pending_light_ready.len(),
                stream.pending_light_deferred.len(),
                stream.light_waiters.len()
            );
            continue;
        }
        stalled_polls = 0;
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

    assert_eq!(stream.dispatch_light_jobs([8.0, 296.0, 8.0], 4), 2);
    assert!(stream.in_flight_light.contains_key(&top));
    assert!(stream.in_flight_light.contains_key(&below));
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
fn adjacent_light_jobs_converge_from_either_camera_priority() {
    for camera in [[0.0, 8.0, 8.0], [32.0, 8.0, 8.0]] {
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
        assert_eq!(stream.dispatch_light_jobs(camera, 2), 1);
        settle_light(&mut stream, camera);

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
fn face_adjacent_initial_light_jobs_are_dispatched_independently() {
    let mut stream = lit_stream(1);
    let left = SubChunkKey::new(1, 0, 0, 0);
    let right = SubChunkKey::new(1, 1, 0, 0);
    stream.record_known_air(left);
    stream.record_known_air(right);
    stream.mark_light_changed_sources([left, right]);

    assert!(stream.light_store.light(left).is_some());
    assert!(stream.light_store.light(right).is_some());
    assert!(!stream.light_ownership.contains_key(&left));
    assert!(!stream.light_ownership.contains_key(&right));
    assert_eq!(stream.dispatch_light_jobs([16.0, 8.0, 8.0], 2), 1);
    assert_eq!(stream.in_flight_light.len(), 1);
}

#[test]
fn mixed_vertical_batches_propagate_emission_into_air_above() {
    for (dimension, base_y, expected_sky) in [(1, 0, 0), (0, 17, 15)] {
        let mut stream = lit_stream(dimension);
        let emitter = SubChunkKey::new(dimension, 0, base_y, 0);
        let middle = SubChunkKey::new(dimension, 0, base_y + 1, 0);
        let top = SubChunkKey::new(dimension, 0, base_y + 2, 0);
        stream
            .store
            .commit_sub_chunk(emitter, super::uniform_sub_chunk(1))
            .unwrap();
        stream.resident.insert(emitter);
        stream.record_known_air(middle);
        stream.record_known_air(top);
        stream.mark_light_changed_sources([emitter, middle, top]);
        settle_light(&mut stream, [8.0, (base_y * 16 + 40) as f32, 8.0]);
        for key in [emitter, middle, top] {
            assert!(stream.light_is_current(key));
        }
        let light = stream.light_store.light(middle).unwrap();
        assert_eq!(
            light.get(LightChannel::Block, 0, 0, 0),
            Some(14),
            "dimension {dimension}"
        );
        assert_eq!(
            light.get(LightChannel::Sky, 0, 0, 0),
            Some(expected_sky),
            "dimension {dimension}"
        );
        assert!(stream.light_waiters.is_empty());
    }
}

#[test]
fn mutually_dependent_lit_air_regions_converge() {
    let mut stream = lit_stream(1);
    let emitter = SubChunkKey::new(1, 0, 0, 0);
    let air = [
        SubChunkKey::new(1, 1, 0, 0),
        SubChunkKey::new(1, 0, 0, 1),
        SubChunkKey::new(1, 1, 0, 1),
    ];
    stream
        .store
        .commit_sub_chunk(emitter, super::uniform_sub_chunk(1))
        .unwrap();
    stream.resident.insert(emitter);
    for key in air {
        stream.record_known_air(key);
    }
    stream.mark_light_changed_sources(std::iter::once(emitter).chain(air));
    let mut completions = 0;
    while !stream.pending_light.is_empty() || !stream.in_flight_light.is_empty() {
        stream.dispatch_light_jobs([24.0, 8.0, 24.0], usize::MAX);
        let completion = stream
            .light_rx
            .recv_timeout(Duration::from_secs(2))
            .expect("light dependencies must make progress");
        stream.accept_light_completion(completion);
        completions += 1;
        assert!(
            completions <= 128,
            "stationary light dependencies did not converge"
        );
    }
    for key in std::iter::once(emitter).chain(air) {
        assert!(stream.light_is_current(key));
    }
    assert!(stream.light_waiters.is_empty());
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
            let queued_at = stream.pending_light[&target].queued_at;
            stream.pending_light_scan.clear();
            stream
                .light_waiters
                .entry(source)
                .or_default()
                .insert(target);
            let direct_sky = stream.direct_sky[&source].clone();

            stream.finish_accepted_light_completion(
                source,
                1,
                &direct_sky,
                [false; 6],
                source_urgent,
            );

            assert_eq!(stream.pending_light[&target].revision, pending_revision);
            assert_eq!(stream.pending_light[&target].queued_at, queued_at);
            let effective_urgent = pending_urgent || source_urgent;
            assert_eq!(stream.pending_light[&target].urgent, effective_urgent);
            assert_eq!(
                stream.light_priority_wakeups.get(&target),
                Some(&pending_revision)
            );
            assert!(!stream.light_waiters.contains_key(&source));
            let queued = if effective_urgent {
                stream.pending_light_scan.front()
            } else {
                stream.pending_light_scan.back()
            };
            assert_eq!(queued, Some(&(target, pending_revision)));
            assert_eq!(stream.pending_light_scan.len(), 1);
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
        .light_rx
        .recv_timeout(Duration::from_secs(2))
        .expect("upper light completion");

    let below_revision = stream.mark_light_dirty_exact(below).unwrap();
    let below_queued_at = stream.pending_light[&below].queued_at;
    assert_eq!(stream.dispatch_light_jobs(camera, 1), 0);
    assert!(stream.light_waiters[&top].contains(&below));
    assert!(stream.pending_light_scan.is_empty());

    stream.accept_light_completion(top_completion);

    assert_eq!(stream.pending_light[&below].revision, below_revision);
    assert_eq!(stream.pending_light[&below].queued_at, below_queued_at);
    assert_eq!(stream.dispatch_light_jobs(camera, 1), 1);
    settle_light(&mut stream, camera);
    assert!(stream.light_is_current(top));
    assert!(stream.light_is_current(below));
    assert!(stream.pending_light.is_empty());
    assert!(stream.in_flight_light.is_empty());
    assert!(stream.light_waiters.is_empty());
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
        .light_rx
        .recv_timeout(Duration::from_secs(2))
        .expect("upper light completion");

    let below_revision = stream
        .mark_light_dirty_exact_with_priority(below, true)
        .unwrap();
    let below_queued_at = stream.pending_light[&below].queued_at;
    assert_eq!(stream.dispatch_light_jobs(camera, 1), 0);
    assert!(stream.light_waiters[&top].contains(&below));
    assert!(stream.pending_light_scan.is_empty());
    let ordinary_revision = stream.mark_light_dirty_exact(ordinary).unwrap();
    assert_eq!(
        stream.pending_light_scan.front(),
        Some(&(ordinary, ordinary_revision))
    );

    stream.accept_light_completion(top_completion);

    assert_eq!(stream.pending_light[&below].revision, below_revision);
    assert_eq!(stream.pending_light[&below].queued_at, below_queued_at);
    assert!(stream.pending_light[&below].urgent);
    assert_eq!(
        stream.pending_light_scan.front(),
        Some(&(below, below_revision))
    );
    assert_eq!(
        stream.pending_light_scan.back(),
        Some(&(ordinary, ordinary_revision))
    );
    assert_eq!(stream.dispatch_light_jobs(camera, 1), 1);
    assert!(stream.in_flight_light.contains_key(&below));
    settle_light(&mut stream, camera);
    assert!(
        [top, below, ordinary]
            .into_iter()
            .all(|key| stream.light_is_current(key))
    );
    assert!(stream.pending_light.is_empty());
    assert!(stream.in_flight_light.is_empty());
    assert!(stream.light_waiters.is_empty());
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
        block_generation: stream.block_generations[&target],
        previous_light_generation: stream
            .light_store
            .light(target)
            .map(|light| light.generation()),
        batch_id: 41,
        urgent: false,
    };
    stream.pending_light.remove(&target);
    stream.in_flight_light.insert(target, identity);
    stream
        .light_waiters
        .entry(source)
        .or_default()
        .insert(target);
    let direct_sky = stream.direct_sky[&source].clone();

    stream.finish_accepted_light_completion(source, 1, &direct_sky, [false; 6], true);

    assert_eq!(stream.in_flight_light.get(&target), Some(&identity));
    assert!(stream.pending_light[&target].revision > dispatched_revision);
    assert!(stream.pending_light[&target].urgent);
    assert!(
        !stream
            .light_revisions
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
        block_generation: stream.block_generations[&target],
        previous_light_generation: stream
            .light_store
            .light(target)
            .map(|light| light.generation()),
        batch_id: 42,
        urgent: false,
    };
    stream.pending_light.remove(&target);
    stream.in_flight_light.insert(target, identity);
    let replacement_revision = stream.mark_light_dirty_exact(target).unwrap();
    let replacement_queued_at = stream.pending_light[&target].queued_at;
    stream
        .light_waiters
        .entry(source)
        .or_default()
        .insert(target);
    let direct_sky = stream.direct_sky[&source].clone();

    stream.finish_accepted_light_completion(source, 1, &direct_sky, [false; 6], true);

    assert_eq!(stream.in_flight_light.get(&target), Some(&identity));
    assert_eq!(stream.pending_light[&target].revision, replacement_revision);
    assert_eq!(
        stream.pending_light[&target].queued_at,
        replacement_queued_at
    );
    assert!(stream.pending_light[&target].urgent);
    assert!(
        !stream
            .light_revisions
            .is_current(target, dispatched_revision)
    );
}

#[test]
fn staggered_emitter_region_converges_to_exact_light() {
    let mut stream = lit_stream(1);
    let mut keys = Vec::new();
    for chunk_x in -1..=1 {
        for chunk_z in -1..=1 {
            let key = SubChunkKey::new(1, chunk_x, 0, chunk_z);
            if chunk_x == 0 && chunk_z == 0 {
                stream
                    .store
                    .commit_sub_chunk(key, super::uniform_sub_chunk(1))
                    .unwrap();
                stream.resident.insert(key);
            } else {
                stream.record_known_air(key);
            }
            keys.push(key);
        }
    }
    stream.mark_light_changed_sources(keys.iter().copied());

    let mut completions = 0_usize;
    while !stream.pending_light.is_empty() || !stream.in_flight_light.is_empty() {
        stream.dispatch_light_jobs([8.0, 8.0, 8.0], 1);
        let completion = stream
            .light_rx
            .recv_timeout(Duration::from_secs(2))
            .expect("staggered light region must make progress");
        stream.accept_light_completion(completion);
        completions += 1;
        assert!(
            completions <= 128,
            "staggered light region did not converge"
        );
    }

    assert_eq!(stream.stats().stale_light_jobs, 0);
    assert!(stream.pending_light.is_empty());
    assert!(stream.in_flight_light.is_empty());
    assert!(stream.light_waiters.is_empty());
    for key in keys {
        assert!(stream.light_is_current(key));
        for x in 0_u8..16 {
            for z in 0_u8..16 {
                for y in 0_u8..16 {
                    let world_x = key.x * 16 + i32::from(x);
                    let world_z = key.z * 16 + i32::from(z);
                    let dx = if world_x < 0 {
                        -world_x
                    } else if world_x > 15 {
                        world_x - 15
                    } else {
                        0
                    };
                    let dz = if world_z < 0 {
                        -world_z
                    } else if world_z > 15 {
                        world_z - 15
                    } else {
                        0
                    };
                    let expected = u8::try_from(15_i32.saturating_sub(dx + dz).max(0)).unwrap();
                    let position = BlockPos::new(world_x, i32::from(y), world_z);
                    assert_eq!(
                        stream
                            .light_store
                            .light(key)
                            .unwrap()
                            .get(LightChannel::Block, x, y, z),
                        Some(expected),
                        "wrong block light at {position:?}"
                    );
                    assert_eq!(
                        stream
                            .light_store
                            .light(key)
                            .unwrap()
                            .get(LightChannel::Sky, x, y, z),
                        Some(0),
                        "wrong sky light at {position:?}"
                    );
                    assert!(!stream.direct_sky.get(&key).unwrap().mask.get(x, y, z));
                }
            }
        }
    }
}

#[test]
fn mixed_roof_and_air_columns_converge() {
    for roof_mask in [1_u8, 3, 5, 7] {
        let mut stream = lit_stream(0);
        let mut keys = Vec::new();
        for x in 0..2 {
            for z in 0..2 {
                for y in 17..=19 {
                    let key = SubChunkKey::new(0, x, y, z);
                    let roof = y == 19 && roof_mask & (1 << (x * 2 + z)) != 0;
                    if roof || (y == 18 && x == z) {
                        stream
                            .store
                            .commit_sub_chunk(
                                key,
                                super::uniform_sub_chunk(if roof { 2 } else { 3 }),
                            )
                            .unwrap();
                        stream.resident.insert(key);
                    } else {
                        stream.record_known_air(key);
                    }
                    keys.push(key);
                }
            }
        }
        stream.mark_light_changed_sources(keys.iter().copied());
        let mut completions = 0;
        while !stream.pending_light.is_empty() || !stream.in_flight_light.is_empty() {
            stream.dispatch_light_jobs([24.0, 280.0, 24.0], usize::MAX);
            let completion = stream
                .light_rx
                .recv_timeout(Duration::from_secs(2))
                .expect("roof light dependencies must make progress");
            stream.accept_light_completion(completion);
            completions += 1;
            assert!(completions <= 512, "roof mask {roof_mask} did not converge");
        }
        for key in keys {
            assert!(stream.light_is_current(key));
        }
    }
}

#[test]
fn adjacent_initial_emitter_and_air_converge_without_stale_completions() {
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

    let mut completions = 0_usize;
    while !stream.pending_light.is_empty() || !stream.in_flight_light.is_empty() {
        stream.dispatch_light_jobs([16.0, 8.0, 8.0], usize::MAX);
        let completion = stream
            .light_rx
            .recv_timeout(Duration::from_secs(2))
            .expect("adjacent initial light convergence");
        stream.accept_light_completion(completion);
        completions += 1;
        assert!(completions <= 3, "adjacent light convergence churned");
    }

    assert_eq!(stream.stats().stale_light_jobs, 0);
    assert_eq!(
        stream
            .light_store
            .light(air)
            .unwrap()
            .get(LightChannel::Block, 0, 0, 0),
        Some(14)
    );
}

#[test]
fn mid_flight_block_replacement_rejects_old_completion_and_preserves_adjacent_pending() {
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
    assert_eq!(stream.dispatch_light_jobs([16.0, 8.0, 8.0], 2), 1);
    let completion = stream
        .light_rx
        .recv_timeout(Duration::from_secs(2))
        .unwrap();

    stream
        .store
        .commit_sub_chunk(changed, super::uniform_sub_chunk(2))
        .unwrap();
    stream.mark_changed(changed, Instant::now());
    stream.accept_light_completion(completion);

    assert_eq!(stream.stats().stale_light_jobs, 1);
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
    assert_eq!(stream.dispatch_light_jobs([16.0, 8.0, 8.0], 2), 1);
    let completion = stream
        .light_rx
        .recv_timeout(Duration::from_secs(2))
        .unwrap();

    stream.evict_column(evicted.chunk());
    stream.accept_light_completion(completion);
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
