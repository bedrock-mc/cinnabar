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
    bridge.request(second.clone());
    bridge.finish(first.identity(), bridge.geometry_epoch(), true);
    assert!(bridge.status(first.identity()).is_none());
    assert!(bridge.status(second.identity()).is_none());
    bridge.finish(first.identity(), bridge.geometry_epoch(), false);
    assert!(bridge.status(second.identity()).is_none());
    bridge.finish(second.identity(), bridge.geometry_epoch(), true);
    assert!(matches!(bridge.status(second.identity()), Some(Ok(()))));
}

#[test]
fn repeated_request_preserves_completion_but_new_candidate_clears_it() {
    let bridge = ChunkTextureReload::default();
    let first = candidate(1);
    bridge.request(first.clone());
    bridge.finish(first.identity(), bridge.geometry_epoch(), false);
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
    bridge.finish(assets.identity(), bridge.geometry_epoch(), true);
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
    bridge.finish(identity, bridge.geometry_epoch(), true);
    assert!(bridge.requested().is_none());
    assert!(bridge.status(identity).is_none());
}

#[test]
fn cancellation_preserves_published_candidate_until_render_can_consume_it() {
    let bridge = ChunkTextureReload::default();
    let published = candidate(1);
    bridge.request(published.clone());
    bridge.finish(published.identity(), bridge.geometry_epoch(), true);
    bridge.cancel_except(published.identity());
    assert!(bridge.requested().is_some());
    assert!(matches!(bridge.status(published.identity()), Some(Ok(()))));
}

/// A reset invalidates readiness from the retired snapshot before main-world selection.
#[test]
fn discarding_geometry_revokes_its_readiness_without_holding_the_next_session() {
    let bridge = ChunkTextureReload::default();
    let assets = candidate(3);
    bridge.request_geometry(assets.clone(), Arc::from([]));
    bridge.finish(assets.identity(), bridge.geometry_epoch(), true);
    assert_eq!(bridge.status(assets.identity()), Some(Ok(())));
    bridge.discard_geometry();
    assert!(matches!(bridge.status(assets.identity()), Some(Err(_))));
    assert!(bridge.geometry().is_none());
    assert!(!bridge.geometry_pending());
    assert_eq!(bridge.requested().unwrap().identity(), assets.identity());
}

/// An older publication cannot consume a newer immutable preparation request.
#[test]
fn publication_acknowledges_only_its_matching_request_and_snapshot_epoch() {
    let bridge = ChunkTextureReload::default();
    let first = candidate(1);
    let second = candidate(2);
    bridge.request_geometry(first.clone(), Arc::from([]));
    let original = bridge.preparation().unwrap();
    bridge.request_geometry(second.clone(), Arc::from([]));
    bridge.published(first.identity(), original.epoch);
    let next = bridge.preparation().unwrap();
    assert_eq!(next.assets.identity(), second.identity());
    assert_eq!(
        next.epoch, original.epoch,
        "a new request is still in the same session"
    );
    assert!(bridge.geometry_pending());
    bridge.discard_geometry();
    assert_ne!(bridge.geometry_epoch(), original.epoch);
    bridge.finish(second.identity(), original.epoch, true);
    assert!(matches!(bridge.status(second.identity()), Some(Err(_))));
    bridge.published(second.identity(), original.epoch);
    assert!(
        bridge.requested().is_some(),
        "an old epoch cannot consume the reset request"
    );
    bridge.published(second.identity(), bridge.geometry_epoch());
    assert!(bridge.requested().is_none());
}
