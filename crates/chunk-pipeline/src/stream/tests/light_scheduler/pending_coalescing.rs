use super::*;

#[test]
fn queued_light_changes_preserve_one_revision_age_and_ingress_entry() {
    let mut stream = lit_stream(1);
    let key = SubChunkKey::new(1, 0, 0, 0);
    install_current_light(&mut stream, key, 0, 0, false);
    stream.mark_light_changed_sources([key]);
    let pending = stream.lighting.jobs.pending[&key];
    let generation = stream.lighting.block_generations[&key];
    for _ in 0..32 {
        stream.mark_light_changed_sources([key]);
    }
    assert!(stream.lighting.block_generations[&key] > generation);
    assert_eq!(
        stream.lighting.jobs.pending[&key].revision,
        pending.revision
    );
    assert_eq!(
        stream.lighting.jobs.pending[&key].queued_at,
        pending.queued_at
    );
    assert_eq!(stream.lighting.jobs.scan.len(), 1);
    assert_eq!(stream.lighting.revisions.entries.len(), 1);
    complete_one_light(&mut stream, [8.0; 3]);
    assert!(stream.light_is_current(key));
    assert_eq!(stream.stats.accepted_light_jobs, 1);
}

#[test]
fn queued_light_urgency_promotes_once_and_retains_dependency_wakeups() {
    let mut stream = lit_stream(1);
    let key = SubChunkKey::new(1, 0, 0, 0);
    let dependency = SubChunkKey::new(1, 1, 0, 0);
    install_current_light(&mut stream, key, 0, 0, false);
    let revision = stream.mark_light_dirty_exact(key).unwrap();
    let queued_at = stream.lighting.jobs.pending[&key].queued_at;
    stream
        .lighting
        .waiters
        .entry(dependency)
        .or_default()
        .insert(key);
    stream.lighting.priority_wakeups.insert(key, revision);
    for urgent in [false, true, true, false, true] {
        assert_eq!(
            stream.mark_light_dirty_exact_with_priority(key, urgent),
            Some(revision)
        );
    }
    assert!(stream.lighting.jobs.pending[&key].urgent);
    assert_eq!(stream.lighting.jobs.pending[&key].queued_at, queued_at);
    assert_eq!(stream.lighting.jobs.scan.len(), 2);
    assert_eq!(stream.lighting.priority_wakeups.get(&key), Some(&revision));
    assert!(stream.lighting.waiters[&dependency].contains(&key));
}

#[test]
fn first_in_flight_change_invalidates_snapshot_and_later_changes_share_successor() {
    let mut stream = lit_stream(1);
    let key = SubChunkKey::new(1, 0, 0, 0);
    install_current_light(&mut stream, key, 0, 0, false);
    let original = Arc::clone(stream.lighting.store.light(key).unwrap());
    let mut completion = synthetic_light_completion(
        &mut stream,
        key,
        DirectSkyMask::Uniform(false),
        false,
        false,
        [false; 6],
    );
    completion.identity.urgent = true;
    stream
        .lighting
        .jobs
        .in_flight
        .insert(key, completion.identity);
    stream.lighting.jobs.scan.clear();
    stream.mark_light_changed_sources([key]);
    let successor = stream.lighting.jobs.pending[&key];
    assert_ne!(successor.revision, completion.identity.revision);
    assert!(successor.urgent);
    for _ in 0..32 {
        stream.mark_light_changed_sources([key]);
    }
    assert_eq!(
        stream.lighting.jobs.pending[&key].revision,
        successor.revision
    );
    assert_eq!(stream.lighting.jobs.scan.len(), 1);
    stream.accept_light_completion(completion);
    assert_eq!(stream.stats.stale_light_jobs, 1);
    assert_eq!(
        stream.lighting.jobs.pending[&key].revision,
        successor.revision
    );
    complete_one_light(&mut stream, [8.0; 3]);
    assert!(stream.light_is_current(key));
    assert!(Arc::ptr_eq(
        stream.lighting.store.light(key).unwrap(),
        &original
    ));
    assert_eq!(stream.stats.noop_light_jobs, 1);
}

/// A pending seeded relight falls back to a full solve once a neighbour's relight changes the
/// light on their shared face.
#[test]
fn a_neighbour_relight_drops_pending_changed_cells_across_its_changed_face() {
    let mut stream = lit_stream(0);
    let west = SubChunkKey::new(0, 0, 0, 0);
    let east = SubChunkKey::new(0, 1, 0, 0);
    for key in [west, east] {
        stream
            .authority
            .commit_sub_chunk(key, uniform_sub_chunk(2))
            .unwrap();
        install_current_light(&mut stream, key, 0, 0, false);
    }
    for (key, cell) in [
        (west, BlockPos::new(8, 8, 8)),
        (east, BlockPos::new(24, 8, 8)),
    ] {
        stream.mark_light_changed_sources_reaching(
            [key],
            true,
            NeighbourReach::of_cell(8, 8, 8),
            Some(&[cell]),
        );
    }
    assert!(stream.lighting.changed_cells.contains_key(&west));
    assert!(stream.lighting.changed_cells.contains_key(&east));
    let mut changed_faces = [false; 6];
    changed_faces[LIGHT_NEIGHBOUR_OFFSETS
        .iter()
        .position(|&offset| offset_sub_chunk_key(west, offset) == Some(east))
        .unwrap()] = true;
    let direct_sky = stream.lighting.direct_sky[&west].clone();

    stream.finish_accepted_light_completion(west, 1, &direct_sky, changed_faces, true);

    assert!(stream.lighting.jobs.pending.contains_key(&east));
    assert!(!stream.lighting.changed_cells.contains_key(&east));
}
