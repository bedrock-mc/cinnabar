use super::*;

/// Records an identified device failure without loading a native driver.
fn attempt(backend: Backend, adapter: Option<Adapter>) -> Attempt {
    Attempt {
        backend,
        adapter,
        stage: Stage::CreatingDevice,
        explicit: false,
    }
}

/// Identifies one synthetic adapter and its installed driver.
fn adapter() -> Adapter {
    Adapter {
        name: "test GPU".into(),
        vendor: 1,
        device: 2,
        driver: "driver".into(),
        driver_info: "version 1".into(),
        driver_version: "1".into(),
        integrated: false,
    }
}

#[test]
fn device_exclusions_expire_after_gpu_or_driver_changes_and_stay_expired() {
    let original = adapter();
    for replacement in [
        Adapter {
            driver_version: "2".into(),
            ..original.clone()
        },
        Adapter {
            driver_info: "version 2".into(),
            ..original.clone()
        },
        Adapter {
            vendor: 3,
            ..original.clone()
        },
        Adapter {
            device: 4,
            ..original.clone()
        },
    ] {
        let root = tempfile::tempdir().unwrap();
        let mut first = StartupGuard::open(root.path()).unwrap();
        first
            .begin(attempt(Backend::Vulkan, Some(original.clone())))
            .unwrap();
        drop(first);
        let mut next = StartupGuard::open(root.path()).unwrap();
        assert!(next.recovery.enumeration_exclusions().is_empty());
        next.begin(attempt(Backend::Vulkan, None)).unwrap();
        assert!(next.adapter_excluded(Backend::Vulkan, &original).unwrap());
        assert!(
            !next
                .adapter_excluded(Backend::Vulkan, &replacement)
                .unwrap()
        );
        next.failed().unwrap();
        drop(next);
        let reopened = StartupGuard::open(root.path()).unwrap();
        assert!(reopened.recovery.failed_backends().is_empty());
    }
}

#[test]
fn comparing_different_apis_does_not_clear_an_unchanged_device_failure() {
    let root = tempfile::tempdir().unwrap();
    let original = adapter();
    let mut first = StartupGuard::open(root.path()).unwrap();
    first
        .begin(attempt(Backend::Vulkan, Some(original.clone())))
        .unwrap();
    drop(first);
    let mut next = StartupGuard::open(root.path()).unwrap();
    let other_api = Adapter {
        driver: "other API driver".into(),
        ..original.clone()
    };
    assert!(!next.adapter_excluded(Backend::Dx12, &other_api).unwrap());
    assert!(next.adapter_excluded(Backend::Vulkan, &original).unwrap());
}

#[test]
fn exhausted_legacy_exclusions_are_cleared_once_and_persisted() {
    let root = tempfile::tempdir().unwrap();
    fs::write(
        root.path().join(CHOICE_FILE),
        br#"{"working":null,"failed":["vulkan","dx12","gl"]}"#,
    )
    .unwrap();
    let mut guard = StartupGuard::open(root.path()).unwrap();
    let allowed = [Backend::Vulkan, Backend::Dx12];
    assert!(
        allowed
            .iter()
            .all(|api| guard.recovery.enumeration_exclusions().contains(api))
    );
    assert!(guard.retry_exclusions(&allowed).unwrap());
    assert!(!guard.retry_exclusions(&allowed).unwrap());
    assert_eq!(guard.recovery.failed_backends(), [Backend::Gl]);
    drop(guard);
    let reopened = StartupGuard::open(root.path()).unwrap();
    assert_eq!(reopened.recovery.failed_backends(), [Backend::Gl]);
}

#[test]
fn exhausted_identified_failures_allow_one_retry_of_the_same_drivers() {
    let root = tempfile::tempdir().unwrap();
    let allowed = [Backend::Vulkan, Backend::Dx12];
    for backend in allowed {
        let mut guard = StartupGuard::open(root.path()).unwrap();
        guard.begin(attempt(backend, Some(adapter()))).unwrap();
    }
    let mut guard = StartupGuard::open(root.path()).unwrap();
    for backend in allowed {
        assert!(guard.adapter_excluded(backend, &adapter()).unwrap());
    }
    assert!(guard.retry_exclusions(&allowed).unwrap());
    for backend in allowed {
        assert!(!guard.adapter_excluded(backend, &adapter()).unwrap());
    }
    assert!(!guard.retry_exclusions(&allowed).unwrap());
}
