use super::*;

#[cfg(unix)]
#[test]
fn the_next_good_start_confirms_an_unconfirmed_bundle_or_appimage() {
    for bundle in [false, true] {
        let root = tempfile::tempdir().unwrap();
        let target = root.path().join("target");
        let candidate = root.path().join("candidate");
        let backup = crate::target::backup(&target).unwrap();
        if bundle {
            fs::create_dir(&target).unwrap();
            fs::create_dir(&candidate).unwrap();
            fs::write(target.join("client"), "old").unwrap();
            fs::write(candidate.join("client"), "new").unwrap();
        } else {
            fs::write(&target, "old").unwrap();
            fs::write(&candidate, "new").unwrap();
        }
        crate::apply::swap(&target, &candidate, &backup).unwrap();
        assert!(!confirmation(&backup).exists());
        let next_start = Startup::observe(target.clone()).unwrap();
        next_start.confirm(&root.path().join("updates")).unwrap();
        assert!(!backup.exists());
        let payload = if bundle {
            target.join("client")
        } else {
            target
        };
        assert_eq!(fs::read(payload).unwrap(), b"new");
    }
}

#[cfg(unix)]
#[test]
fn an_older_running_client_cannot_confirm_a_replacement_it_did_not_start() {
    let root = tempfile::tempdir().unwrap();
    let target = root.path().join("target");
    let candidate = root.path().join("candidate");
    let backup = crate::target::backup(&target).unwrap();
    fs::write(&target, "old").unwrap();
    let old_start = Startup::observe(target.clone()).unwrap();
    fs::write(&candidate, "new").unwrap();
    crate::apply::swap(&target, &candidate, &backup).unwrap();
    old_start.confirm(&root.path().join("updates")).unwrap();
    assert_eq!(fs::read(&backup).unwrap(), b"old");
    assert!(!confirmation(&backup).exists());
}

#[cfg(any(target_os = "macos", target_os = "linux"))]
#[test]
fn interrupted_preparation_keeps_the_old_install_and_recovers_on_good_start() {
    let root = tempfile::tempdir().unwrap();
    let target = root.path().join("target");
    let backup = crate::target::backup(&target).unwrap();
    fs::write(&target, "old").unwrap();
    // The helper prepared the candidate beside the install, but did not exchange them yet.
    prepare_exchange(&target, &backup).unwrap();
    fs::write(&backup, "candidate").unwrap();
    Startup::observe(target.clone())
        .unwrap()
        .confirm(&root.path().join("updates"))
        .unwrap();
    assert_eq!(fs::read(&target).unwrap(), b"old");
    assert!(!backup.exists());
}

#[cfg(any(target_os = "macos", target_os = "linux"))]
#[test]
fn interrupted_rollback_is_not_reversed_by_failed_start_recovery() {
    let root = tempfile::tempdir().unwrap();
    let target = root.path().join("target");
    let backup = crate::target::backup(&target).unwrap();
    fs::write(&target, "old").unwrap();
    prepare_exchange(&target, &backup).unwrap();
    fs::write(&backup, "failed new").unwrap();
    exchange(&target, &backup).unwrap();
    exchange(&target, &backup).unwrap();
    assert!(!needs_rollback(&target, root.path()));
    assert!(!restore(&target, root.path()).unwrap());
    assert_eq!(fs::read(&target).unwrap(), b"old");
    assert!(!backup.exists());
}

#[cfg(any(target_os = "macos", target_os = "linux"))]
#[test]
fn failed_start_recovery_does_not_mistake_a_prepared_candidate_for_the_predecessor() {
    let root = tempfile::tempdir().unwrap();
    let target = root.path().join("target");
    let backup = crate::target::backup(&target).unwrap();
    fs::write(&target, "old").unwrap();
    prepare_exchange(&target, &backup).unwrap();
    fs::write(&backup, "not yet installed").unwrap();
    assert!(!needs_rollback(&target, root.path()));
    assert!(!restore(&target, root.path()).unwrap());
    assert_eq!(fs::read(&target).unwrap(), b"old");
    assert!(!backup.exists());
    assert!(!preparation(&backup).exists());
}

#[test]
fn a_failed_unconfirmed_restart_restores_the_predecessor_without_manual_cleanup() {
    let root = tempfile::tempdir().unwrap();
    let target = root.path().join("target");
    let backup = crate::target::backup(&target).unwrap();
    fs::write(&target, "failed new").unwrap();
    fs::write(&backup, "old").unwrap();
    assert!(needs_rollback(&target, root.path()));
    assert!(restore(&target, root.path()).unwrap());
    assert_eq!(fs::read(&target).unwrap(), b"old");
    assert!(!backup.exists());
    assert!(!needs_rollback(&target, root.path()));
    assert!(!restore(&target, root.path()).unwrap());
}

#[test]
fn recovery_never_rolls_back_a_confirmed_start_or_a_live_helpers_swap() {
    let root = tempfile::tempdir().unwrap();
    let target = root.path().join("target");
    let backup = crate::target::backup(&target).unwrap();
    fs::write(&target, "new").unwrap();
    fs::write(&backup, "old").unwrap();
    let lease = File::options()
        .create(true)
        .truncate(false)
        .write(true)
        .open(root.path().join(crate::LOCK_FILE))
        .unwrap();
    lease.lock().unwrap();
    assert!(!needs_rollback(&target, root.path()));
    assert!(restore(&target, root.path()).is_err());
    drop(lease);
    confirm_start(&backup).unwrap();
    assert!(!needs_rollback(&target, root.path()));
    assert!(!restore(&target, root.path()).unwrap());
    assert_eq!(fs::read(&target).unwrap(), b"new");
}
