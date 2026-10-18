use super::*;
use render_api::{PublicationAllowance, PublicationServiceConfig};

#[test]
fn urgent_scheduler_work_preempts_nearer_ordinary_work() {
    let camera = super::SchedulerView {
        position: [0.0; 3],
        forward: None,
        startup_center: None,
    };
    let near = SubChunkKey::new(0, 0, 0, 0);
    let far = SubChunkKey::new(0, 16, 0, 0);
    let mut candidates = BinaryHeap::from([
        PendingSchedulerCandidate::new(near, 1, camera, false),
        PendingSchedulerCandidate::new(far, 1, camera, true),
    ]);

    assert_eq!(candidates.pop().map(|candidate| candidate.key), Some(far));
}

#[test]
fn live_mutation_marks_light_and_mesh_work_urgent() {
    let mut stream = lit_stream(0);
    let key = SubChunkKey::new(0, 0, 0, 0);
    stream
        .authority
        .commit_sub_chunk(key, super::uniform_sub_chunk(1))
        .unwrap();
    install_current_light(&mut stream, key, 0, 0, false);

    stream.mark_live_mutation_changed(key, Instant::now(), true);

    assert!(stream.lighting.jobs.pending.values().all(|p| p.urgent));
    assert!(stream.mesh_jobs.pending.values().all(|p| p.urgent));
}

#[test]
fn light_outcome_counters_saturate() {
    let key = SubChunkKey::new(1, 0, 0, 0);

    let mut noop = lit_stream(1);
    install_current_light(&mut noop, key, 0, 0, false);
    noop.stats.accepted_light_jobs = u64::MAX;
    noop.stats.noop_light_jobs = u64::MAX;
    let completion = synthetic_light_completion(
        &mut noop,
        key,
        DirectSkyMask::Uniform(false),
        false,
        false,
        [false; 6],
    );
    noop.accept_light_completion(completion);
    assert_eq!(noop.stats().accepted_light_jobs, u64::MAX);
    assert_eq!(noop.stats().noop_light_jobs, u64::MAX);

    let mut provenance = lit_stream(1);
    install_current_light(&mut provenance, key, 0, 0, false);
    provenance.stats.accepted_light_jobs = u64::MAX;
    provenance.stats.provenance_only_light_jobs = u64::MAX;
    let completion = synthetic_light_completion(
        &mut provenance,
        key,
        DirectSkyMask::Uniform(true),
        false,
        true,
        [true; 6],
    );
    provenance.accept_light_completion(completion);
    assert_eq!(provenance.stats().accepted_light_jobs, u64::MAX);
    assert_eq!(provenance.stats().provenance_only_light_jobs, u64::MAX);

    let mut changed = lit_stream(1);
    install_current_light(&mut changed, key, 0, 0, false);
    changed.stats.accepted_light_jobs = u64::MAX;
    changed.stats.value_changed_light_jobs = u64::MAX;
    changed.stats.light_mesh_invalidations = u64::MAX;
    let completion = synthetic_light_completion(
        &mut changed,
        key,
        DirectSkyMask::Uniform(false),
        true,
        false,
        [true; 6],
    );
    changed.accept_light_completion(completion);
    assert_eq!(changed.stats().accepted_light_jobs, u64::MAX);
    assert_eq!(changed.stats().value_changed_light_jobs, u64::MAX);
    assert_eq!(changed.stats().light_mesh_invalidations, u64::MAX);
}

#[test]
fn light_dispatch_rotates_past_a_blocked_nearest_window() {
    let mut stream = lit_stream(0);
    for index in 0..MAX_PENDING_SCHEDULER_SCANS_PER_POLL {
        let key = SubChunkKey::new(0, index as i32 * 3, 0, 0);
        stream
            .authority
            .commit_sub_chunk(key, super::uniform_sub_chunk(1))
            .unwrap();
        install_current_light(&mut stream, key, 0, 0, false);
        stream.record_known_air(SubChunkKey::new(0, key.x, 1, key.z));
        stream.mark_light_dirty_exact(key).unwrap();
    }
    let farther = SubChunkKey::new(0, MAX_PENDING_SCHEDULER_SCANS_PER_POLL as i32 * 3, 0, 0);
    stream
        .authority
        .commit_sub_chunk(farther, super::uniform_sub_chunk(1))
        .unwrap();
    install_current_light(&mut stream, farther, 0, 0, false);
    let farther_revision = stream.mark_light_dirty_exact(farther).unwrap();

    assert_eq!(stream.dispatch_light_jobs([0.0; 3], 1), 0);
    for index in 0..MAX_PENDING_SCHEDULER_SCANS_PER_POLL {
        let key = SubChunkKey::new(0, index as i32 * 3, 0, 3);
        stream
            .authority
            .commit_sub_chunk(key, super::uniform_sub_chunk(1))
            .unwrap();
        install_current_light(&mut stream, key, 0, 0, false);
        stream.record_known_air(SubChunkKey::new(0, key.x, 1, key.z));
        stream.mark_light_dirty_exact(key).unwrap();
    }
    assert_eq!(stream.dispatch_light_jobs([16.0, 0.0, 0.0], 1), 1);
    assert_eq!(
        stream
            .lighting
            .jobs
            .in_flight
            .get(&farther)
            .map(|job| job.revision),
        Some(farther_revision)
    );
}

#[test]
fn mesh_dispatch_bounds_resident_readiness_scan_and_keeps_window_progressing() {
    let mut stream = stream();
    let blocked = (0..128)
        .map(|index| SubChunkKey::new(0, index * 3, 0, 0))
        .collect::<Vec<_>>();
    for key in blocked.iter().copied() {
        stream
            .authority
            .commit_sub_chunk(key, super::uniform_sub_chunk(1))
            .unwrap();
        install_current_light(&mut stream, key, 0, 0, false);
        stream.mark_light_dirty_exact(key);
        stream.mark_dirty_exact(key, Instant::now());
    }
    let farther = SubChunkKey::new(0, 128 * 3, 0, 0);
    stream
        .authority
        .commit_sub_chunk(farther, super::uniform_sub_chunk(1))
        .unwrap();
    install_current_light(&mut stream, farther, 0, 0, false);
    let farther_revision = stream.mark_dirty_exact(farther, Instant::now());

    assert_eq!(stream.dispatch_mesh_jobs([0.0; 3], 1), 0);
    assert!(!stream.mesh_jobs.in_flight.contains_key(&farther));
    assert!(stream.mesh_jobs.pending.contains_key(&farther));
    for index in 0..MAX_PENDING_SCHEDULER_SCANS_PER_POLL {
        let key = SubChunkKey::new(0, index as i32 * 3, 0, 3);
        stream
            .authority
            .commit_sub_chunk(key, super::uniform_sub_chunk(1))
            .unwrap();
        install_current_light(&mut stream, key, 0, 0, false);
        stream.mark_light_dirty_exact(key);
        stream.mark_dirty_exact(key, Instant::now());
    }

    assert_eq!(stream.dispatch_mesh_jobs([16.0, 0.0, 0.0], 1), 1);
    assert_eq!(
        stream.mesh_jobs.in_flight.get(&farther),
        Some(&farther_revision)
    );
}

#[test]
fn exhausted_removal_authority_still_reclassifies_ready_resident_work() {
    let mut stream = stream();
    let allowance = PublicationAllowance::new(PublicationServiceConfig::DEFAULT);
    allowance.begin_frame(
        1,
        0,
        0,
        0,
        PublicationServiceConfig::DEFAULT.maximum_frame_items,
    );
    stream.set_publication_allowance(allowance);

    let stale = SubChunkKey::new(0, 2, 0, 0);
    let resident = SubChunkKey::new(0, 1, 0, 0);
    let removal = SubChunkKey::new(0, 3, 0, 0);
    for key in [stale, resident, removal] {
        stream.mark_dirty_exact(key, Instant::now());
    }
    assert_eq!(stream.dispatch_mesh_jobs([0.0; 3], 1), 0);

    stream.mark_dirty_exact(stale, Instant::now());
    stream
        .authority
        .commit_sub_chunk(resident, super::uniform_sub_chunk(1))
        .unwrap();
    install_current_light(&mut stream, resident, 0, 0, false);
    stream.mark_dirty_exact(resident, Instant::now());
    let resident_revision = stream.mesh_jobs.pending[&resident].revision;

    assert_eq!(stream.dispatch_mesh_jobs([0.0; 3], 1), 1);
    assert_eq!(
        stream.mesh_jobs.in_flight.get(&resident),
        Some(&resident_revision)
    );
}

#[test]
fn mesh_worker_budget_keeps_unscanned_residents_behind_nearer_ready_work() {
    let mut stream = stream();
    let keys = (0..=MAX_PENDING_SCHEDULER_SCANS_PER_POLL)
        .map(|index| SubChunkKey::new(0, index as i32 * 3, 0, 0))
        .collect::<Vec<_>>();
    for key in keys.iter().copied() {
        stream
            .authority
            .commit_sub_chunk(key, super::uniform_sub_chunk(1))
            .unwrap();
        install_current_light(&mut stream, key, 0, 0, false);
        stream.mark_dirty_exact(key, Instant::now());
    }

    assert_eq!(stream.dispatch_mesh_jobs([0.0; 3], 1), 1);
    assert!(stream.mesh_jobs.in_flight.contains_key(&keys[0]));
    stream.mesh_jobs.in_flight.remove(&keys[0]);

    assert_eq!(stream.dispatch_mesh_jobs([0.0; 3], 1), 1);
    assert!(stream.mesh_jobs.in_flight.contains_key(&keys[1]));
    assert!(
        !stream
            .mesh_jobs
            .in_flight
            .contains_key(keys.last().unwrap())
    );
}

#[test]
fn renewed_removal_authority_restores_nearest_first_order() {
    let mut stream = stream();
    let keys = (0..3)
        .map(|x| SubChunkKey::new(0, x, 0, 0))
        .collect::<Vec<_>>();
    for key in keys.iter().copied() {
        stream.mark_dirty_exact(key, Instant::now());
    }

    for expected in keys {
        assert_eq!(stream.dispatch_mesh_jobs([0.0; 3], 1), 0);
        assert_eq!(stream.pop_mesh_change().unwrap().key(), expected);
    }
}

#[test]
fn runtime_light_metadata_propagates_across_a_known_air_seam() {
    let mut stream = lit_stream(0);
    let emitter = SubChunkKey::new(0, -1, 0, 0);
    stream
        .authority
        .commit_sub_chunk(emitter, super::uniform_sub_chunk(1))
        .unwrap();
    stream.resident.insert(emitter);
    stream.mark_changed(emitter, Instant::now());
    complete_one_light(&mut stream, [-8.0, 8.0, 8.0]);
    assert!(stream.light_is_current(emitter));

    let air = SubChunkKey::new(0, 0, 0, 0);
    stream.record_known_air(air);
    stream.mark_changed(air, Instant::now());
    complete_one_light(&mut stream, [-8.0, 8.0, 8.0]);
    complete_one_light(&mut stream, [8.0, 8.0, 8.0]);

    assert_eq!(
        stream
            .lighting
            .store
            .light(air)
            .unwrap()
            .get(LightChannel::Block, 0, 0, 0),
        Some(14)
    );
    assert!(stream.light_is_current(air));
}

#[test]
fn retained_column_batch_does_not_requeue_accepted_members() {
    let mut stream = lit_stream(1);
    let top = SubChunkKey::new(1, 0, 7, 0);
    let below = SubChunkKey::new(1, 0, 6, 0);
    for key in [top, below] {
        stream.record_known_air(key);
        install_current_light(&mut stream, key, 1, 0, false);
        stream.mark_light_dirty_exact(key);
    }

    assert_eq!(stream.dispatch_light_jobs([8.0, 120.0, 8.0], 1), 2);
    assert!(stream.lighting.waiters.is_empty());
    for _ in 0..2 {
        let completion = stream
            .lighting
            .rx
            .recv_timeout(Duration::from_secs(2))
            .unwrap();
        stream.accept_light_completion(completion);
    }

    assert!(stream.lighting.jobs.pending.is_empty());
    assert!(stream.lighting.jobs.in_flight.is_empty());
    assert!(stream.light_is_current(top));
    assert!(stream.light_is_current(below));
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

    assert!(stream.lighting.jobs.pending.is_empty());
    assert!(
        stream
            .mesh_jobs
            .pending
            .values()
            .all(|pending| pending.urgent)
    );
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
    let direct_sky = stream.lighting.direct_sky[&key].clone();

    stream.finish_accepted_light_completion(key, 1, &direct_sky, changed_faces, true);

    assert!(stream.lighting.jobs.pending[&neighbour].urgent);
    assert_eq!(
        stream.lighting.jobs.pending[&neighbour].revision,
        ordinary_revision
    );
    assert_eq!(
        stream.lighting.priority_wakeups.get(&neighbour),
        Some(&ordinary_revision)
    );
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
    assert!(stream.lighting.jobs.in_flight[&top].urgent);
    assert!(stream.lighting.jobs.in_flight[&below].urgent);
    stream.mark_light_dirty_exact(below);
    assert!(stream.lighting.jobs.pending[&below].urgent);
}

#[test]
fn ordinary_mesh_redirty_inherits_in_flight_urgency() {
    let mut stream = lit_stream(0);
    let key = SubChunkKey::new(0, 0, 0, 0);
    stream.urgent_mesh_in_flight.insert(key);

    stream.mark_dirty_exact(key, Instant::now());

    assert!(stream.mesh_jobs.pending[&key].urgent);
}

#[test]
fn forced_remesh_inherits_queued_publication_urgency() {
    let mut stream = lit_stream(0);
    let key = SubChunkKey::new(0, 0, 0, 0);
    stream.mesh_changes.push(WorldMeshChange::Upsert {
        output_permit: None,
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

    assert!(stream.mesh_jobs.pending[&key].urgent);
}

#[test]
fn urgent_known_air_removal_uses_reserved_permit_after_ordinary_saturation() {
    let config = PublicationServiceConfig::DEFAULT;
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
#[ignore = "release-only publication full-view lighting completion gate"]
fn release_full_view_known_air_lighting_completes_within_two_seconds() {
    let mut stream = lit_stream(0);
    let radius = super::super::MAX_VIEW_RADIUS_CHUNKS;
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
    while !stream.lighting.jobs.pending.is_empty() || !stream.lighting.jobs.in_flight.is_empty() {
        stream.dispatch_light_jobs([8.0, 80.0, 8.0], usize::MAX);
        if stream.lighting.jobs.in_flight.is_empty() {
            stalled_polls += 1;
            assert!(
                stalled_polls <= 256,
                "full-view lighting made no progress after bounded scheduler scans: \
                 pending={} ready={} deferred={} waiters={}",
                stream.lighting.jobs.pending.len(),
                stream.lighting.jobs.lanes[0].ready.len(),
                stream.lighting.jobs.lanes[0].deferred.len(),
                stream.lighting.waiters.len()
            );
            continue;
        }
        stalled_polls = 0;
        let completion = stream
            .lighting
            .rx
            .recv_timeout(Duration::from_secs(5))
            .expect("full-view light completion");
        stream.accept_light_completion(completion);
        completions += 1;
        while let Ok(completion) = stream.lighting.rx.try_recv() {
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

    assert!(stream.lighting.waiters.is_empty());
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
fn face_adjacent_initial_light_jobs_are_dispatched_independently() {
    let mut stream = lit_stream(1);
    let left = SubChunkKey::new(1, 0, 0, 0);
    let right = SubChunkKey::new(1, 1, 0, 0);
    stream.record_known_air(left);
    stream.record_known_air(right);
    stream.mark_light_changed_sources([left, right]);

    assert!(stream.lighting.store.light(left).is_some());
    assert!(stream.lighting.store.light(right).is_some());
    assert!(!stream.lighting.ownership.contains_key(&left));
    assert!(!stream.lighting.ownership.contains_key(&right));
    assert_eq!(stream.dispatch_light_jobs([16.0, 8.0, 8.0], 2), 1);
    assert_eq!(stream.lighting.jobs.in_flight.len(), 1);
}
