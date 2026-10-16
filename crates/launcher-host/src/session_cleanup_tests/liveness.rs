use super::*;
use std::cell::Cell;

/// Seeds one stale account endpoint owned by the supplied process identity.
fn stale_account(root: &TempRoot, pid: u32, token: &str) -> PathBuf {
    let directory = root.join(&format!("account-{pid}-1"));
    fs::create_dir(&directory).unwrap();
    write_marker(
        &directory,
        token,
        pid,
        "account",
        1,
        FAKE_NOW - 2 * DAY_SECS,
    );
    fs::write(directory.join("bridge.endpoint"), b"original endpoint").unwrap();
    directory
}

#[test]
fn old_live_foreign_account_endpoint_survives_the_production_probe() {
    let root = TempRoot::new("live-foreign-account");
    let directory = stale_account(&root, process::id(), "live-token");
    let mut policy = policy_at(FAKE_NOW, 8);
    policy.current_pid = process::id().wrapping_add(1);
    let report = super::super::reclaim_stale_entries(root.path(), &policy);
    assert_eq!(report.reclaimed_directories, 0);
    assert_eq!(report.skipped_protected_identity, 1);
    assert_eq!(
        fs::read(directory.join("bridge.endpoint")).unwrap(),
        b"original endpoint"
    );
}

#[test]
fn indeterminate_foreign_owner_is_preserved_even_when_old() {
    let root = TempRoot::new("unknown-owner");
    let directory = stale_account(&root, 777, "unknown-token");
    let report =
        super::super::reclaim_stale_entries_with(root.path(), &policy_at(FAKE_NOW, 8), &|_| {
            OwnerState::Unknown
        });
    assert_eq!(report.reclaimed_directories, 0);
    assert_eq!(report.skipped_protected_identity, 1);
    assert!(directory.join("bridge.endpoint").exists());
}

#[test]
fn revived_or_indeterminate_owner_is_rechecked_before_removal() {
    for state in [OwnerState::Alive, OwnerState::Unknown] {
        let root = TempRoot::new("changed-owner-state");
        let directory = stale_account(&root, 777, "original-token");
        let probes = Cell::new(0);
        let report =
            super::super::reclaim_stale_entries_with(root.path(), &policy_at(FAKE_NOW, 8), &|_| {
                probes.set(probes.get() + 1);
                if probes.get() == 1 {
                    OwnerState::Dead
                } else {
                    state
                }
            });
        assert_eq!(probes.get(), 2);
        assert_eq!(report.reclaimed_directories, 0);
        assert_eq!(report.skipped_protected_identity, 1);
        assert!(directory.join("bridge.endpoint").exists());
    }
}

#[test]
fn changed_marker_is_preserved_after_a_dead_owner_was_classified() {
    let root = TempRoot::new("changed-marker");
    let directory = stale_account(&root, 777, "original-token");
    let probes = Cell::new(0);
    let report =
        super::super::reclaim_stale_entries_with(root.path(), &policy_at(FAKE_NOW, 8), &|_| {
            probes.set(probes.get() + 1);
            if probes.get() == 2 {
                write_marker(
                    &directory,
                    "replacement-token",
                    777,
                    "account",
                    1,
                    FAKE_NOW - 2 * DAY_SECS,
                );
            }
            OwnerState::Dead
        });
    assert_eq!(probes.get(), 2);
    assert_eq!(report.reclaimed_directories, 0);
    assert_eq!(report.skipped_protected_identity, 1);
    assert_eq!(
        read_marker_token(&directory).as_deref(),
        Some("replacement-token")
    );
    assert!(directory.join("bridge.endpoint").exists());
}

#[test]
fn unavailable_root_lock_preserves_entries_and_retries_later() {
    let root = TempRoot::new("root-lock");
    let directory = stale_account(&root, 777, "dead-token");
    let held = super::super::lock_root(root.path(), false).unwrap();
    let report = reclaim_stale_entries(root.path(), &policy_at(FAKE_NOW, 8));
    assert_eq!(report.reclaimed_directories, 0);
    assert!(directory.exists());
    drop(held);
    let report = reclaim_stale_entries(root.path(), &policy_at(FAKE_NOW, 8));
    assert_eq!(report.reclaimed_directories, 1);
    assert!(!directory.exists());
}

#[test]
fn root_lock_errors_preserve_entries_and_refuse_uncoordinated_binding() {
    let root = TempRoot::new("root-lock-error");
    let directory = stale_account(&root, 777, "dead-token");
    fs::create_dir(root.join(super::super::ROOT_LOCK_FILE_NAME)).unwrap();
    let report = reclaim_stale_entries(root.path(), &policy_at(FAKE_NOW, 8));
    assert_eq!(report.reclaimed_directories, 0);
    assert!(directory.join("bridge.endpoint").exists());
    let new_directory = root.join(&format!("direct-{}", process::id()));
    assert!(SessionDirectoryGuard::bind(new_directory.clone()).is_err());
    assert!(
        !new_directory.exists(),
        "an uncoordinated bind must not start a session"
    );
}

#[test]
fn classification_and_removal_hold_the_same_os_lock_as_binding() {
    let root = TempRoot::new("held-through-probe");
    stale_account(&root, 777, "dead-token");
    let probes = Cell::new(0);
    let report =
        super::super::reclaim_stale_entries_with(root.path(), &policy_at(FAKE_NOW, 8), &|_| {
            probes.set(probes.get() + 1);
            assert!(
                super::super::lock_root(root.path(), true).is_err(),
                "another owner must not bind through reclamation's lock"
            );
            OwnerState::Dead
        });
    assert_eq!(probes.get(), 2);
    assert_eq!(report.reclaimed_directories, 1);
    assert!(
        super::super::lock_root(root.path(), true).is_ok(),
        "the pass releases its lock"
    );
}

#[test]
fn old_live_foreign_catalog_is_preserved() {
    let root = TempRoot::new("live-catalog");
    let catalog = root.join(&format!("catalog-{}.json", process::id()));
    fs::write(&catalog, b"original catalog").unwrap();
    set_file_mtime(&catalog, FAKE_NOW - 2 * DAY_SECS);
    let mut policy = policy_at(FAKE_NOW, 8);
    policy.current_pid = process::id().wrapping_add(1);
    let report = super::super::reclaim_stale_entries(root.path(), &policy);
    assert_eq!(report.reclaimed_catalog_files, 0);
    assert_eq!(report.skipped_protected_identity, 1);
    assert_eq!(fs::read(&catalog).unwrap(), b"original catalog");
}
