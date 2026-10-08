use super::*;

/// Independent wire allocations share contents but never source-pointer identities.
fn sources(count: usize) -> Vec<Arc<SkinGeometrySource>> {
    (0..count)
        .map(|_| super::super::tests::source(super::super::tests::MODEL))
        .collect()
}

#[test]
fn cold_queue_admission_is_bounded_and_never_computes_inline() {
    let sources = sources(MAX_SKIN_PREPARATIONS_PER_PASS * 2);
    let mut queue = SkinPreparationQueue::default();
    queue.begin_frame();
    for source in &sources {
        assert!(!queue.request(source));
    }
    assert_eq!(queue.queued.len(), MAX_SKIN_PREPARATIONS_PER_PASS);
    assert_eq!(queue.entries.len(), MAX_SKIN_PREPARATIONS_PER_PASS);
    assert_eq!(
        queue.source_bytes,
        sources[0].byte_len() * MAX_SKIN_PREPARATIONS_PER_PASS
    );
    assert!(queue.entries.values().all(|entry| entry.outcome.is_none()));
}

#[test]
fn worker_content_reuse_shares_geometry_and_mesh_without_an_inline_fallback() {
    let sources = sources(MAX_SKIN_PREPARATIONS_PER_PASS);
    let mut queue = SkinPreparationQueue::default();
    assert!(!queue.is_pending());
    for source in &sources {
        queue.request(source);
    }
    assert!(queue.is_pending());
    queue.submit(&super::super::super::render_frame::tests::counting_random_assets());
    assert!(queue.is_pending());
    for source in &sources {
        assert!(queue.get(source).is_none());
    }
    queue.finish_fixture_batch();
    assert!(!queue.is_pending());
    let first = queue.get(&sources[0]).unwrap().0.unwrap();
    for source in &sources {
        assert!(queue.request(source));
        let next = queue.get(source).unwrap().0.unwrap();
        assert!(Arc::ptr_eq(&first.geometry, &next.geometry));
        assert!(Arc::ptr_eq(
            &first.mesh.as_ref().unwrap().vertices,
            &next.mesh.as_ref().unwrap().vertices
        ));
    }
    assert!(queue.queued.is_empty(), "unchanged sources enqueue no work");
}

#[test]
fn old_worker_channels_cannot_complete_a_new_owner_with_the_same_source() {
    let source = sources(1).pop().unwrap();
    let mut old = SkinPreparationQueue::default();
    old.request(&source);
    old.submit(&super::super::super::render_frame::tests::counting_random_assets());
    let old_receiver = old.receiver.take().unwrap().into_inner().unwrap();
    drop(old);
    let mut next = SkinPreparationQueue::default();
    next.request(&source);
    let _ = old_receiver.recv();
    assert!(next.get(&source).is_none());
    assert_eq!(next.queued.len(), 1);
}

#[test]
fn equal_sources_charge_one_mesh_allocation_at_the_capacity_boundary() {
    let sources = sources(MAX_SKIN_PREPARATIONS_PER_PASS);
    let assets = super::super::super::render_frame::tests::counting_random_assets();
    let mut worker = WorkerCache::default();
    let first = worker.prepare(&sources[0], &assets).0.unwrap();
    let mut queue = SkinPreparationQueue::default();
    queue.mesh_budget = first.mesh_bytes();
    queue.cache = Some(worker);
    for source in &sources {
        queue.request(source);
    }
    queue.submit(&assets);
    queue.finish_fixture_batch();
    for source in &sources {
        assert!(queue.get(source).is_some());
    }
    assert_eq!(queue.mesh_bytes, first.mesh_bytes());
    assert_eq!(queue.allocations.len(), 1);
    queue.begin_frame();
    queue.begin_frame();
    queue.begin_frame();
    assert_eq!(queue.mesh_bytes, 0);
    assert!(queue.allocations.is_empty());
}

#[test]
fn equal_replacement_reuses_the_ready_result_after_worker_memo_eviction() {
    let sources = sources(2);
    let assets = super::super::super::render_frame::tests::counting_random_assets();
    let mut queue = SkinPreparationQueue::default();
    queue.request(&sources[0]);
    queue.submit(&assets);
    queue.finish_fixture_batch();
    let first = queue.get(&sources[0]).unwrap().0.unwrap();
    queue.cache = Some(WorkerCache::default());
    assert!(!queue.request_replacing(&sources[1], Some(&sources[0])));
    assert_eq!(
        queue.entries[&(Arc::as_ptr(&sources[0]) as usize)].in_flight_references,
        1
    );
    queue.submit(&assets);
    queue.finish_fixture_batch();
    let next = queue.get(&sources[1]).unwrap().0.unwrap();
    assert!(Arc::ptr_eq(&first, &next));
    assert!(queue.replaces_unchanged(&sources[1], &sources[0]));
    assert_eq!(
        queue.entries[&(Arc::as_ptr(&sources[0]) as usize)].in_flight_references,
        0
    );
}

#[test]
fn a_full_ready_population_can_admit_a_replacement_without_retiring_its_old_appearance() {
    let sources = sources(crate::actor_store::MAX_TRACKED_ACTORS + 1);
    let mut queue = SkinPreparationQueue::default();
    for source in &sources[..crate::actor_store::MAX_TRACKED_ACTORS] {
        queue.source_bytes += source.byte_len();
        queue.entries.insert(
            Arc::as_ptr(source) as usize,
            Entry {
                source: Arc::clone(source),
                outcome: Some((None, false)),
                seen: 0,
                allocation: None,
                in_flight_references: 0,
                unchanged_from: None,
            },
        );
    }
    let replacement = sources.last().unwrap();
    assert!(!queue.request_replacing(replacement, Some(&sources[0])));
    assert_eq!(queue.queued.len(), 1);
    assert_eq!(
        queue.entries.len(),
        crate::actor_store::MAX_TRACKED_ACTORS + 1
    );
    assert_eq!(
        queue.entries[&(Arc::as_ptr(&sources[0]) as usize)].in_flight_references,
        1
    );
}

#[test]
fn mesh_budget_rejection_is_ready_and_never_requeues_unchanged_sources() {
    let source = sources(1).pop().unwrap();
    let assets = super::super::super::render_frame::tests::counting_random_assets();
    let mut queue = SkinPreparationQueue::default();
    queue.mesh_budget = 0;
    assert!(!queue.request(&source));
    queue.submit(&assets);
    queue.finish_fixture_batch();
    for _ in 0..4 {
        queue.begin_frame();
        assert!(
            queue.request(&source),
            "budget fallback completes appearance readiness"
        );
        let (prepared, rejected) = queue.get(&source).unwrap();
        assert!(prepared.is_none(), "fallback retains no over-budget mesh");
        assert!(rejected);
        assert!(
            queue.queued.is_empty(),
            "unchanged input queues no more preparation"
        );
    }
    assert_eq!(queue.mesh_bytes, 0);
    assert!(queue.allocations.is_empty());
    assert_eq!(queue.source_bytes, source.byte_len());
}
