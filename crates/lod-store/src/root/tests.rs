use super::*;
use crate::{
    identity::Dimension,
    test_support::{key, record},
};

const OVERWORLD: Dimension = Dimension(0);

/// Creates and closes the store of `destination`, opened at `now`, holding `records`
/// sections; returns its directory and size.
fn make_store(root: &StoreRoot, destination: &str, now: u32, records: i32) -> (PathBuf, u64) {
    let key = StoreKey::new(destination, "", 0);
    let mut store = root
        .open(&key, Minute::new(now), StoreOptions::default())
        .expect("open");
    store
        .begin_session(OVERWORLD, Minute::new(now))
        .expect("begin");
    for x in 0..records {
        store
            .put(
                OVERWORLD,
                &record(key_at(x), x as u32, 200),
                Minute::new(now),
            )
            .expect("put");
    }
    assert!(store.flush());
    store.close();
    let dir = root.store_dir(&key);
    let bytes = dir_bytes(&dir).expect("size");
    (dir, bytes)
}

/// Section keys spread over separate region files.
fn key_at(index: i32) -> lod::geometry::SectionKey {
    key(0, index * 32, 0, 0)
}

#[test]
fn the_root_is_versioned_and_lists_stores_by_last_open() {
    let dir = tempfile::tempdir().expect("temp dir");
    let root = StoreRoot::new(dir.path());
    assert_eq!(root.dir(), dir.path().join("v1"));
    assert!(root.stores().expect("list").is_empty());
    let (a, a_bytes) = make_store(&root, "remote:a", 100, 1);
    let (b, _) = make_store(&root, "remote:b", 200, 1);
    fs::create_dir_all(root.dir().join("not-a-store")).expect("stray dir");
    let stores = root.stores().expect("list");
    assert_eq!(stores.len(), 2);
    let a_summary = stores
        .iter()
        .find(|store| store.path == a)
        .expect("a listed");
    assert_eq!(a_summary.opened, Some(Minute::new(100)));
    assert_eq!(a_summary.bytes, a_bytes);
    assert!(
        stores
            .iter()
            .any(|store| store.path == b && store.opened == Some(Minute::new(200)))
    );
}

#[test]
fn quotas_remove_expired_then_least_recently_opened_stores() {
    let dir = tempfile::tempdir().expect("temp dir");
    let root = StoreRoot::new(dir.path());
    let (ancient, _) = make_store(&root, "remote:ancient", 0, 1);
    let (old, old_bytes) = make_store(&root, "remote:old", STORE_EXPIRY_MINUTES, 2);
    let (recent, recent_bytes) = make_store(&root, "remote:recent", STORE_EXPIRY_MINUTES + 10, 2);
    let now = Minute::new(STORE_EXPIRY_MINUTES + 20);
    let roomy = Quotas::default();
    let report = root.enforce(roomy, now, None, None).expect("enforce");
    assert_eq!(report.removed.len(), 1);
    assert_eq!(report.removed[0].0, ancient, "expired regardless of space");
    assert!(old.exists() && recent.exists());

    let tight = Quotas {
        global_bytes: recent_bytes + old_bytes - 1,
        ..roomy
    };
    let report = root.enforce(tight, now, None, None).expect("enforce");
    assert_eq!(report.removed, [(old.clone(), old_bytes)]);
    assert!(!old.exists() && recent.exists());
}

#[test]
fn stores_in_use_are_kept_and_the_open_store_gives_up_far_regions() {
    let dir = tempfile::tempdir().expect("temp dir");
    let root = StoreRoot::new(dir.path());
    let (busy, _) = make_store(&root, "remote:busy", 1, 2);
    let (_, idle_bytes) = make_store(&root, "remote:idle", 2, 2);
    let _holder = root
        .open(
            &StoreKey::new("remote:busy", "", 0),
            Minute::new(3),
            StoreOptions::default(),
        )
        .expect("open busy");
    let current_key = StoreKey::new("remote:current", "", 0);
    make_store(&root, "remote:current", 4, 4);
    let mut current = root
        .open(&current_key, Minute::new(5), StoreOptions::default())
        .expect("open current");
    let current_bytes = current.disk_bytes().expect("size");
    let busy_bytes = dir_bytes(&busy).expect("size");
    // Room for the busy store and half the current one only.
    let quotas = Quotas {
        global_bytes: busy_bytes + current_bytes / 2,
        ..Quotas::default()
    };
    let report = root
        .enforce(quotas, Minute::new(5), Some(&mut current), None)
        .expect("enforce");
    assert_eq!(report.removed.len(), 1, "the idle store goes");
    assert_eq!(report.removed[0].1, idle_bytes);
    assert!(matches!(report.kept.as_slice(), [(path, RemoveError::InUse)] if *path == busy));
    assert!(report.evicted_region_bytes > 0);
    assert!(current.disk_bytes().expect("size") <= current_bytes / 2);
    assert!(current.dir().exists(), "the open store is never removed");
}

#[test]
fn clearing_removes_every_store_not_in_use() {
    let dir = tempfile::tempdir().expect("temp dir");
    let root = StoreRoot::new(dir.path());
    let (idle, _) = make_store(&root, "remote:idle", 1, 1);
    let (busy, _) = make_store(&root, "remote:busy", 1, 1);
    let holder = root
        .open(
            &StoreKey::new("remote:busy", "", 0),
            Minute::new(2),
            StoreOptions::default(),
        )
        .expect("open busy");
    // A removal interrupted after its rename.
    let leftover = root
        .dir()
        .join(format!("{}{REMOVING_SUFFIX}", "f".repeat(64)));
    fs::create_dir_all(leftover.join("dim0")).expect("leftover");
    let report = root.clear().expect("clear");
    assert_eq!(report.removed, std::slice::from_ref(&idle));
    assert!(matches!(report.kept.as_slice(), [(path, RemoveError::InUse)] if *path == busy));
    assert!(!idle.exists() && busy.exists() && !leftover.exists());
    drop(holder);
    assert_eq!(root.clear().expect("clear").removed, [busy]);
    assert!(root.stores().expect("list").is_empty());
}

#[test]
fn a_store_whose_last_open_is_unreadable_counts_as_opened_now() {
    let dir = tempfile::tempdir().expect("temp dir");
    let root = StoreRoot::new(dir.path());
    let (torn, _) = make_store(&root, "remote:torn", 1, 1);
    fs::write(torn.join(crate::access::OPENED_FILE), b"torn").expect("damage");
    let now = Minute::new(STORE_EXPIRY_MINUTES + 10);
    let report = root
        .enforce(Quotas::default(), now, None, None)
        .expect("enforce");
    assert!(report.removed.is_empty(), "never treated as expired");
    assert!(torn.exists());
    let summary = root.stores().expect("list");
    assert_eq!(
        summary[0].opened,
        Some(now),
        "stamped so it can expire later"
    );
}

#[test]
fn stores_open_read_only_are_never_removed() {
    let dir = tempfile::tempdir().expect("temp dir");
    let root = StoreRoot::new(dir.path());
    let (path, _) = make_store(&root, "remote:shared", 1, 1);
    let key = StoreKey::new("remote:shared", "", 0);
    let writer = root
        .open(&key, Minute::new(2), StoreOptions::default())
        .expect("writer");
    let reader = root
        .open(&key, Minute::new(2), StoreOptions::default())
        .expect("reader");
    assert_eq!(reader.access(), crate::region_file::Access::ReadOnly);
    // The writer leaves while the reader stays.
    writer.close();
    assert!(matches!(remove_store(&path), Err(RemoveError::InUse)));
    assert!(path.exists());
    drop(reader);
    remove_store(&path).expect("removed once nobody holds it");
    assert!(!path.exists());
}

/// Builds a store of `destination` in the layout directory of `version` beside `root`,
/// as a client of that layout would, and returns its directory.
fn make_layout_store(root: &StoreRoot, version: u32, destination: &str) -> PathBuf {
    let cache_dir = root.dir().parent().expect("versioned root");
    let other = StoreRoot {
        cache_dir: cache_dir.to_owned(),
        dir: cache_dir.join(version_dir_name(version)),
    };
    make_store(&other, destination, 1, 1).0
}

#[test]
fn older_layouts_are_removed_by_quotas_and_every_layout_by_clearing() {
    let dir = tempfile::tempdir().expect("temp dir");
    let root = StoreRoot::new(dir.path());
    let older = dir.path().join(version_dir_name(0));
    let old_store = make_layout_store(&root, 0, "remote:old");
    let newer_store = make_layout_store(&root, STORE_FORMAT_VERSION + 1, "remote:new");
    let newer = newer_store.parent().expect("layout").to_owned();
    fs::create_dir_all(dir.path().join("v01")).expect("not a layout name");
    let (current, current_bytes) = make_store(&root, "remote:current", 5, 1);
    let newer_bytes = dir_bytes(&newer).expect("size");

    let roomy = Quotas::default();
    let report = root
        .enforce(roomy, Minute::new(6), None, None)
        .expect("enforce");
    assert!(
        !old_store.exists() && !older.exists(),
        "older layouts go first"
    );
    assert!(report.removed.iter().any(|(path, _)| *path == old_store));
    assert!(
        newer_store.exists(),
        "newer layouts belong to newer clients"
    );

    let tight = Quotas {
        global_bytes: newer_bytes + current_bytes - 1,
        ..roomy
    };
    let report = root
        .enforce(tight, Minute::new(6), None, None)
        .expect("enforce");
    assert_eq!(
        report.removed,
        [(current.clone(), current_bytes)],
        "newer layouts count toward the global cap"
    );

    make_layout_store(&root, 0, "remote:old");
    let report = root.clear().expect("clear");
    assert!(report.kept.is_empty());
    assert!(!older.exists() && !newer.exists());
    assert!(
        dir.path().join("v01").exists(),
        "unknown names are left alone"
    );
}

#[test]
fn clearing_keeps_an_older_layout_while_a_client_holds_one_of_its_stores() {
    let dir = tempfile::tempdir().expect("temp dir");
    let root = StoreRoot::new(dir.path());
    let held = make_layout_store(&root, 0, "remote:held");
    let holder = StoreLock::try_acquire(&held).expect("lock").expect("free");
    let report = root.clear().expect("clear");
    assert!(matches!(report.kept.as_slice(), [(path, RemoveError::InUse)] if *path == held));
    assert!(held.exists());
    drop(holder);
    let report = root.clear().expect("clear");
    assert_eq!(
        report.removed.len(),
        2,
        "the store, then its layout directory"
    );
    assert!(!held.parent().expect("layout").exists());
}
