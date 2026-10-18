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
    assert!(bridge.status(first.binding_identity()).is_none());
    assert!(bridge.status(second.binding_identity()).is_none());
    bridge.finish(&first_request, false);
    assert!(bridge.status(second.binding_identity()).is_none());
    bridge.finish(&second_request, true);
    assert!(matches!(
        bridge.status(second.binding_identity()),
        Some(Ok(()))
    ));
}

#[test]
fn repeated_request_preserves_completion_but_new_candidate_clears_it() {
    let bridge = ChunkTextureReload::default();
    let first = candidate(1);
    bridge.request(first.clone());
    bridge.finish(&bridge.preparation().unwrap(), false);
    bridge.request(first.clone());
    assert!(matches!(
        bridge.status(first.binding_identity()),
        Some(Err(_))
    ));
    let second = candidate(2);
    bridge.request(second.clone());
    assert!(bridge.status(second.binding_identity()).is_none());
}

#[test]
fn repeated_geometry_request_retains_the_exact_acknowledged_snapshot() {
    let bridge = ChunkTextureReload::default();
    let assets = candidate(1);
    let geometry: Arc<[ChunkRenderInstance]> = Arc::from([]);
    bridge.request_geometry(bridge.geometry_epoch(), assets.clone(), geometry.clone());
    let request = bridge.preparation().unwrap();
    bridge.finish(&request, true);
    bridge.request_geometry(bridge.geometry_epoch(), assets.clone(), Arc::from([]));
    assert!(bridge.is_current(&request));
    assert!(Arc::ptr_eq(&bridge.geometry().unwrap(), &geometry));
    assert_eq!(bridge.status(assets.binding_identity()), Some(Ok(())));
    assert!(bridge.geometry_pending());
}

#[test]
fn cancellation_releases_mailbox_ownership_and_ignores_late_worker_finish() {
    let bridge = ChunkTextureReload::default();
    let current = candidate(1);
    let abandoned = candidate(2);
    let identity = abandoned.binding_identity();
    let weak = Arc::downgrade(abandoned.assets());
    bridge.request(abandoned);
    let request = bridge.preparation().unwrap();
    assert!(weak.upgrade().is_some());
    bridge.cancel_except(current.binding_identity());
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
    bridge.cancel_except(published.binding_identity());
    assert!(bridge.requested().is_some());
    assert!(matches!(
        bridge.status(published.binding_identity()),
        Some(Ok(()))
    ));
}

#[test]
fn obsolete_preparation_cannot_complete_retried_geometry_with_same_assets() {
    for succeeded in [true, false] {
        let bridge = ChunkTextureReload::default();
        let current = candidate(0);
        let assets = candidate(1);
        bridge.request_geometry(bridge.geometry_epoch(), assets.clone(), Arc::from([]));
        let cached = bridge.preparation().unwrap();
        bridge.finish(&cached, true);
        bridge.cancel_except(current.binding_identity());
        assert!(bridge.geometry().is_none());
        bridge.request_geometry(bridge.geometry_epoch(), assets.clone(), Arc::from([]));
        assert!(!bridge.finish(&cached, succeeded));
        assert!(bridge.status(assets.binding_identity()).is_none());
        bridge.published(&cached);
        let replacement = bridge.preparation().unwrap();
        assert!(replacement.geometry().is_some());
        assert!(bridge.finish(&replacement, true));
        assert_eq!(bridge.status(assets.binding_identity()), Some(Ok(())));
    }
}

#[test]
fn obsolete_preparation_cannot_complete_geometry_added_to_same_atlas() {
    let bridge = ChunkTextureReload::default();
    let assets = candidate(1);
    bridge.request(assets.clone());
    let cached = bridge.preparation().unwrap();
    let geometry = Arc::from([]);
    bridge.request_geometry(
        bridge.geometry_epoch(),
        assets.clone(),
        Arc::clone(&geometry),
    );
    let replacement = bridge.preparation().unwrap();
    assert!(cached.geometry().is_none());
    assert!(Arc::ptr_eq(replacement.geometry().unwrap(), &geometry));
    assert!(!bridge.finish(&cached, true));
    assert!(bridge.status(assets.binding_identity()).is_none());
    assert!(bridge.finish(&replacement, true));
    assert_eq!(bridge.status(assets.binding_identity()), Some(Ok(())));
}

/// A reset invalidates readiness from the retired snapshot before main-world selection.
#[test]
fn discarding_geometry_revokes_its_readiness_without_holding_the_next_session() {
    let bridge = ChunkTextureReload::default();
    let assets = candidate(3);
    bridge.request_geometry(bridge.geometry_epoch(), assets.clone(), Arc::from([]));
    bridge.finish(&bridge.preparation().unwrap(), true);
    assert_eq!(bridge.status(assets.binding_identity()), Some(Ok(())));
    bridge.discard_geometry();
    assert!(bridge.status(assets.binding_identity()).is_none());
    assert!(bridge.geometry().is_none());
    assert!(!bridge.geometry_pending());
    assert!(bridge.requested().is_none());
}

/// An older publication cannot consume a newer immutable preparation request.
#[test]
fn publication_acknowledges_only_its_matching_attempt_after_a_reset() {
    let bridge = ChunkTextureReload::default();
    let first = candidate(1);
    let second = candidate(2);
    bridge.request_geometry(bridge.geometry_epoch(), first.clone(), Arc::from([]));
    let original = bridge.preparation().unwrap();
    bridge.request_geometry(bridge.geometry_epoch(), second.clone(), Arc::from([]));
    bridge.published(&original);
    let next = bridge.preparation().unwrap();
    assert_eq!(next.assets().binding_identity(), second.binding_identity());
    assert_eq!(
        next.epoch, original.epoch,
        "a new request is still in the same session"
    );
    assert!(bridge.geometry_pending());
    bridge.discard_geometry();
    assert_ne!(bridge.geometry_epoch(), original.epoch);
    assert!(!bridge.finish(&next, true));
    bridge.request_geometry(bridge.geometry_epoch(), second.clone(), Arc::from([]));
    bridge.published(&next);
    assert!(
        bridge.requested().is_some(),
        "an old publication cannot consume the new session request"
    );
}

/// Old CPU completions cannot acquire the new world's mesh hold.
#[test]
fn stale_geometry_cannot_enter_or_hold_a_new_generation() {
    let bridge = ChunkTextureReload::default();
    let epoch = bridge.geometry_epoch();
    bridge.discard_geometry();
    bridge.hold_geometry(epoch);
    bridge.request_geometry(epoch, candidate(1), Arc::from([]));
    assert!(!bridge.geometry_pending());
    assert!(bridge.preparation().is_none());
}
