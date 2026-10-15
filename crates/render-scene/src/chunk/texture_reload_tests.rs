use super::*;
use crate::{ChunkRenderInstance, ChunkTextureAssets};

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
    let first_request = bridge.preparation().unwrap();
    bridge.request(second.clone());
    let second_request = bridge.preparation().unwrap();
    bridge.finish(&first_request, true);
    assert!(bridge.status(first.identity()).is_none());
    assert!(bridge.status(second.identity()).is_none());
    bridge.finish(&first_request, false);
    assert!(bridge.status(second.identity()).is_none());
    bridge.finish(&second_request, true);
    assert!(matches!(bridge.status(second.identity()), Some(Ok(()))));
}

#[test]
fn repeated_request_preserves_completion_but_new_candidate_clears_it() {
    let bridge = ChunkTextureReload::default();
    let first = candidate(1);
    bridge.request(first.clone());
    bridge.finish(&bridge.preparation().unwrap(), false);
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
    let request = bridge.preparation().unwrap();
    bridge.finish(&request, true);
    bridge.request_geometry(assets.clone(), Arc::from([]));
    assert!(bridge.is_current(&request));
    assert!(Arc::ptr_eq(&bridge.geometry().unwrap(), &geometry));
    assert_eq!(bridge.status(assets.identity()), Some(Ok(())));
    assert!(bridge.geometry_pending());
}

#[test]
fn cancellation_releases_mailbox_ownership_and_ignores_late_worker_finish() {
    let bridge = ChunkTextureReload::default();
    let current = candidate(1);
    let abandoned = candidate(2);
    let identity = abandoned.identity();
    let weak = Arc::downgrade(abandoned.assets());
    bridge.request(abandoned);
    let request = bridge.preparation().unwrap();
    assert!(weak.upgrade().is_some());
    bridge.cancel_except(current.identity());
    assert!(!bridge.is_current(&request));
    assert!(!bridge.finish(&request, true));
    drop(request);
    assert!(weak.upgrade().is_none());
    assert!(bridge.requested().is_none());
    assert!(bridge.status(identity).is_none());
}

#[test]
fn cancellation_preserves_published_candidate_until_render_can_consume_it() {
    let bridge = ChunkTextureReload::default();
    let published = candidate(1);
    bridge.request(published.clone());
    bridge.finish(&bridge.preparation().unwrap(), true);
    bridge.cancel_except(published.identity());
    assert!(bridge.requested().is_some());
    assert!(matches!(bridge.status(published.identity()), Some(Ok(()))));
}

#[test]
fn obsolete_preparation_cannot_complete_retried_geometry_with_same_assets() {
    for succeeded in [true, false] {
        let bridge = ChunkTextureReload::default();
        let current = candidate(0);
        let assets = candidate(1);
        bridge.request_geometry(assets.clone(), Arc::from([]));
        let cached = bridge.preparation().unwrap();
        bridge.finish(&cached, true);
        bridge.cancel_except(current.identity());
        assert!(bridge.geometry().is_none());
        bridge.request_geometry(assets.clone(), Arc::from([]));
        assert!(!bridge.finish(&cached, succeeded));
        assert!(bridge.status(assets.identity()).is_none());
        bridge.published(&cached);
        let replacement = bridge.preparation().unwrap();
        assert!(replacement.geometry().is_some());
        assert!(bridge.finish(&replacement, true));
        assert_eq!(bridge.status(assets.identity()), Some(Ok(())));
    }
}

#[test]
fn obsolete_preparation_cannot_complete_geometry_added_to_same_atlas() {
    let bridge = ChunkTextureReload::default();
    let assets = candidate(1);
    bridge.request(assets.clone());
    let cached = bridge.preparation().unwrap();
    let geometry = Arc::from([]);
    bridge.request_geometry(assets.clone(), Arc::clone(&geometry));
    let replacement = bridge.preparation().unwrap();
    assert!(cached.geometry().is_none());
    assert!(Arc::ptr_eq(replacement.geometry().unwrap(), &geometry));
    assert!(!bridge.finish(&cached, true));
    assert!(bridge.status(assets.identity()).is_none());
    assert!(bridge.finish(&replacement, true));
    assert_eq!(bridge.status(assets.identity()), Some(Ok(())));
}

#[test]
fn discarding_geometry_retains_the_atlas_attempt_until_new_geometry_is_queued() {
    let bridge = ChunkTextureReload::default();
    let assets = candidate(1);
    bridge.request_geometry(assets.clone(), Arc::from([]));
    let geometry = bridge.preparation().unwrap();
    bridge.discard_geometry();
    let atlas = bridge.preparation().unwrap();
    assert!(geometry.geometry().is_some());
    assert!(atlas.geometry().is_none());
    assert_eq!(atlas.assets().identity(), assets.identity());
    assert!(bridge.finish(&geometry, true));
    assert!(bridge.is_current(&atlas));
    assert_eq!(bridge.status(assets.identity()), Some(Ok(())));
    bridge.request_geometry(assets.clone(), Arc::from([]));
    assert!(!bridge.finish(&atlas, true));
    bridge.published(&atlas);
    assert!(bridge.preparation().unwrap().geometry().is_some());
    assert!(bridge.status(assets.identity()).is_none());
}
