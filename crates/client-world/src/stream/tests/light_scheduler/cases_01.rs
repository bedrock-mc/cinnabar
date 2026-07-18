use super::*;

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
fn unchanged_packed_light_completion_advances_only_block_ownership() {
    let mut stream = lit_stream(1);
    let key = SubChunkKey::new(1, 0, 0, 0);
    stream
        .store
        .update_block(key, BlockUpdate::new(8, 8, 8, 0, 1), 0)
        .unwrap();
    stream.resident.insert(key);
    stream.mark_changed(key, Instant::now());
    complete_one_light(&mut stream, [8.0; 3]);
    let original_light = Arc::clone(stream.light_store.light(key).unwrap());
    let original_direct = Arc::clone(&stream.direct_sky[&key].mask);
    let original_light_revision = stream.light_ownership[&key].light_revision;
    assert_eq!(original_light.get(LightChannel::Block, 8, 8, 8), Some(15));
    assert_ne!(original_light.get(LightChannel::Block, 0, 0, 0), Some(15));

    stream.mark_light_changed_sources([key]);
    let replacement_block_generation = stream.block_generations[&key];
    let pending_mesh_before = stream
        .pending_mesh
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
        stream.light_store.light(key).unwrap(),
        &original_light
    ));
    assert!(Arc::ptr_eq(&stream.direct_sky[&key].mask, &original_direct));
    assert_eq!(
        stream.light_ownership[&key],
        LightOwnership {
            block_generation: replacement_block_generation,
            light_revision: original_light_revision,
        }
    );
    assert_eq!(
        stream
            .pending_mesh
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
    let original_ownership = stream.light_ownership[&key];
    let completion = synthetic_light_completion(
        &mut stream,
        key,
        DirectSkyMask::Uniform(false),
        false,
        false,
        [false; 6],
    );
    stream.direct_sky.remove(&key);

    stream.accept_light_completion(completion);

    assert_eq!(stream.stats().stale_light_jobs, 1);
    assert_eq!(stream.stats().accepted_light_jobs, 0);
    assert_eq!(stream.light_ownership[&key], original_ownership);
    assert!(stream.light_revisions.dirty(key).is_some());
}

#[test]
fn provenance_only_completion_preserves_sampled_mesh_identity() {
    let mut stream = lit_stream(1);
    let key = SubChunkKey::new(1, 0, 0, 0);
    stream
        .store
        .commit_sub_chunk(key, super::uniform_sub_chunk(2))
        .unwrap();
    install_current_light(&mut stream, key, 0, 0, false);
    let original_light = Arc::clone(stream.light_store.light(key).unwrap());
    let original_direct = Arc::clone(&stream.direct_sky[&key].mask);
    let original_light_revision = stream.light_ownership[&key].light_revision;
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
        stream.light_store.light(key).unwrap(),
        &original_light
    ));
    assert!(!Arc::ptr_eq(
        &stream.direct_sky[&key].mask,
        &original_direct
    ));
    assert_eq!(
        stream.direct_sky[&key].light_revision,
        original_light_revision
    );
    assert_eq!(
        stream.light_ownership[&key].light_revision,
        original_light_revision
    );
    assert_eq!(stream.in_flight.get(&key), Some(&mesh_revision));
    assert!(!stream.pending_mesh.contains_key(&key));
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
        .light_rx
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
    assert!(stream.pending_light.contains_key(&neighbour));
    assert!(stream.in_flight_light.contains_key(&neighbour));

    stream.accept_light_completion(older_neighbour_completion);
    assert_eq!(stream.stats().stale_light_jobs, 1);
    assert!(stream.pending_light.contains_key(&neighbour));
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
fn mesh_light_halo_samples_center_face_edge_corner_and_absent_fallback() {
    let mut stream = stream();
    let center = SubChunkKey::new(0, 4, 5, 6);
    install_current_light(&mut stream, center, 1, 2, false);
    install_current_light(&mut stream, SubChunkKey::new(0, 5, 5, 6), 3, 4, false);
    install_current_light(&mut stream, SubChunkKey::new(0, 5, 6, 6), 5, 6, false);
    install_current_light(&mut stream, SubChunkKey::new(0, 3, 4, 5), 7, 8, true);

    let halo = stream.mesh_light_halo(center).expect("current fixed halo");
    assert_eq!(halo.sample_channels([0, 0, 0]), [1, 2]);
    assert_eq!(halo.sample_channels([16, 0, 0]), [3, 4]);
    assert_eq!(halo.sample_channels([16, 16, 0]), [5, 6]);
    assert_eq!(halo.sample_channels([-1, -1, -1]), [7, 8]);
    assert_eq!(halo.sample_channels([0, 0, 16]), [0, 0]);
    assert_eq!(halo.sample_channels([32, 0, 0]), [0, 0]);
    assert_eq!(halo.occupied_slot_count(), 4);
}

#[test]
fn mesh_dispatch_waits_for_every_known_light_halo_slot() {
    let mut stream = stream();
    let center = SubChunkKey::new(0, 0, -4, 0);
    let decoded = DecodedLevelChunk::decode(
        center.y,
        1,
        include_bytes!(concat!(
            env!("CARGO_MANIFEST_DIR"),
            "/../world/fixtures/uniform_non_air.bin"
        )),
    )
    .unwrap();
    stream
        .store
        .commit_level_chunk(center.chunk(), decoded)
        .unwrap();
    install_current_light(&mut stream, center, 0, 0, false);
    let face = SubChunkKey::new(0, 1, -4, 0);
    let edge = SubChunkKey::new(0, 1, -3, 0);
    let corner = SubChunkKey::new(0, 1, -3, 1);
    for key in [face, edge, corner] {
        install_current_light(&mut stream, key, 0, 0, false);
    }
    let mesh_revision = stream.mark_dirty_exact(center, Instant::now());
    stream.mark_light_dirty_exact(corner);

    assert_eq!(stream.dispatch_mesh_jobs([0.0; 3], 1), 0);
    assert_eq!(stream.pending_mesh[&center].revision, mesh_revision);
    assert!(!stream.in_flight.contains_key(&center));

    install_current_light(&mut stream, corner, 0, 0, false);
    assert_eq!(stream.dispatch_mesh_jobs([0.0; 3], 1), 1);
    assert_eq!(stream.in_flight.get(&center), Some(&mesh_revision));
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
    )
    .unwrap();
    stream
        .store
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
    assert!(!stream.in_flight.contains_key(&center));
    assert_eq!(stream.pending_mesh[&center].revision, revision);
    assert!(stream.revisions.is_current(center, revision));

    assert_eq!(stream.dispatch_mesh_jobs([0.0; 3], 1), 1);
    let completion = stream
        .mesh_rx
        .recv_timeout(Duration::from_secs(2))
        .expect("provenance-only identity mesh completion");
    stream.direct_sky.get_mut(&corner).unwrap().mask = Arc::new(DirectSkyMask::Uniform(false));
    stream.accept_mesh_completion(completion);
    assert_eq!(stream.stats().stale_mesh_jobs, 1);
    assert_eq!(stream.take_mesh_changes().len(), 1);
    assert!(!stream.pending_mesh.contains_key(&center));
}

#[test]
fn mid_flight_light_halo_load_rejects_preload_mesh_completion() {
    let mut stream = stream();
    let center = SubChunkKey::new(0, 0, -4, 0);
    let newly_loaded_corner = SubChunkKey::new(0, 1, -3, 1);
    let decoded = DecodedLevelChunk::decode(
        center.y,
        1,
        include_bytes!(concat!(
            env!("CARGO_MANIFEST_DIR"),
            "/../world/fixtures/uniform_non_air.bin"
        )),
    )
    .unwrap();
    stream
        .store
        .commit_level_chunk(center.chunk(), decoded)
        .unwrap();
    install_current_light(&mut stream, center, 0, 0, false);
    let revision = stream.mark_dirty_exact(center, Instant::now());
    assert_eq!(stream.dispatch_mesh_jobs([0.0; 3], 1), 1);
    let completion = stream
        .mesh_rx
        .recv_timeout(Duration::from_secs(2))
        .expect("preload mesh worker completion");

    install_current_light(&mut stream, newly_loaded_corner, 3, 4, true);
    stream.accept_mesh_completion(completion);

    assert_eq!(stream.stats().stale_mesh_jobs, 1);
    assert!(stream.take_mesh_changes().is_empty());
    assert_eq!(stream.pending_mesh[&center].revision, revision);
}

#[test]
fn light_change_invalidation_only_dirties_renderable_dependents_once() {
    let mut stream = lit_stream(0);
    let source = SubChunkKey::new(0, 4, 5, 6);
    stream
        .store
        .commit_sub_chunk(source, super::uniform_sub_chunk(1))
        .unwrap();
    stream.resident.insert(source);
    let known_air = SubChunkKey::new(0, 5, 5, 6);
    stream.record_known_air(known_air);
    let first = Instant::now();

    stream.mark_light_mesh_dependents(source, first);
    stream.mark_light_mesh_dependents(source, first + Duration::from_millis(1));

    assert_eq!(
        stream.pending_mesh.keys().copied().collect::<BTreeSet<_>>(),
        BTreeSet::from([source])
    );
    assert!(!stream.pending_mesh.contains_key(&known_air));
    assert!(
        stream
            .pending_mesh
            .values()
            .all(|pending| pending.since == first)
    );
}

#[test]
fn mesh_dispatch_waits_for_current_light() {
    let mut stream = stream();
    let key = SubChunkKey::new(0, 0, -4, 0);
    let decoded = DecodedLevelChunk::decode(
        key.y,
        1,
        include_bytes!(concat!(
            env!("CARGO_MANIFEST_DIR"),
            "/../world/fixtures/uniform_non_air.bin"
        )),
    )
    .unwrap();
    stream
        .store
        .commit_level_chunk(key.chunk(), decoded)
        .unwrap();
    stream.resident.insert(key);
    stream.mark_changed(key, Instant::now());

    assert_eq!(stream.dispatch_mesh_jobs([0.0; 3], 1), 0);
    assert!(!stream.in_flight.contains_key(&key));

    complete_one_light(&mut stream, [0.0; 3]);
    assert_eq!(stream.dispatch_mesh_jobs([0.0; 3], 1), 1);
    assert_eq!(
        stream.in_flight.get(&key).copied(),
        stream.revisions.dirty(key).map(|dirty| dirty.revision)
    );
}

#[test]
fn runtime_light_metadata_propagates_across_a_known_air_seam() {
    let mut stream = lit_stream(0);
    let emitter = SubChunkKey::new(0, -1, 0, 0);
    stream
        .store
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
            .light_store
            .light(air)
            .unwrap()
            .get(LightChannel::Block, 0, 0, 0),
        Some(14)
    );
    assert!(stream.light_is_current(air));
}

#[test]
fn dirty_or_stale_boundary_light_is_untrusted() {
    let mut stream = lit_stream(0);
    let boundary = SubChunkKey::new(0, -1, 0, 0);
    stream
        .store
        .commit_sub_chunk(boundary, super::uniform_sub_chunk(1))
        .unwrap();
    stream.resident.insert(boundary);
    stream.mark_changed(boundary, Instant::now());
    complete_one_light(&mut stream, [-8.0, 8.0, 8.0]);
    let target = SubChunkKey::new(0, 0, 0, 0);
    stream.record_known_air(target);
    stream.mark_changed(target, Instant::now());
    complete_one_light(&mut stream, [-8.0, 8.0, 8.0]);
    let sample = BlockPos::new(-1, 0, 0);
    assert_eq!(
        stream
            .light_prior_snapshot(target)
            .boundary_light(0, sample, LightChannel::Block),
        BoundaryLightSample::trusted(15, false).unwrap()
    );

    stream.mark_light_dirty_exact(boundary);
    assert_eq!(
        stream
            .light_prior_snapshot(target)
            .boundary_light(0, sample, LightChannel::Block),
        BoundaryLightSample::untrusted()
    );
    stream.light_revisions.entries.remove(&boundary);
    stream.pending_light.remove(&boundary);
    stream.mark_light_changed_sources([boundary]);
    assert_eq!(
        stream
            .light_prior_snapshot(target)
            .boundary_light(0, sample, LightChannel::Block),
        BoundaryLightSample::untrusted()
    );
}

#[test]
fn changed_light_levels_dirty_a_renderable_mesh_generation() {
    let mut stream = lit_stream(0);
    let emitter = SubChunkKey::new(0, -1, 0, 0);
    stream
        .store
        .commit_sub_chunk(emitter, super::uniform_sub_chunk(1))
        .unwrap();
    stream.resident.insert(emitter);
    stream.mark_changed(emitter, Instant::now());
    complete_one_light(&mut stream, [-8.0, 8.0, 8.0]);

    let target = SubChunkKey::new(0, 0, 0, 0);
    stream
        .store
        .commit_sub_chunk(target, super::uniform_sub_chunk(3))
        .unwrap();
    stream.resident.insert(target);
    stream.mark_changed(target, Instant::now());
    complete_one_light(&mut stream, [-8.0, 8.0, 8.0]);
    assert_eq!(stream.dispatch_light_jobs([8.0, 8.0, 8.0], 1), 1);
    stream.pending_mesh.clear();
    stream.revisions.entries.clear();
    let previous_light = Arc::clone(stream.light_store.light(target).unwrap());
    stream.stats.accepted_light_jobs = 0;
    stream.stats.noop_light_jobs = 0;
    stream.stats.value_changed_light_jobs = 0;
    stream.stats.provenance_only_light_jobs = 0;
    stream.stats.light_mesh_invalidations = 0;

    let completion = stream
        .light_rx
        .recv_timeout(Duration::from_secs(2))
        .unwrap();
    stream.accept_light_completion(completion);

    assert_eq!(
        stream
            .light_store
            .light(target)
            .unwrap()
            .get(LightChannel::Block, 0, 0, 0),
        Some(14)
    );
    assert!(!Arc::ptr_eq(
        stream.light_store.light(target).unwrap(),
        &previous_light
    ));
    assert!(stream.pending_mesh.contains_key(&target));
    assert!(stream.revisions.dirty(target).is_some());
    assert_eq!(
        target
            .mesh_neighbourhood_dependents()
            .filter(|dependent| stream.pending_mesh.contains_key(dependent))
            .count(),
        2
    );
    assert!(stream.pending_mesh.contains_key(&emitter));
    assert_eq!(stream.stats().accepted_light_jobs, 1);
    assert_eq!(stream.stats().noop_light_jobs, 0);
    assert_eq!(stream.stats().value_changed_light_jobs, 1);
    assert_eq!(stream.stats().provenance_only_light_jobs, 0);
    assert_eq!(stream.stats().light_mesh_invalidations, 1);
}

#[test]
fn overworld_seeds_direct_sky_only_from_known_air_at_dimension_top() {
    let mut stream = lit_stream(0);
    let top = SubChunkKey::new(0, 0, 19, 0);
    stream.record_known_air(top);
    stream.mark_changed(top, Instant::now());
    complete_one_light(&mut stream, [8.0, 312.0, 8.0]);

    let light = stream.light_store.light(top).unwrap();
    assert_eq!(light.get(LightChannel::Sky, 0, 15, 0), Some(15));
    assert_eq!(light.get(LightChannel::Sky, 0, 14, 0), Some(15));
    assert!(stream.direct_sky[&top].mask.get(0, 15, 0));
    assert!(stream.direct_sky[&top].mask.get(0, 14, 0));

    let below = SubChunkKey::new(0, 0, 18, 0);
    stream.record_known_air(below);
    stream.mark_changed(below, Instant::now());
    let blocks = stream.light_block_snapshot(below);
    assert_eq!(blocks.sky_seed(BlockPos::new(0, 303, 0)), 0);
    complete_one_light(&mut stream, [8.0, 312.0, 8.0]);
    complete_one_light(&mut stream, [8.0, 296.0, 8.0]);
    assert_eq!(
        stream
            .light_store
            .light(below)
            .unwrap()
            .get(LightChannel::Sky, 0, 0, 0),
        Some(15)
    );
    assert!(stream.direct_sky[&below].mask.get(0, 0, 0));
    assert!(stream.light_is_current(top));
    assert!(stream.light_is_current(below));

    stream.mark_light_dirty_exact(top);
    stream.mark_light_dirty_exact(below);
    assert_eq!(stream.dispatch_light_jobs([8.0, 296.0, 8.0], 1), 1);
    assert!(stream.in_flight_light.contains_key(&top));
    assert!(!stream.in_flight_light.contains_key(&below));
    let completion = stream
        .light_rx
        .recv_timeout(Duration::from_secs(2))
        .unwrap();
    stream.accept_light_completion(completion);
    complete_one_light(&mut stream, [8.0, 296.0, 8.0]);
    assert!(stream.direct_sky[&below].mask.get(0, 0, 0));
    assert_eq!(
        stream
            .light_store
            .light(below)
            .unwrap()
            .get(LightChannel::Sky, 0, 0, 0),
        Some(15)
    );
    assert!(stream.pending_light.is_empty());
    assert!(stream.in_flight_light.is_empty());
    assert!(stream.light_is_current(top));
    assert!(stream.light_is_current(below));
}

#[test]
fn limited_empty_column_propagates_direct_sky_into_the_mesh_light_sidecar() {
    let mut stream = lit_stream(0);
    stream
        .submit(
            1,
            super::request_level_chunk_event(
                0,
                0,
                0,
                protocol::LevelChunkMode::LimitedRequests { highest: 0 },
                1,
            ),
        )
        .unwrap();
    super::complete_pending_decode_jobs(&mut stream);

    assert!(stream.take_requests().is_empty());
    for y in (-4..=19).rev() {
        complete_one_light(&mut stream, [8.0, y as f32 * 16.0 + 8.0, 8.0]);
    }

    let bottom = SubChunkKey::new(0, 0, -4, 0);
    assert_eq!(
        stream
            .light_store
            .light(bottom)
            .unwrap()
            .get(LightChannel::Sky, 0, 0, 0),
        Some(15)
    );
    assert!(stream.direct_sky[&bottom].mask.get(0, 0, 0));
    let halo = stream
        .mesh_light_halo(bottom)
        .expect("settled implicit air exposes a current mesh-light sidecar");
    assert_eq!(halo.sample_channels([0, 0, 0]), [0, 15]);
}

#[test]
fn nether_and_end_never_seed_sky() {
    for (dimension, y) in [(1, 7), (2, 15)] {
        let mut stream = lit_stream(dimension);
        let key = SubChunkKey::new(dimension, 0, y, 0);
        stream.record_known_air(key);
        stream.mark_changed(key, Instant::now());
        complete_one_light(&mut stream, [8.0, y as f32 * 16.0 + 8.0, 8.0]);

        assert_eq!(
            stream
                .light_store
                .light(key)
                .unwrap()
                .get(LightChannel::Sky, 0, 15, 0),
            Some(0)
        );
        assert!(!stream.direct_sky[&key].mask.get(0, 15, 0));
    }
}

#[test]
fn changed_light_face_requeues_exact_resident_neighbour() {
    let mut stream = lit_stream(0);
    let source = SubChunkKey::new(0, 0, 0, 0);
    let neighbour = SubChunkKey::new(0, 1, 0, 0);
    stream.record_known_air(neighbour);
    stream.mark_changed(neighbour, Instant::now());
    stream.light_revisions.entries.remove(&neighbour);
    stream.pending_light.remove(&neighbour);
    stream
        .store
        .commit_sub_chunk(source, super::uniform_sub_chunk(1))
        .unwrap();
    stream.resident.insert(source);
    stream.mark_changed(source, Instant::now());
    assert_eq!(stream.dispatch_light_jobs([8.0, 8.0, 8.0], 1), 1);
    stream.light_revisions.entries.remove(&neighbour);
    stream.pending_light.remove(&neighbour);

    let completion = stream
        .light_rx
        .recv_timeout(Duration::from_secs(2))
        .unwrap();
    stream.accept_light_completion(completion);

    assert!(stream.pending_light.contains_key(&neighbour));
    assert!(
        !stream
            .pending_light
            .contains_key(&SubChunkKey::new(0, 1, 1, 0))
    );
}

#[test]
fn light_snapshots_and_invalidation_exclude_diagonals() {
    let mut stream = lit_stream(0);
    let center = SubChunkKey::new(0, 0, 0, 0);
    let face = SubChunkKey::new(0, 1, 0, 0);
    let diagonal = SubChunkKey::new(0, 1, 1, 0);
    stream.record_known_air(center);
    stream.record_known_air(face);
    stream.record_known_air(diagonal);
    stream.mark_changed(center, Instant::now());
    stream.pending_light.clear();
    stream.light_revisions.entries.clear();

    let snapshot = stream.light_block_snapshot(center);
    assert_eq!(snapshot.blocks.len(), 2);
    assert!(snapshot.blocks.contains_key(&center));
    assert!(snapshot.blocks.contains_key(&face));
    assert!(!snapshot.blocks.contains_key(&diagonal));

    stream.mark_light_changed_sources([center]);
    assert!(stream.pending_light.contains_key(&center));
    assert!(!stream.pending_light.contains_key(&diagonal));
}

#[test]
fn dispatch_snapshot_defers_palette_light_resolution_to_the_worker() {
    let mut stream = lit_stream(1);
    let key = SubChunkKey::new(1, 0, 0, 0);
    stream
        .store
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
        .light_rx
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
    let original_light = Arc::clone(stream.light_store.light(key).unwrap());
    stream.mark_light_dirty_exact(key).unwrap();
    assert_eq!(stream.dispatch_light_jobs([8.0, 312.0, 8.0], 1), 1);
    let completion = stream
        .light_rx
        .recv_timeout(Duration::from_secs(2))
        .expect("provenance-only worker completion");
    let solved = completion.result.as_ref().unwrap();
    assert!(!solved.light_levels_changed);
    assert!(solved.direct_sky_changed);

    stream.accept_light_completion(completion);
    assert!(Arc::ptr_eq(
        stream.light_store.light(key).unwrap(),
        &original_light
    ));
    assert!(stream.direct_sky[&key].mask.get(0, 15, 0));
    assert_eq!(stream.stats().provenance_only_light_jobs, 1);
    assert_eq!(stream.stats().light_mesh_invalidations, 0);
}

#[test]
fn light_jobs_are_nearest_first_deduplicated_and_worker_bounded() {
    let mut stream = lit_stream(0);
    let keys = (0..6)
        .map(|x| SubChunkKey::new(0, x, 0, 0))
        .collect::<Vec<_>>();
    for key in &keys {
        stream.record_known_air(*key);
    }
    stream.mark_light_changed_sources(keys.iter().copied());
    let latest = stream.pending_light[&keys[5]].revision;
    stream.mark_light_dirty_exact(keys[5]);
    assert_eq!(stream.pending_light.len(), keys.len());
    assert_ne!(stream.pending_light[&keys[5]].revision, latest);

    assert_eq!(stream.dispatch_light_jobs([8.0, 8.0, 8.0], usize::MAX), 3);
    assert_eq!(stream.in_flight_light.len(), 3);
    assert_eq!(
        stream
            .in_flight_light
            .keys()
            .copied()
            .collect::<BTreeSet<_>>(),
        [keys[0], keys[2], keys[4]].into_iter().collect()
    );
}

#[test]
fn one_completion_releases_one_independent_worker_slot() {
    let mut stream = lit_stream(1);
    let capacity = super::super::MAX_IN_FLIGHT_LIGHT_JOBS;
    let radius = super::super::PHASE0_MAX_VIEW_RADIUS_CHUNKS;
    let keys = (-radius..=radius)
        .flat_map(|x| (-radius..=radius).map(move |z| (x, z)))
        .filter(|(x, z)| (x + z).rem_euclid(2) == 0)
        .map(|(x, z)| SubChunkKey::new(1, x, 0, z))
        .take(capacity + 1)
        .collect::<Vec<_>>();
    assert_eq!(keys.len(), capacity + 1);
    for key in &keys {
        stream.record_known_air(*key);
    }
    stream.mark_light_changed_sources(keys.iter().copied());

    assert_eq!(
        stream.dispatch_light_jobs([8.0, 8.0, 8.0], usize::MAX),
        capacity
    );
    assert_eq!(stream.in_flight_light.len(), capacity);
    let completion = stream
        .light_rx
        .recv_timeout(Duration::from_secs(5))
        .expect("independent light completion");
    stream.accept_light_completion(completion);

    assert_eq!(stream.dispatch_light_jobs([8.0; 3], usize::MAX), 1);
    assert_eq!(stream.in_flight_light.len(), capacity);
}
