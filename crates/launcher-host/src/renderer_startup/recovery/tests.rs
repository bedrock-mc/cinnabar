use super::*;

/// Models an interrupted API probe without requiring a native driver.
fn attempt(backend: Backend) -> Attempt {
    Attempt {
        backend,
        adapter: None,
        stage: Stage::EnumeratingAdapters,
        explicit: false,
    }
}

#[test]
fn version_change_clears_exclusions_and_does_not_recover_an_old_sentinel() {
    let root = tempfile::tempdir().unwrap();
    let mut first = StartupGuard::open_version(root.path(), "one").unwrap();
    first.begin(attempt(Backend::Vulkan)).unwrap();
    drop(first);
    let mut same = StartupGuard::open_version(root.path(), "one").unwrap();
    assert_eq!(same.recovery.failed_backends(), [Backend::Vulkan]);
    same.begin(attempt(Backend::Dx12)).unwrap();
    drop(same);
    let next = StartupGuard::open_version(root.path(), "two").unwrap();
    assert!(next.interrupted.is_none());
    assert!(next.recovery.failed_backends().is_empty());
    assert!(StartupGuard::pending(root.path()).unwrap().is_none());
    drop(next);
    let reopened = StartupGuard::open_version(root.path(), "two").unwrap();
    assert!(reopened.recovery.failed_backends().is_empty());
}

#[test]
fn graphics_api_selection_resets_exclusions_and_persists_the_revision() {
    let root = tempfile::tempdir().unwrap();
    let mut first = StartupGuard::open(root.path()).unwrap();
    first.refresh_settings(1, false).unwrap();
    first.begin(attempt(Backend::Vulkan)).unwrap();
    drop(first);
    let mut next = StartupGuard::open(root.path()).unwrap();
    next.refresh_settings(1, false).unwrap();
    assert_eq!(next.recovery.failed_backends(), [Backend::Vulkan]);
    next.refresh_settings(2, false).unwrap();
    assert!(next.interrupted.is_none());
    assert!(next.recovery.failed_backends().is_empty());
    next.begin(attempt(Backend::Dx12)).unwrap();
    drop(next);
    let mut reopened = StartupGuard::open(root.path()).unwrap();
    reopened.refresh_settings(2, false).unwrap();
    assert_eq!(reopened.recovery.failed_backends(), [Backend::Dx12]);
    reopened.refresh_settings(3, false).unwrap();
    assert!(reopened.recovery.failed_backends().is_empty());
}

#[test]
fn explicit_choice_keeps_automatic_crash_exclusions_for_its_auto_fallback() {
    let root = tempfile::tempdir().unwrap();
    let mut first = StartupGuard::open(root.path()).unwrap();
    first.begin(attempt(Backend::Vulkan)).unwrap();
    drop(first);
    let mut next = StartupGuard::open(root.path()).unwrap();
    next.refresh_settings(1, true).unwrap();
    assert_eq!(next.recovery.failed_backends(), [Backend::Vulkan]);
    next.refresh_settings(2, false).unwrap();
    assert!(next.recovery.failed_backends().is_empty());
}

#[test]
fn crashed_explicit_choice_is_remembered_until_the_setting_changes_or_succeeds() {
    let root = tempfile::tempdir().unwrap();
    let explicit = |backend| Attempt {
        explicit: true,
        ..attempt(backend)
    };
    let mut first = StartupGuard::open(root.path()).unwrap();
    first.refresh_settings(1, true).unwrap();
    first.begin(explicit(Backend::Dx12)).unwrap();
    drop(first);
    for _ in 0..2 {
        let mut next = StartupGuard::open(root.path()).unwrap();
        next.refresh_settings(1, true).unwrap();
        assert_eq!(next.recovery.crashed_choice(), Some(Backend::Dx12));
        assert!(next.recovery.failed_backends().is_empty());
    }
    let mut changed = StartupGuard::open(root.path()).unwrap();
    changed.refresh_settings(2, true).unwrap();
    assert_eq!(changed.recovery.crashed_choice(), None);
    changed.recovery.recover(&explicit(Backend::Dx12));
    changed.recovery.succeeded(explicit(Backend::Dx12));
    assert_eq!(changed.recovery.crashed_choice(), None);
}

#[test]
fn exhausted_candidates_can_clear_partial_exclusions_only_once() {
    let root = tempfile::tempdir().unwrap();
    let mut guard = StartupGuard::open(root.path()).unwrap();
    let allowed = [Backend::Vulkan, Backend::Dx12];
    guard.recovery.recover(&attempt(Backend::Vulkan));
    guard.recovery.recover(&attempt(Backend::Gl));
    assert!(guard.retry_exclusions(&allowed).unwrap());
    assert_eq!(guard.recovery.failed_backends(), [Backend::Gl]);
    assert_eq!(
        StartupGuard::saved(root.path()).unwrap().failed_backends(),
        [Backend::Gl]
    );
    for backend in allowed {
        guard.recovery.recover(&attempt(backend));
    }
    assert!(!guard.retry_exclusions(&allowed).unwrap());
    assert!(!guard.retry_exclusions(&[]).unwrap());
}

#[test]
fn lone_backend_exclusion_has_the_same_bounded_retry() {
    let root = tempfile::tempdir().unwrap();
    let mut guard = StartupGuard::open(root.path()).unwrap();
    guard.recovery.recover(&attempt(Backend::Metal));
    assert!(guard.retry_exclusions(&[Backend::Metal]).unwrap());
    guard.recovery.recover(&attempt(Backend::Metal));
    assert!(!guard.retry_exclusions(&[Backend::Metal]).unwrap());
}

#[test]
fn legacy_identity_fields_are_discarded_on_version_migration() {
    let root = tempfile::tempdir().unwrap();
    fs::write(
        root.path().join(CHOICE_FILE),
        br#"{"working":null,"failed":["vulkan"],"direct_driver":"old","failed_adapters":[]}"#,
    )
    .unwrap();
    let guard = StartupGuard::open(root.path()).unwrap();
    assert!(guard.recovery.failed_backends().is_empty());
    assert_eq!(guard.recovery.version, launcher::PRODUCT_VERSION);
}
