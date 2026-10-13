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
fn fresh_start_and_crashed_launch_use_a_bounded_backend_order() {
    let mut policy = Recovery::default();
    assert_eq!(policy.candidates(&Backend::ORDER, None), Backend::ORDER);
    policy.recover(&attempt(Backend::Vulkan, false));
    policy.recover(&attempt(Backend::Vulkan, false));
    assert_eq!(
        policy.candidates(&Backend::ORDER, None),
        [Backend::Dx12, Backend::Gl]
    );
    policy.recover(&attempt(Backend::Dx12, false));
    assert_eq!(policy.candidates(&Backend::ORDER, None), [Backend::Gl]);
    policy.recover(&attempt(Backend::Gl, false));
    assert!(policy.candidates(&Backend::ORDER, None).is_empty());
}

#[test]
fn working_dx12_failure_tries_vulkan_next() {
    let mut policy = Recovery::default();
    policy.succeeded(attempt(Backend::Dx12, false));
    assert_eq!(
        policy.candidates(&Backend::ORDER, None),
        [Backend::Dx12, Backend::Vulkan, Backend::Gl]
    );
    policy.recover(&attempt(Backend::Dx12, false));
    assert_eq!(
        policy.candidates(&Backend::ORDER, None),
        [Backend::Vulkan, Backend::Gl]
    );
}

#[test]
fn explicit_override_wins_without_poisoning_automatic_recovery() {
    let mut policy = Recovery::default();
    policy.recover(&attempt(Backend::Vulkan, false));
    assert_eq!(
        policy.candidates(&Backend::ORDER, Some(&[Backend::Vulkan])),
        [Backend::Vulkan]
    );
    policy.recover(&attempt(Backend::Dx12, true));
    assert_eq!(
        policy.candidates(&Backend::ORDER, None),
        [Backend::Dx12, Backend::Gl]
    );
    policy.succeeded(attempt(Backend::Dx12, true));
    assert_eq!(
        policy.candidates(&Backend::ORDER, None),
        [Backend::Dx12, Backend::Vulkan, Backend::Gl]
    );
    assert_eq!(
        policy.candidates(&[Backend::Dx12], Some(&[Backend::Gl])),
        []
    );
}

#[test]
fn successful_fallback_clears_the_sentinel_and_survives_relaunch() {
    let dir = tempfile::tempdir().unwrap();
    let mut first = StartupGuard::open(dir.path()).unwrap();
    first.begin(attempt(Backend::Vulkan, false)).unwrap();
    let mut second = StartupGuard::open(dir.path()).unwrap();
    assert_eq!(second.interrupted, Some(attempt(Backend::Vulkan, false)));
    assert_eq!(
        second.recovery.candidates(&Backend::ORDER, None),
        [Backend::Dx12, Backend::Gl]
    );
    second.begin(attempt(Backend::Dx12, false)).unwrap();
    second.presented().unwrap();
    assert!(StartupGuard::pending(dir.path()).unwrap().is_none());
    let third = StartupGuard::open(dir.path()).unwrap();
    assert!(third.interrupted.is_none());
    assert_eq!(third.recovery.working.as_ref().unwrap().stage, Stage::Ready);
    assert_eq!(
        third.recovery.candidates(&Backend::ORDER, None),
        [Backend::Dx12, Backend::Vulkan, Backend::Gl]
    );
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
    });
    pending.stage = Stage::FirstPresentation;
    guard.begin(pending.clone()).unwrap();
    assert_eq!(StartupGuard::pending(dir.path()).unwrap(), Some(pending));
}
