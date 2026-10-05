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
    assert_eq!(stream.stats.phase2_stages.mesh_jobs_dispatched, 0);
    assert!(stream.resident.contains(&key));
    assert!(!stream.known_air.contains(&key));
    assert!(Arc::ptr_eq(
        &source,
        &stream.authority.terrain().sub_chunk(key).unwrap()
    ));
    assert_eq!(stream.connectivity[&key], FaceConnectivity::all());
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
