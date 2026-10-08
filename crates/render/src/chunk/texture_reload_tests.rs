use super::*;

/// Uses original diagnostic pixels and distinct revisions without a GPU or external assets.
fn candidate(revision: u64) -> ChunkTextureAssets {
    ChunkTextureAssets::with_revision(Arc::new(assets::RuntimeAssets::diagnostic()), revision)
}

#[test]
fn stale_success_and_failure_cannot_complete_the_replacement_request() {
    let bridge = ChunkTextureReload::default();
    let first = candidate(1);
    let second = candidate(2);
    bridge.request(first.clone());
    bridge.request(second.clone());
    bridge.finish(first.identity(), true);
    assert!(bridge.status(first.identity()).is_none());
    assert!(bridge.status(second.identity()).is_none());
    bridge.finish(first.identity(), false);
    assert!(bridge.status(second.identity()).is_none());
    bridge.finish(second.identity(), true);
    assert!(matches!(bridge.status(second.identity()), Some(Ok(()))));
}

#[test]
fn repeated_request_preserves_completion_but_new_candidate_clears_it() {
    let bridge = ChunkTextureReload::default();
    let first = candidate(1);
    bridge.request(first.clone());
    bridge.finish(first.identity(), false);
    bridge.request(first.clone());
    assert!(matches!(bridge.status(first.identity()), Some(Err(_))));
    let second = candidate(2);
    bridge.request(second.clone());
    assert!(bridge.status(second.identity()).is_none());
}

#[test]
fn repeated_geometry_request_retains_the_exact_acknowledged_snapshot() {
    let bridge = ChunkTextureReload::default();
    let assets = candidate(1);
    let geometry: Arc<[ChunkRenderInstance]> = Arc::from([]);
    bridge.request_geometry(assets.clone(), geometry.clone());
    bridge.finish(assets.identity(), true);
    bridge.request_geometry(assets.clone(), Arc::from([]));
    assert!(Arc::ptr_eq(&bridge.geometry().unwrap(), &geometry));
    assert_eq!(bridge.status(assets.identity()), Some(Ok(())));
    assert!(bridge.geometry_pending());
}

#[test]
fn cancellation_releases_abandoned_cpu_assets_and_ignores_late_worker_finish() {
    let bridge = ChunkTextureReload::default();
    let current = candidate(1);
    let abandoned = candidate(2);
    let identity = abandoned.identity();
    let weak = Arc::downgrade(abandoned.assets());
    bridge.request(abandoned);
    assert!(weak.upgrade().is_some());
    bridge.cancel_except(current.identity());
    assert!(weak.upgrade().is_none());
    bridge.finish(identity, true);
    assert!(bridge.requested().is_none());
    assert!(bridge.status(identity).is_none());
}

#[test]
fn cancellation_preserves_published_candidate_until_render_can_consume_it() {
    let bridge = ChunkTextureReload::default();
    let published = candidate(1);
    bridge.request(published.clone());
    bridge.finish(published.identity(), true);
    bridge.cancel_except(published.identity());
    assert!(bridge.requested().is_some());
    assert!(matches!(bridge.status(published.identity()), Some(Ok(()))));
}
