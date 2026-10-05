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
