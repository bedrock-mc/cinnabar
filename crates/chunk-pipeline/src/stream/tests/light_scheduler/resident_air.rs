use super::*;

fn resident(stream: &mut WorldStream, key: SubChunkKey, chunk: world::SubChunk) {
    stream.authority.commit_sub_chunk(key, chunk).unwrap();
    stream.sync_resident(key);
}

#[test]
fn resident_air_publishes_removal_without_worker_credit_or_ready_light_halo() {
    let mut stream = lit_stream(0);
    let key = SubChunkKey::new(0, 0, 5, 0);
    let neighbour = SubChunkKey::new(0, 1, 5, 0);
    resident(&mut stream, key, super::uniform_sub_chunk(0));
    install_current_light(&mut stream, neighbour, 0, 15, true);
    stream.mark_light_dirty_exact(neighbour).unwrap();
    let light_pending = stream.lighting.jobs.pending.len();
    let source = stream.authority.terrain().sub_chunk(key).unwrap();
    let generation = stream.mark_dirty_exact(key, Instant::now());
    assert!(stream.mesh_light_halo(key).is_none());
    assert_eq!(
        stream.dispatch_mesh_jobs_with_limits([8.0, 80.0, 8.0], 0, 1),
        0
    );
    let change = stream
        .pop_mesh_change()
        .expect("proven air publishes its removal directly");
    let WorldMeshChange::Remove {
        key: removed,
        generation: actual,
        dirty_since,
        ..
    } = change
    else {
        panic!("proven air used the geometry worker path");
    };
    assert_eq!((removed, actual), (key, generation));
    assert_eq!(stream.admitted_mesh_jobs.load(Ordering::Acquire), 0);
    assert_eq!(stream.mesh_memory.retained.load(Ordering::Acquire), 0);
    assert!(stream.mesh_jobs.in_flight.is_empty());
    assert!(stream.mesh_rx.is_empty());
    assert_eq!(stream.stats.publication_stages.mesh_jobs_dispatched, 0);
    assert!(stream.resident.contains(&key));
    assert!(!stream.known_air.contains(&key));
    assert!(Arc::ptr_eq(
        &source,
        &stream.authority.terrain().sub_chunk(key).unwrap()
    ));
    assert_eq!(stream.connectivity.get(&key), Some(FaceConnectivity::all()));
    assert_eq!(
        stream.mesh_dependency_mask(key),
        Some((generation, MeshDependencyMask::default()))
    );
    assert_eq!(stream.lighting.jobs.pending.len(), light_pending);
    assert!(!stream.is_mesh_clean(key));
    stream.acknowledge_mesh_upload(key, generation, dirty_since, Instant::now());
    assert!(stream.is_mesh_clean(key));
}

#[test]
fn resident_non_air_unknown_and_mixed_palettes_keep_geometry_worker_path() {
    let mixed = world::SubChunk::decode(&[8, 2, 1, 0, 1, 2], &world::RawBlockIds { air: 0 });
    for chunk in [
        super::uniform_sub_chunk(2),
        super::uniform_sub_chunk(4),
        mixed,
    ] {
        let mut stream = lit_stream(1);
        let key = SubChunkKey::new(1, 0, 0, 0);
        resident(&mut stream, key, chunk);
        install_current_light(&mut stream, key, 0, 0, false);
        stream.mark_dirty_exact(key, Instant::now());
        assert_eq!(stream.dispatch_mesh_jobs([8.0; 3], 1), 1);
        assert!(stream.mesh_jobs.in_flight.contains_key(&key));
        assert!(stream.mesh_memory.retained.load(Ordering::Acquire) > 0);
        let completion = stream.mesh_rx.recv_timeout(Duration::from_secs(5)).unwrap();
        stream.accept_mesh_completion(completion);
        assert!(matches!(
            stream.pop_mesh_change(),
            Some(WorldMeshChange::Upsert { .. })
        ));
    }
}

#[test]
fn resident_air_removal_fences_old_mesh_and_later_non_air_replacement() {
    let mut stream = lit_stream(1);
    let key = SubChunkKey::new(1, 0, 0, 0);
    resident(&mut stream, key, super::uniform_sub_chunk(2));
    install_current_light(&mut stream, key, 0, 0, false);
    stream.mark_dirty_exact(key, Instant::now());
    assert_eq!(stream.dispatch_mesh_jobs([8.0; 3], 1), 1);
    let completion = stream.mesh_rx.recv_timeout(Duration::from_secs(5)).unwrap();
    stream.accept_mesh_completion(completion);
    let Some(WorldMeshChange::Upsert {
        generation,
        dirty_since,
        ..
    }) = stream.pop_mesh_change()
    else {
        panic!("initial opaque publication");
    };
    stream.acknowledge_mesh_upload(key, generation, dirty_since, Instant::now());
    stream.mark_dirty_exact(key, Instant::now());
    assert_eq!(stream.dispatch_mesh_jobs([8.0; 3], 1), 1);
    let old_completion = stream.mesh_rx.recv_timeout(Duration::from_secs(5)).unwrap();
    resident(&mut stream, key, super::uniform_sub_chunk(0));
    let air_generation = stream.mark_dirty_exact(key, Instant::now());
    assert_eq!(stream.dispatch_mesh_jobs_with_limits([8.0; 3], 0, 1), 0);
    let Some(WorldMeshChange::Remove {
        generation: removed_generation,
        dirty_since: air_since,
        ..
    }) = stream.pop_mesh_change()
    else {
        panic!("air removal while obsolete worker is retained");
    };
    assert_eq!(removed_generation, air_generation);
    stream.accept_mesh_completion(old_completion);
    assert!(stream.mesh_changes.is_empty());
    assert_eq!(stream.stats.stale_mesh_jobs, 1);
    stream.acknowledge_mesh_upload(key, generation, dirty_since, Instant::now());
    assert!(!stream.is_mesh_clean(key));
    stream.acknowledge_mesh_upload(key, air_generation, air_since, Instant::now());
    assert!(stream.is_mesh_clean(key));
    resident(&mut stream, key, super::uniform_sub_chunk(2));
    install_current_light(&mut stream, key, 0, 0, false);
    let replacement_generation = stream.mark_dirty_exact(key, Instant::now());
    stream.acknowledge_mesh_upload(key, air_generation, air_since, Instant::now());
    assert!(!stream.is_mesh_clean(key));
    assert_eq!(stream.dispatch_mesh_jobs([8.0; 3], 1), 1);
    let completion = stream.mesh_rx.recv_timeout(Duration::from_secs(5)).unwrap();
    stream.accept_mesh_completion(completion);
    let Some(WorldMeshChange::Upsert {
        generation,
        dirty_since,
        mesh,
        ..
    }) = stream.pop_mesh_change()
    else {
        panic!("replacement opaque publication");
    };
    assert_eq!(generation, replacement_generation);
    assert!(!mesh.is_empty());
    stream.acknowledge_mesh_upload(key, generation, dirty_since, Instant::now());
    assert!(stream.is_mesh_clean(key));
    assert_eq!(
        stream.applied_mesh_generations[&key],
        replacement_generation
    );
}

#[test]
fn resident_air_reclassifies_a_cached_geometry_candidate_while_old_worker_is_retained() {
    let mut stream = lit_stream(1);
    let key = SubChunkKey::new(1, 0, 0, 0);
    resident(&mut stream, key, super::uniform_sub_chunk(2));
    install_current_light(&mut stream, key, 0, 0, false);
    stream.mark_dirty_exact(key, Instant::now());
    assert_eq!(stream.dispatch_mesh_jobs([8.0; 3], 1), 1);
    let old_completion = stream.mesh_rx.recv_timeout(Duration::from_secs(5)).unwrap();
    let revision = stream.mark_dirty_exact(key, Instant::now());
    let view = stream.scheduler_view([8.0; 3]);
    stream.mesh_jobs.scan.clear();
    stream.mesh_jobs.lanes[RESIDENT_MESH_LANE]
        .ready
        .push(PendingSchedulerCandidate::new(key, revision, view, false));
    resident(&mut stream, key, super::uniform_sub_chunk(0));
    stream.mark_changed(key, Instant::now());
    assert_eq!(stream.mesh_jobs.pending[&key].revision, revision);
    assert!(stream.mesh_jobs.scan.is_empty());
    stream.poll_deadline = Some(Instant::now());
    assert_eq!(stream.dispatch_mesh_jobs_with_limits([8.0; 3], 0, 1), 0);
    assert!(
        matches!(stream.pop_mesh_change(), Some(WorldMeshChange::Remove { generation, .. }) if generation == revision)
    );
    stream.accept_mesh_completion(old_completion);
    assert!(stream.mesh_changes.is_empty());
}

/// An air conversion coalesced into a queued mesh is removed even when a nearer geometry
/// candidate meets zero worker capacity first.
#[test]
fn coalesced_air_conversion_is_removed_behind_a_capacity_blocked_nearer_mesh() {
    let mut stream = lit_stream(1);
    let near = SubChunkKey::new(1, 0, 0, 0);
    let far = SubChunkKey::new(1, 3, 0, 0);
    for key in [near, far] {
        resident(&mut stream, key, super::uniform_sub_chunk(2));
        install_current_light(&mut stream, key, 0, 0, false);
        stream.mark_dirty_exact(key, Instant::now());
    }
    let revision = stream.mesh_jobs.pending[&far].revision;
    assert_eq!(stream.dispatch_mesh_jobs_with_limits([8.0; 3], 0, 1), 0);
    resident(&mut stream, far, super::uniform_sub_chunk(0));
    stream.mark_changed(far, Instant::now());
    assert_eq!(stream.mesh_jobs.pending[&far].revision, revision);

    assert_eq!(stream.dispatch_mesh_jobs_with_limits([8.0; 3], 0, 1), 0);

    assert!(stream.mesh_jobs.in_flight.is_empty());
    assert!(matches!(
        stream.pop_mesh_change(),
        Some(WorldMeshChange::Remove { key, generation, .. }) if key == far && generation == revision
    ));
}

/// A coalesced removal and the original candidate routed to the removal lane spend one
/// zero-byte permit and publish one removal.
#[test]
fn duplicate_removal_records_spend_one_permit() {
    let mut stream = lit_stream(1);
    let key = SubChunkKey::new(1, 0, 0, 0);
    resident(&mut stream, key, super::uniform_sub_chunk(2));
    install_current_light(&mut stream, key, 0, 0, false);
    stream.mark_dirty_exact(key, Instant::now());
    assert_eq!(stream.dispatch_mesh_jobs_with_limits([8.0; 3], 0, 1), 0);
    resident(&mut stream, key, super::uniform_sub_chunk(0));
    stream.mark_changed(key, Instant::now());
    let config = render_api::PublicationServiceConfig::DEFAULT;
    let allowance = render_api::PublicationAllowance::new(config);
    // Grants several zero-byte operations, so a second removal permit would be admitted.
    allowance.begin_frame(1, 8, 0, 8, 8);
    assert!(allowance.zero_byte_admission_capacity_with_priority(true) >= 2);
    stream.set_publication_allowance(allowance.clone());
    let before = allowance.remaining_zero_byte_operations();

    assert_eq!(stream.dispatch_mesh_jobs_with_limits([8.0; 3], 1, 4), 0);

    assert_eq!(allowance.remaining_zero_byte_operations(), before - 1);
    assert!(matches!(
        stream.pop_mesh_change(),
        Some(WorldMeshChange::Remove { key: removed, .. }) if removed == key
    ));
    assert!(stream.pop_mesh_change().is_none());
}

/// Repeated changes to an air section with no removal credit keep one pending removal.
#[test]
fn repeated_air_invalidations_without_credit_keep_one_removal() {
    let mut stream = lit_stream(1);
    let key = SubChunkKey::new(1, 0, 0, 0);
    resident(&mut stream, key, super::uniform_sub_chunk(2));
    install_current_light(&mut stream, key, 0, 0, false);
    stream.mark_dirty_exact(key, Instant::now());
    let allowance =
        render_api::PublicationAllowance::new(render_api::PublicationServiceConfig::DEFAULT);
    stream.set_publication_allowance(allowance);
    assert_eq!(stream.dispatch_mesh_jobs_with_limits([8.0; 3], 0, 1), 0);
    resident(&mut stream, key, super::uniform_sub_chunk(0));

    for _ in 0..8 {
        stream.mark_changed(key, Instant::now());
        assert_eq!(stream.dispatch_mesh_jobs_with_limits([8.0; 3], 1, 1), 0);
    }

    assert_eq!(stream.air_removals.len(), 1);
    let removal_lane = &stream.mesh_jobs.lanes[MESH_REMOVAL_LANE];
    assert!(
        removal_lane
            .ready
            .iter()
            .chain(removal_lane.deferred.iter())
            .all(|candidate| candidate.key != key)
    );
    assert!(stream.pop_mesh_change().is_none());
}

/// A section that turns to air and back to geometry with no removal credit is rebuilt, never
/// stranded in the removal set.
#[test]
fn air_turned_back_to_geometry_is_rebuilt_without_removal_credit() {
    let mut stream = lit_stream(1);
    let key = SubChunkKey::new(1, 0, 0, 0);
    resident(&mut stream, key, super::uniform_sub_chunk(2));
    install_current_light(&mut stream, key, 0, 0, false);
    stream.mark_dirty_exact(key, Instant::now());
    stream.set_publication_allowance(render_api::PublicationAllowance::new(
        render_api::PublicationServiceConfig::DEFAULT,
    ));
    assert_eq!(stream.dispatch_mesh_jobs_with_limits([8.0; 3], 0, 1), 0);
    resident(&mut stream, key, super::uniform_sub_chunk(0));
    stream.mark_changed(key, Instant::now());
    assert_eq!(stream.dispatch_mesh_jobs_with_limits([8.0; 3], 1, 1), 0);
    assert!(stream.air_removals.contains_key(&key));
    resident(&mut stream, key, super::uniform_sub_chunk(2));
    stream.mark_changed(key, Instant::now());
    settle_light(&mut stream, [8.0; 3]);

    for _ in 0..2 {
        stream.dispatch_mesh_jobs_with_limits([8.0; 3], 1, 1);
    }

    assert!(stream.mesh_jobs.in_flight.contains_key(&key));
    assert!(stream.air_removals.is_empty());
}

/// With credit for one removal, an urgent removal goes before a queued non-urgent air section.
#[test]
fn urgent_removal_outranks_a_queued_air_section() {
    let mut stream = lit_stream(1);
    let calm = SubChunkKey::new(1, 0, 0, 0);
    let urgent = SubChunkKey::new(1, 1, 0, 0);
    for key in [calm, urgent] {
        resident(&mut stream, key, super::uniform_sub_chunk(2));
        install_current_light(&mut stream, key, 0, 0, false);
        stream.mark_dirty_exact(key, Instant::now());
    }
    assert_eq!(stream.dispatch_mesh_jobs_with_limits([8.0; 3], 0, 1), 0);
    resident(&mut stream, calm, super::uniform_sub_chunk(0));
    stream.mark_changed(calm, Instant::now());
    assert!(stream.air_removals.contains_key(&calm));
    resident(&mut stream, urgent, super::uniform_sub_chunk(0));
    stream.mark_dirty_exact_with_priority(urgent, Instant::now(), true);
    let config = render_api::PublicationServiceConfig::DEFAULT;
    let allowance = render_api::PublicationAllowance::new(config);
    allowance.begin_frame(1, 1, 0, 1, 1);
    assert_eq!(
        allowance.zero_byte_admission_capacity_with_priority(true),
        1
    );
    stream.set_publication_allowance(allowance);

    assert_eq!(stream.dispatch_mesh_jobs_with_limits([8.0; 3], 0, 4), 0);

    assert!(matches!(
        stream.pop_mesh_change(),
        Some(WorldMeshChange::Remove { key, .. }) if key == urgent
    ));
    assert!(stream.pop_mesh_change().is_none());
}

/// One dispatch examines a bounded number of air entries, stale ones included.
#[test]
fn air_removal_routing_is_bounded_per_dispatch() {
    let mut stream = lit_stream(1);
    let queued = SubChunkKey::new(1, -8, 0, 0);
    resident(&mut stream, queued, super::uniform_sub_chunk(2));
    stream.mark_dirty_exact(queued, Instant::now());
    for x in 0..1_000 {
        stream.air_removals.insert(SubChunkKey::new(1, x, 0, 0), 1);
    }

    stream.dispatch_mesh_jobs_with_limits([8.0; 3], 0, 4);

    assert!(stream.air_removals.len() < 1_000);
    assert!(stream.air_removals.len() >= 1_000 - MAX_PENDING_SCHEDULER_SCANS_PER_POLL);
}

/// A section that flips between air and geometry while a nearer mesh holds the only dispatch
/// slot keeps one returned ready record, however many flips there are.
#[test]
fn air_geometry_flips_without_worker_capacity_keep_one_returned_record() {
    let mut stream = lit_stream(1);
    let near = SubChunkKey::new(1, 0, 0, 0);
    let flipping = SubChunkKey::new(1, 2, 0, 0);
    for key in [near, flipping] {
        resident(&mut stream, key, super::uniform_sub_chunk(2));
        install_current_light(&mut stream, key, 0, 0, false);
        stream.mark_dirty_exact(key, Instant::now());
    }
    stream.set_publication_allowance(render_api::PublicationAllowance::new(
        render_api::PublicationServiceConfig::DEFAULT,
    ));
    assert_eq!(stream.dispatch_mesh_jobs_with_limits([8.0; 3], 0, 1), 0);
    let records = |stream: &WorldStream| {
        let lane = &stream.mesh_jobs.lanes[RESIDENT_MESH_LANE];
        lane.ready
            .iter()
            .chain(lane.deferred.iter())
            .filter(|candidate| candidate.key == flipping)
            .count()
    };
    let flip = |stream: &mut WorldStream| {
        for block in [0, 2] {
            resident(stream, flipping, super::uniform_sub_chunk(block));
            stream.mark_changed(flipping, Instant::now());
            assert_eq!(stream.dispatch_mesh_jobs_with_limits([8.0; 3], 0, 1), 0);
        }
    };
    flip(&mut stream);
    let after_one_flip = records(&stream);

    for _ in 0..8 {
        flip(&mut stream);
    }

    assert_eq!(records(&stream), after_one_flip);
    assert!(after_one_flip <= 2);
}

/// Stale returned-section records are pruned a bounded slice per dispatch, and all of them
/// over enough dispatches.
#[test]
fn stale_air_returns_are_pruned_a_bounded_slice_per_dispatch() {
    let mut stream = lit_stream(1);
    let queued = SubChunkKey::new(1, -8, 0, 0);
    resident(&mut stream, queued, super::uniform_sub_chunk(2));
    stream.mark_dirty_exact(queued, Instant::now());
    for x in 0..200 {
        stream.air_requeued.insert(SubChunkKey::new(1, x, 0, 0), 1);
    }

    stream.dispatch_mesh_jobs_with_limits([8.0; 3], 0, 1);
    assert_eq!(
        stream.air_requeued.len(),
        200 - AIR_REQUEUE_PRUNES_PER_DISPATCH
    );

    for _ in 0..200 {
        stream.dispatch_mesh_jobs_with_limits([8.0; 3], 0, 1);
    }
    assert!(stream.air_requeued.is_empty());
}

/// Replacing an urgent removal while credit is exhausted must preserve the current removal.
#[test]
fn superseded_deferred_removal_cannot_replace_current_ready_revision() {
    let mut stream = lit_stream(1);
    let key = SubChunkKey::new(1, 0, 0, 0);
    resident(&mut stream, key, super::uniform_sub_chunk(0));
    let first = stream.mark_dirty_exact_with_priority(key, Instant::now(), true);
    assert_eq!(stream.dispatch_mesh_jobs_with_limits([8.0; 3], 1, 0), 0);
    assert!(stream.pop_mesh_change().is_none());
    let current = stream.mark_dirty_exact(key, Instant::now());
    assert_ne!(first, current);
    assert!(stream.mesh_jobs.pending[&key].urgent);

    assert_eq!(stream.dispatch_mesh_jobs_with_limits([8.0; 3], 1, 1), 0);
    assert!(matches!(
        stream.pop_mesh_change(),
        Some(WorldMeshChange::Remove { key: removed, generation, .. })
            if removed == key && generation == current
    ));
    assert!(!stream.mesh_jobs.pending.contains_key(&key));
}
