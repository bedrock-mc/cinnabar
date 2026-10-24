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
    assert_eq!(recovery.failed_backends(), [Backend::Vulkan]);
    assert_eq!(recovery.working.as_ref().unwrap().backend, Backend::Dx12);
    recovery.succeeded(attempt(Backend::Vulkan, true));
    assert!(recovery.failed_backends().is_empty());
}

#[test]
fn successful_fallback_clears_the_sentinel_and_survives_relaunch() {
    let dir = tempfile::tempdir().unwrap();
    let mut first = StartupGuard::open(dir.path()).unwrap();
    first.begin(attempt(Backend::Dx12, false)).unwrap();
    drop(first); // Release the OS lease as an exited process would.
    let mut second = StartupGuard::open(dir.path()).unwrap();
    assert_eq!(second.interrupted, Some(attempt(Backend::Dx12, false)));
    assert_eq!(second.recovery.failed_backends(), [Backend::Dx12]);
    second.begin(attempt(Backend::Vulkan, false)).unwrap();
    second.presented().unwrap();
    assert!(StartupGuard::pending(dir.path()).unwrap().is_none());
    let third = StartupGuard::open(dir.path()).unwrap();
    assert!(third.interrupted.is_none());
    assert_eq!(third.recovery.working.as_ref().unwrap().stage, Stage::Ready);
    assert_eq!(third.recovery.failed_backends(), [Backend::Dx12]);
    assert_eq!(
        launcher::graphics_backend::candidates(
            launcher::graphics_backend::Platform::Windows.backends(),
            launcher::graphics_backend::Platform::Windows.backends(),
            None,
            None,
            third.recovery.failed_backends()
        ),
        [Backend::Vulkan]
    );
}

#[test]
fn concurrent_startup_cannot_consume_or_change_a_live_sentinel() {
    let dir = tempfile::tempdir().unwrap();
    let mut first = StartupGuard::open(dir.path()).unwrap();
    first.begin(attempt(Backend::Vulkan, false)).unwrap();
    let sentinel = fs::read(dir.path().join(SENTINEL_FILE)).unwrap();
    let choice = fs::read(dir.path().join(CHOICE_FILE)).unwrap();
    assert!(StartupGuard::open(dir.path()).is_err());
    assert_eq!(fs::read(dir.path().join(SENTINEL_FILE)).unwrap(), sentinel);
    assert_eq!(fs::read(dir.path().join(CHOICE_FILE)).unwrap(), choice);
    assert!(first.recovery.failed_backends().is_empty());
    first.presented().unwrap();
    let second = StartupGuard::open(dir.path()).unwrap();
    assert!(second.interrupted.is_none());
    assert!(second.recovery.failed_backends().is_empty());
}

#[test]
fn returned_errors_leave_automatic_backends_retryable() {
    let dir = tempfile::tempdir().unwrap();
    let mut guard = StartupGuard::open(dir.path()).unwrap();
    guard.begin(attempt(Backend::Vulkan, false)).unwrap();
    guard.failed().unwrap();
    drop(guard);
    let next = StartupGuard::open(dir.path()).unwrap();
    assert!(next.interrupted.is_none());
    assert!(next.recovery.failed_backends().is_empty());
    assert!(StartupGuard::pending(dir.path()).unwrap().is_none());
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
fn a_completed_guard_cannot_overwrite_the_next_startup() {
    let dir = tempfile::tempdir().unwrap();
    let mut first = StartupGuard::open(dir.path()).unwrap();
    first.begin(attempt(Backend::Vulkan, false)).unwrap();
    first.presented().unwrap();
    let mut second = StartupGuard::open(dir.path()).unwrap();
    let pending = attempt(Backend::Dx12, false);
    second.begin(pending.clone()).unwrap();
    assert!(first.begin(attempt(Backend::Gl, false)).is_err());
    assert!(first.refresh_settings(1, false).is_err());
    assert!(first.failed().is_err());
    assert!(first.retry_exclusions(&[Backend::Vulkan]).is_err());
    first.presented().unwrap();
    first.cancel().unwrap();
    assert_eq!(StartupGuard::pending(dir.path()).unwrap(), Some(pending));
}

#[test]
fn cancelling_releases_the_lease_without_changing_the_saved_choice() {
    let dir = tempfile::tempdir().unwrap();
    let mut first = StartupGuard::open(dir.path()).unwrap();
    first.begin(attempt(Backend::Gl, false)).unwrap();
    first.presented().unwrap();
    let mut cancelled = StartupGuard::open(dir.path()).unwrap();
    cancelled.begin(attempt(Backend::Vulkan, false)).unwrap();
    cancelled.cancel().unwrap();
    let mut next = StartupGuard::open(dir.path()).unwrap();
    assert!(next.interrupted.is_none());
    assert!(next.recovery.failed_backends().is_empty());
    assert_eq!(next.recovery.working.as_ref().unwrap().backend, Backend::Gl);
    let pending = attempt(Backend::Gl, false);
    next.begin(pending.clone()).unwrap();
    cancelled.cancel().unwrap();
    assert_eq!(StartupGuard::pending(dir.path()).unwrap(), Some(pending));
    assert!(cancelled.begin(attempt(Backend::Vulkan, false)).is_err());
}

#[test]
fn reports_do_not_attribute_another_process_or_reused_pid_to_this_client() {
    let dir = tempfile::tempdir().unwrap();
    let mut guard = StartupGuard::open(dir.path()).unwrap();
    guard.begin(attempt(Backend::Vulkan, false)).unwrap();
    let path = dir.path().join(SENTINEL_FILE);
    let mut sentinel: serde_json::Value =
        serde_json::from_slice(&fs::read(&path).unwrap()).unwrap();
    let owner = sentinel["owner"].clone();
    sentinel["owner"]["start_time"] =
        serde_json::json!(owner["start_time"].as_u64().unwrap().wrapping_add(1));
    fs::write(&path, serde_json::to_vec(&sentinel).unwrap()).unwrap();
    assert!(StartupGuard::pending(dir.path()).unwrap().is_none());
    sentinel["owner"] = owner;
    sentinel["owner"]["pid"] = serde_json::json!(std::process::id().wrapping_add(1));
    fs::write(&path, serde_json::to_vec(&sentinel).unwrap()).unwrap();
    assert!(StartupGuard::pending(dir.path()).unwrap().is_none());
}
