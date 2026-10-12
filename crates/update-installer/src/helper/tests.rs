use super::*;

fn helper_copies(update_dir: &Path) -> usize {
    fs::read_dir(update_dir)
        .unwrap()
        .flatten()
        .filter(|entry| entry.file_name().to_string_lossy().starts_with(PREFIX))
        .count()
}

#[test]
fn a_copy_survives_until_its_helper_takes_the_lock() {
    let update_dir = tempfile::tempdir().unwrap();
    let source = update_dir.path().join("packaged-helper");
    fs::write(&source, "helper").unwrap();
    let (helper, parent_reader) = copy(update_dir.path(), &source).unwrap();
    // A concurrent prune between spawning and the helper's own lock keeps the copy.
    prune(update_dir.path(), SystemTime::now());
    assert!(helper.exists());
    let running = hold(&helper).unwrap();
    drop(parent_reader);
    let later = SystemTime::now() + HANDOFF_WINDOW * 2;
    prune(update_dir.path(), later);
    assert!(helper.exists());
    drop(running);
    prune(update_dir.path(), later);
    assert!(!helper.exists());
}

#[test]
fn finished_attempts_are_removed_after_the_handoff_window() {
    let update_dir = tempfile::tempdir().unwrap();
    let source = update_dir.path().join("packaged-helper");
    fs::write(&source, "helper").unwrap();
    fs::create_dir(update_dir.path().join("ready")).unwrap();
    let (finished, parent_reader) = copy(update_dir.path(), &source).unwrap();
    drop(hold(&finished).unwrap());
    drop(parent_reader);
    fs::create_dir(update_dir.path().join("helper-crashed")).unwrap();
    assert_eq!(helper_copies(update_dir.path()), 2);
    prune(update_dir.path(), SystemTime::now() + HANDOFF_WINDOW * 2);
    assert_eq!(helper_copies(update_dir.path()), 0);
    assert!(update_dir.path().join("ready").is_dir());
}

#[test]
fn the_install_itself_is_never_held() {
    let update_dir = tempfile::tempdir().unwrap();
    let install = update_dir.path().join("Cinnabar").join("cinnabar-updater");
    assert!(hold(&install).is_err());
}

#[test]
fn client_and_helper_readers_overlap_without_exposing_the_copy_to_pruning() {
    let root = tempfile::tempdir().unwrap();
    let source = root.path().join("source");
    fs::write(&source, "helper").unwrap();
    let (helper, parent_reader) = copy(root.path(), &source).unwrap();
    let helper_reader = hold(&helper).unwrap();
    let later = SystemTime::now() + HANDOFF_WINDOW * 2;
    prune(root.path(), later);
    assert!(helper.exists());
    drop(parent_reader);
    prune(root.path(), later);
    assert!(helper.exists());
    drop(helper_reader);
    prune(root.path(), later);
    assert!(!helper.exists());
}

#[test]
fn manual_packages_share_the_held_lease_and_recent_handoff_retention_rule() {
    let root = tempfile::tempdir().unwrap();
    let (directory, lease) = snapshot_directory(root.path()).unwrap();
    let directory = directory.keep();
    let package = directory.join("installer");
    fs::write(&package, "manual package").unwrap();
    let handed_off = SystemTime::now() + HANDOFF_WINDOW * 2;
    prune(root.path(), handed_off);
    assert!(package.exists());
    // Even a slow snapshot gets a full window after the desktop handoff finishes.
    lease.set_modified(handed_off).unwrap();
    drop(lease);
    prune(root.path(), handed_off + HANDOFF_WINDOW / 2);
    assert!(package.exists());
    prune(root.path(), handed_off + HANDOFF_WINDOW * 2);
    assert!(!directory.exists());
}

#[test]
fn an_abandoned_manual_copy_is_pruned_without_touching_ready() {
    let root = tempfile::tempdir().unwrap();
    fs::create_dir(root.path().join("ready")).unwrap();
    let (directory, lease) = snapshot_directory(root.path()).unwrap();
    let directory = directory.keep();
    drop(lease);
    prune(root.path(), SystemTime::now() + HANDOFF_WINDOW * 2);
    assert!(!directory.exists());
    assert!(root.path().join("ready").is_dir());
}
