use super::*;

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
    assert_eq!(halo.sample_channels([0, 0, 16]), [0, 15]);
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
        &RAW_IDS,
    );
    stream
        .authority
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
    assert_eq!(stream.mesh_jobs.pending[&center].revision, mesh_revision);
    assert!(!stream.mesh_jobs.in_flight.contains_key(&center));

    install_current_light(&mut stream, corner, 0, 0, false);
    assert_eq!(stream.dispatch_mesh_jobs([0.0; 3], 1), 1);
    assert_eq!(
        stream.mesh_jobs.in_flight.get(&center),
        Some(&mesh_revision)
    );
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
        &RAW_IDS,
    );
    stream
        .authority
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
    assert_eq!(stream.mesh_jobs.pending[&center].revision, revision);
}

#[test]
fn light_change_invalidation_only_dirties_renderable_dependents_once() {
    let mut stream = lit_stream(0);
    let source = SubChunkKey::new(0, 4, 5, 6);
    stream
        .authority
        .commit_sub_chunk(source, super::uniform_sub_chunk(1))
        .unwrap();
    stream.resident.insert(source);
    let known_air = SubChunkKey::new(0, 5, 5, 6);
    stream.record_known_air(known_air);
    let first = Instant::now();

    stream.mark_light_mesh_dependents(source, first);
    stream.mark_light_mesh_dependents(source, first + Duration::from_millis(1));

    assert_eq!(
        stream
            .mesh_jobs
            .pending
            .keys()
            .copied()
            .collect::<BTreeSet<_>>(),
        BTreeSet::from([source])
    );
    assert!(!stream.mesh_jobs.pending.contains_key(&known_air));
    assert!(
        stream
            .mesh_jobs
            .pending
            .values()
            .all(|pending| pending.since == first)
    );
}

#[test]
fn changed_light_faces_scope_mesh_invalidation_to_sampling_dependents() {
    let mut stream = lit_stream(0);
    let source = SubChunkKey::new(0, 4, 5, 6);
    let east = SubChunkKey::new(0, 5, 5, 6);
    let up = SubChunkKey::new(0, 4, 6, 6);
    let east_up = SubChunkKey::new(0, 5, 6, 6);
    let west = SubChunkKey::new(0, 3, 5, 6);
    for key in [source, east, up, east_up, west] {
        stream
            .authority
            .commit_sub_chunk(key, super::uniform_sub_chunk(1))
            .unwrap();
        stream.resident.insert(key);
    }

    stream.mark_changed_light_mesh_dependents(
        source,
        [false, true, false, true, false, false],
        Instant::now(),
        false,
    );
    let revisions = stream
        .mesh_jobs
        .pending
        .iter()
        .map(|(&key, pending)| (key, pending.revision))
        .collect::<BTreeMap<_, _>>();
    stream.mark_changed_light_mesh_dependents(
        source,
        [false, true, false, true, false, false],
        Instant::now(),
        false,
    );

    assert_eq!(
        stream
            .mesh_jobs
            .pending
            .keys()
            .copied()
            .collect::<BTreeSet<_>>(),
        BTreeSet::from([source, east, up, east_up])
    );
    assert_eq!(
        stream
            .mesh_jobs
            .pending
            .iter()
            .map(|(&key, pending)| (key, pending.revision))
            .collect::<BTreeMap<_, _>>(),
        revisions,
        "a queued mesh observes the latest light when it is prepared and must not be revised again"
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
        &RAW_IDS,
    );
    stream
        .authority
        .commit_level_chunk(key.chunk(), decoded)
        .unwrap();
    stream.resident.insert(key);
    stream.mark_changed(key, Instant::now());

    assert_eq!(stream.dispatch_mesh_jobs([0.0; 3], 1), 0);
    assert!(!stream.mesh_jobs.in_flight.contains_key(&key));

    complete_one_light(&mut stream, [0.0; 3]);
    assert_eq!(stream.dispatch_mesh_jobs([0.0; 3], 1), 1);
    assert_eq!(
        stream.mesh_jobs.in_flight.get(&key).copied(),
        stream.revisions.dirty(key).map(|dirty| dirty.revision)
    );
}

#[test]
fn dirty_or_stale_boundary_light_is_untrusted() {
    let mut stream = lit_stream(0);
    let boundary = SubChunkKey::new(0, -1, 0, 0);
    stream
        .authority
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
    stream.lighting.revisions.entries.remove(&boundary);
    stream.lighting.jobs.pending.remove(&boundary);
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
        .authority
        .commit_sub_chunk(emitter, super::uniform_sub_chunk(1))
        .unwrap();
    stream.resident.insert(emitter);
    stream.mark_changed(emitter, Instant::now());
    complete_one_light(&mut stream, [-8.0, 8.0, 8.0]);

    let target = SubChunkKey::new(0, 0, 0, 0);
    stream
        .authority
        .commit_sub_chunk(target, super::uniform_sub_chunk(3))
        .unwrap();
    stream.resident.insert(target);
    stream.mark_changed(target, Instant::now());
    complete_one_light(&mut stream, [-8.0, 8.0, 8.0]);
    assert_eq!(stream.dispatch_light_jobs([8.0, 8.0, 8.0], 1), 1);
    stream.mesh_jobs.pending.clear();
    stream.revisions.entries.clear();
    let previous_light = Arc::clone(stream.lighting.store.light(target).unwrap());
    stream.stats.accepted_light_jobs = 0;
    stream.stats.noop_light_jobs = 0;
    stream.stats.value_changed_light_jobs = 0;
    stream.stats.provenance_only_light_jobs = 0;
    stream.stats.light_mesh_invalidations = 0;

    let completion = stream
        .lighting
        .rx
        .recv_timeout(Duration::from_secs(2))
        .unwrap();
    stream.accept_light_completion(completion);

    assert_eq!(
        stream
            .lighting
            .store
            .light(target)
            .unwrap()
            .get(LightChannel::Block, 0, 0, 0),
        Some(14)
    );
    assert!(!Arc::ptr_eq(
        stream.lighting.store.light(target).unwrap(),
        &previous_light
    ));
    assert!(stream.mesh_jobs.pending.contains_key(&target));
    assert!(stream.revisions.dirty(target).is_some());
    assert_eq!(
        target
            .mesh_neighbourhood_dependents()
            .filter(|dependent| stream.mesh_jobs.pending.contains_key(dependent))
            .count(),
        2
    );
    assert!(stream.mesh_jobs.pending.contains_key(&emitter));
    assert_eq!(stream.stats().accepted_light_jobs, 1);
    assert_eq!(stream.stats().noop_light_jobs, 0);
    assert_eq!(stream.stats().value_changed_light_jobs, 1);
    assert_eq!(stream.stats().provenance_only_light_jobs, 0);
    assert_eq!(stream.stats().light_mesh_invalidations, 1);
}

#[test]
fn changed_light_face_requeues_exact_resident_neighbour() {
    let mut stream = lit_stream(0);
    let source = SubChunkKey::new(0, 0, 0, 0);
    let neighbour = SubChunkKey::new(0, 1, 0, 0);
    stream.record_known_air(neighbour);
    stream.mark_changed(neighbour, Instant::now());
    stream.lighting.revisions.entries.remove(&neighbour);
    stream.lighting.jobs.pending.remove(&neighbour);
    stream
        .authority
        .commit_sub_chunk(source, super::uniform_sub_chunk(1))
        .unwrap();
    stream.resident.insert(source);
    stream.mark_changed(source, Instant::now());
    assert_eq!(stream.dispatch_light_jobs([8.0, 8.0, 8.0], 1), 1);
    stream.lighting.revisions.entries.remove(&neighbour);
    stream.lighting.jobs.pending.remove(&neighbour);

    let completion = stream
        .lighting
        .rx
        .recv_timeout(Duration::from_secs(2))
        .unwrap();
    stream.accept_light_completion(completion);

    assert!(stream.lighting.jobs.pending.contains_key(&neighbour));
    assert!(
        !stream
            .lighting
            .jobs
            .pending
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
    stream.lighting.jobs.pending.clear();
    stream.lighting.revisions.entries.clear();

    let snapshot = stream.light_block_snapshot(center);
    assert_eq!(snapshot.blocks.iter().count(), 2);
    assert!(snapshot.blocks.contains_key(&center));
    assert!(snapshot.blocks.contains_key(&face));
    assert!(!snapshot.blocks.contains_key(&diagonal));

    stream.mark_light_changed_sources([center]);
    assert!(stream.lighting.jobs.pending.contains_key(&center));
    assert!(!stream.lighting.jobs.pending.contains_key(&diagonal));
}
