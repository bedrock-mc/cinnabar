use super::*;

/// Builds a pending attempt without requiring a native GPU.
fn attempt(backend: Backend, explicit: bool) -> Attempt {
    Attempt {
        backend,
        adapter: None,
        stage: Stage::CreatingDevice,
        explicit,
    }
}

#[test]
fn crash_guard_accumulates_finite_failures_and_overrides_do_not_poison_auto() {
    let mut recovery = Recovery::default();
    for backend in Backend::ALL {
        recovery.recover(&attempt(*backend, false));
        recovery.recover(&attempt(*backend, false));
    }
    assert_eq!(recovery.failed_backends(), Backend::ALL);
    recovery.reset();
    recovery.recover(&attempt(Backend::Vulkan, true));
    assert!(recovery.failed_backends().is_empty());
    recovery.recover(&attempt(Backend::Vulkan, false));
    recovery.succeeded(attempt(Backend::Dx12, false));
    assert!(recovery.failed_backends().is_empty());
    assert_eq!(recovery.working.unwrap().backend, Backend::Dx12);
}

#[test]
fn successful_fallback_clears_the_sentinel_and_survives_relaunch() {
    let dir = tempfile::tempdir().unwrap();
    let mut first = StartupGuard::open(dir.path()).unwrap();
    first.begin(attempt(Backend::Vulkan, false)).unwrap();
    let mut second = StartupGuard::open(dir.path()).unwrap();
    assert_eq!(second.interrupted, Some(attempt(Backend::Vulkan, false)));
    assert_eq!(second.recovery.failed_backends(), [Backend::Vulkan]);
    second.begin(attempt(Backend::Dx12, false)).unwrap();
    second.presented().unwrap();
    assert!(StartupGuard::pending(dir.path()).unwrap().is_none());
    let third = StartupGuard::open(dir.path()).unwrap();
    assert!(third.interrupted.is_none());
    assert_eq!(third.recovery.working.as_ref().unwrap().stage, Stage::Ready);
    assert!(third.recovery.failed_backends().is_empty());
}

#[test]
fn malformed_or_oversized_state_does_not_start_a_gpu_attempt() {
    let dir = tempfile::tempdir().unwrap();
    let sentinel = dir.path().join(SENTINEL_FILE);
    fs::write(&sentinel, b"broken JSON").unwrap();
    assert!(StartupGuard::open(dir.path()).is_err());
    fs::write(&sentinel, vec![b' '; MAX_FILE_BYTES as usize + 1]).unwrap();
    assert!(StartupGuard::open(dir.path()).is_err());
}

#[test]
fn sentinel_retains_adapter_and_the_operation_before_a_native_call() {
    let dir = tempfile::tempdir().unwrap();
    let mut guard = StartupGuard::open(dir.path()).unwrap();
    let mut pending = attempt(Backend::Dx12, false);
    pending.adapter = Some(Adapter {
        name: "test GPU".into(),
        vendor: 1,
        device: 2,
        driver: "driver".into(),
        driver_info: "version".into(),
        driver_version: String::new(),
        integrated: false,
    });
    pending.stage = Stage::FirstPresentation;
    guard.begin(pending.clone()).unwrap();
    assert_eq!(StartupGuard::pending(dir.path()).unwrap(), Some(pending));
}

#[test]
fn probe_verdict_is_saved_under_the_data_root_and_survives_restart() {
    use launcher::graphics_backend::Verdict;
    let dir = tempfile::tempdir().unwrap();
    let key = Adapter {
        name: "test".into(),
        vendor: 1,
        device: 2,
        driver: "1.2".into(),
        driver_info: "version".into(),
        driver_version: String::new(),
        integrated: false,
    }
    .key();
    let mut cache = load_probe_cache(dir.path()).unwrap();
    cache.record(key.clone(), Backend::Gl, Verdict::TimedOut);
    save_probe_cache(dir.path(), &cache).unwrap();
    assert_eq!(
        load_probe_cache(dir.path()).unwrap().get(&key, Backend::Gl),
        Some(Verdict::TimedOut)
    );
}
