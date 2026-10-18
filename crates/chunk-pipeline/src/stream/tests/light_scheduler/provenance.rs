use super::*;

#[test]
fn unchanged_packed_light_completion_advances_only_block_ownership() {
    let mut stream = lit_stream(1);
    let key = SubChunkKey::new(1, 0, 0, 0);
    stream
        .authority
        .update_block(key, BlockUpdate::new(8, 8, 8, 0, 1), 0)
        .unwrap();
    stream.resident.insert(key);
    stream.mark_changed(key, Instant::now());
    complete_one_light(&mut stream, [8.0; 3]);
    let original_light = Arc::clone(stream.lighting.store.light(key).unwrap());
    let original_direct = Arc::clone(&stream.lighting.direct_sky[&key].mask);
    let original_light_revision = stream.lighting.ownership[&key].light_revision;
    assert_eq!(original_light.get(LightChannel::Block, 8, 8, 8), Some(15));
    assert_ne!(original_light.get(LightChannel::Block, 0, 0, 0), Some(15));

    stream.mark_light_changed_sources([key]);
    let replacement_block_generation = stream.lighting.block_generations[&key];
    let pending_mesh_before = stream
        .mesh_jobs
        .pending
        .iter()
        .map(|(key, pending)| (*key, (pending.revision, pending.since)))
        .collect::<BTreeSet<_>>();
    let mesh_revisions_before = stream
        .revisions
        .entries
        .iter()
        .map(|(key, dirty)| (*key, (dirty.revision, dirty.since)))
        .collect::<BTreeSet<_>>();
    complete_one_light(&mut stream, [8.0; 3]);

    assert!(Arc::ptr_eq(
        stream.lighting.store.light(key).unwrap(),
        &original_light
    ));
    assert!(Arc::ptr_eq(
        &stream.lighting.direct_sky[&key].mask,
        &original_direct
    ));
    assert_eq!(
        stream.lighting.ownership[&key],
        LightOwnership {
            block_generation: replacement_block_generation,
            light_revision: original_light_revision,
        }
    );
    assert_eq!(
        stream
            .mesh_jobs
            .pending
            .iter()
            .map(|(key, pending)| (*key, (pending.revision, pending.since)))
            .collect::<BTreeSet<_>>(),
        pending_mesh_before
    );
    assert_eq!(
        stream
            .revisions
            .entries
            .iter()
            .map(|(key, dirty)| (*key, (dirty.revision, dirty.since)))
            .collect::<BTreeSet<_>>(),
        mesh_revisions_before
    );
}

#[test]
fn unchanged_completion_rejects_missing_prior_provenance_without_publication() {
    let mut stream = lit_stream(1);
    let key = SubChunkKey::new(1, 0, 0, 0);
    install_current_light(&mut stream, key, 0, 0, false);
    let original_ownership = stream.lighting.ownership[&key];
    let completion = synthetic_light_completion(
        &mut stream,
        key,
        DirectSkyMask::Uniform(false),
        false,
        false,
        [false; 6],
    );
    stream.lighting.direct_sky.remove(&key);

    stream.accept_light_completion(completion);

    assert_eq!(stream.stats().stale_light_jobs, 1);
    assert_eq!(stream.stats().accepted_light_jobs, 0);
    assert_eq!(stream.lighting.ownership[&key], original_ownership);
    assert!(stream.lighting.revisions.dirty(key).is_some());
}

#[test]
fn provenance_only_completion_preserves_sampled_mesh_identity() {
    let mut stream = lit_stream(1);
    let key = SubChunkKey::new(1, 0, 0, 0);
    stream
        .authority
        .commit_sub_chunk(key, super::uniform_sub_chunk(2))
        .unwrap();
    install_current_light(&mut stream, key, 0, 0, false);
    let original_light = Arc::clone(stream.lighting.store.light(key).unwrap());
    let original_direct = Arc::clone(&stream.lighting.direct_sky[&key].mask);
    let original_light_revision = stream.lighting.ownership[&key].light_revision;
    let mesh_revision = stream.mark_dirty_exact(key, Instant::now());
    assert_eq!(stream.dispatch_mesh_jobs([0.0; 3], 1), 1);
    let mesh_completion = stream
        .mesh_rx
        .recv_timeout(Duration::from_secs(2))
        .expect("mesh worker completion");

    let completion = synthetic_light_completion(
        &mut stream,
        key,
        DirectSkyMask::Uniform(true),
        false,
        true,
        [true; 6],
    );
    stream.accept_light_completion(completion);

    assert!(Arc::ptr_eq(
        stream.lighting.store.light(key).unwrap(),
        &original_light
    ));
    assert!(!Arc::ptr_eq(
        &stream.lighting.direct_sky[&key].mask,
        &original_direct
    ));
    assert_eq!(
        stream.lighting.direct_sky[&key].light_revision,
        original_light_revision
    );
    assert_eq!(
        stream.lighting.ownership[&key].light_revision,
        original_light_revision
    );
    assert_eq!(stream.mesh_jobs.in_flight.get(&key), Some(&mesh_revision));
    assert!(!stream.mesh_jobs.pending.contains_key(&key));
    assert!(
        stream
            .light_prior_snapshot(key)
            .has_direct_sky_provenance(key.dimension, BlockPos::new(0, 0, 0))
    );

    stream.accept_mesh_completion(mesh_completion);
    assert_eq!(stream.stats().stale_mesh_jobs, 0);
    assert_eq!(stream.take_mesh_changes().len(), 1);
    assert_eq!(stream.stats().accepted_light_jobs, 1);
    assert_eq!(stream.stats().noop_light_jobs, 0);
    assert_eq!(stream.stats().value_changed_light_jobs, 0);
    assert_eq!(stream.stats().provenance_only_light_jobs, 1);
    assert_eq!(stream.stats().light_mesh_invalidations, 0);
}

#[test]
fn provenance_only_face_change_stales_older_neighbour_solve() {
    let mut stream = lit_stream(1);
    let source = SubChunkKey::new(1, 0, 0, 0);
    let neighbour = SubChunkKey::new(1, 1, 0, 0);
    install_current_light(&mut stream, source, 0, 0, false);
    install_current_light(&mut stream, neighbour, 0, 0, false);
    stream.mark_light_dirty_exact(neighbour).unwrap();
    assert_eq!(stream.dispatch_light_jobs([24.0, 8.0, 8.0], 1), 1);
    let older_neighbour_completion = stream
        .lighting
        .rx
        .recv_timeout(Duration::from_secs(2))
        .expect("older neighbour light completion");

    let completion = synthetic_light_completion(
        &mut stream,
        source,
        DirectSkyMask::Uniform(true),
        false,
        true,
        [false, true, false, false, false, false],
    );
    stream.accept_light_completion(completion);
    assert!(stream.lighting.jobs.pending.contains_key(&neighbour));
    assert!(stream.lighting.jobs.in_flight.contains_key(&neighbour));

    stream.accept_light_completion(older_neighbour_completion);
    assert_eq!(stream.stats().stale_light_jobs, 1);
    assert!(stream.lighting.jobs.pending.contains_key(&neighbour));
}

#[test]
fn stale_light_value_mesh_is_requeued_but_provenance_identity_is_ignored() {
    let mut stream = stream();
    let center = SubChunkKey::new(0, 0, -4, 0);
    let corner = SubChunkKey::new(0, 1, -3, 1);
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
    install_current_light(&mut stream, corner, 4, 5, true);
    let revision = stream.mark_dirty_exact(center, Instant::now());
    assert_eq!(stream.dispatch_mesh_jobs([0.0; 3], 1), 1);
    let completion = stream
        .mesh_rx
        .recv_timeout(Duration::from_secs(2))
        .expect("mesh worker completion");

    install_current_light(&mut stream, corner, 6, 7, false);
    stream.accept_mesh_completion(completion);

    assert_eq!(stream.stats().stale_mesh_jobs, 1);
    assert!(stream.take_mesh_changes().is_empty());
    assert!(!stream.mesh_jobs.in_flight.contains_key(&center));
    assert_eq!(stream.mesh_jobs.pending[&center].revision, revision);
    assert!(stream.revisions.is_current(center, revision));

    assert_eq!(stream.dispatch_mesh_jobs([0.0; 3], 1), 1);
    let completion = stream
        .mesh_rx
        .recv_timeout(Duration::from_secs(2))
        .expect("provenance-only identity mesh completion");
    stream.lighting.direct_sky.get_mut(&corner).unwrap().mask =
        Arc::new(DirectSkyMask::Uniform(false));
    stream.accept_mesh_completion(completion);
    assert_eq!(stream.stats().stale_mesh_jobs, 1);
    assert_eq!(stream.take_mesh_changes().len(), 1);
    assert!(!stream.mesh_jobs.pending.contains_key(&center));
}

#[test]
fn dispatch_snapshot_defers_palette_light_resolution_to_the_worker() {
    let mut stream = lit_stream(1);
    let key = SubChunkKey::new(1, 0, 0, 0);
    stream
        .authority
        .commit_sub_chunk(key, super::uniform_sub_chunk(1))
        .unwrap();
    stream.resident.insert(key);
    stream.mark_changed(key, Instant::now());

    let snapshot = stream.light_block_snapshot(key);

    assert!(snapshot.resolved_light.is_empty());
    assert!(snapshot.blocks.contains_key(&key));
}

#[test]
fn worker_completion_contains_precomputed_voxel_change_summary() {
    let mut stream = lit_stream(0);
    let key = SubChunkKey::new(0, 0, 19, 0);
    stream.record_known_air(key);
    stream.mark_changed(key, Instant::now());
    assert_eq!(stream.dispatch_light_jobs([8.0, 312.0, 8.0], 1), 1);

    let completion = stream
        .lighting
        .rx
        .recv_timeout(Duration::from_secs(2))
        .unwrap();
    let solved = completion.result.as_ref().unwrap();

    assert!(solved.used_uniform_fast_path);
    assert!(solved.light_levels_changed);
    assert!(solved.direct_sky_changed);
    assert!(solved.direct_sky.get(0, 15, 0));
    assert!(solved.changed_faces.into_iter().any(|changed| changed));
}

#[test]
fn worker_distinguishes_provenance_only_output_from_sampled_light_changes() {
    let mut stream = lit_stream(0);
    let key = SubChunkKey::new(0, 0, 19, 0);
    install_current_light(&mut stream, key, 0, 15, false);
    let original_light = Arc::clone(stream.lighting.store.light(key).unwrap());
    stream.mark_light_dirty_exact(key).unwrap();
    assert_eq!(stream.dispatch_light_jobs([8.0, 312.0, 8.0], 1), 1);
    let completion = stream
        .lighting
        .rx
        .recv_timeout(Duration::from_secs(2))
        .expect("provenance-only worker completion");
    let solved = completion.result.as_ref().unwrap();
    assert!(!solved.light_levels_changed);
    assert!(solved.direct_sky_changed);

    stream.accept_light_completion(completion);
    assert!(Arc::ptr_eq(
        stream.lighting.store.light(key).unwrap(),
        &original_light
    ));
    assert!(stream.lighting.direct_sky[&key].mask.get(0, 15, 0));
    assert_eq!(stream.stats().provenance_only_light_jobs, 1);
    assert_eq!(stream.stats().light_mesh_invalidations, 0);
}
