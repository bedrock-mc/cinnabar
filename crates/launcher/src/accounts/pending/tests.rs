use super::*;

struct Fixture {
    root: PathBuf,
    store: AccountStore,
}
impl Fixture {
    /// Isolates synthetic credentials from every user account namespace.
    fn new() -> Self {
        let root = std::env::temp_dir().join(format!(
            "cinnabar-native-signin-{}-{}",
            std::process::id(),
            TEMP_COUNTER.fetch_add(1, Ordering::Relaxed)
        ));
        private_directory(&root).unwrap();
        let store = AccountStore::new(root.join("synthetic-token.json"));
        Self { root, store }
    }
    /// Creates a selected synthetic account using the existing store contract.
    fn selected(&self) {
        write_private(&self.store.active_cache,
            br#"{"access_token":"A-access","refresh_token":"A-refresh","cinnabar_sign_in_generation":"A"}"#).unwrap();
        self.store
            .remember_current("123", "Account A", None)
            .unwrap();
    }
}
impl Drop for Fixture {
    /// Removes only this fixture's synthetic namespace.
    fn drop(&mut self) {
        let _ = fs::remove_dir_all(&self.root);
    }
}

/// An old worker cannot publish, commit or cancel a newer pending sign-in.
#[test]
fn superseded_attempt_cannot_modify_selected_or_newer_pending_account() {
    let fixture = Fixture::new();
    fixture.selected();
    let active = read_private(&fixture.store.active_cache).unwrap();
    let saved = read_private(&fixture.store.directory.join("123.json")).unwrap();
    let old = fixture
        .store
        .try_begin_native_sign_in("old-attempt", None)
        .unwrap();
    old.try_publish("old-access", "old-refresh", "2099-01-01T00:00:00Z")
        .unwrap();
    let new = fixture
        .store
        .try_begin_native_sign_in("new-attempt", None)
        .unwrap();
    new.try_publish("B-access", "B-refresh", "2099-01-01T00:00:00Z")
        .unwrap();
    new.try_publish_checkpoint(b"B-proof").unwrap();
    let pending = read_private(&fixture.store.pending_cache()).unwrap();
    assert_eq!(
        old.try_publish("late", "late-refresh", "2099-01-01T00:00:00Z")
            .unwrap_err()
            .kind(),
        io::ErrorKind::PermissionDenied
    );
    fixture
        .store
        .try_discard_native_pending("old-attempt")
        .unwrap();
    assert_eq!(
        read_private(&fixture.store.pending_cache()).unwrap(),
        pending
    );
    let profile = AccountProfile {
        id: "456".into(),
        gamertag: "Account B".into(),
        picture_path: None,
    };
    assert!(old.try_commit(&profile).is_err());
    assert_eq!(read_private(&fixture.store.active_cache).unwrap(), active);
    assert_eq!(
        read_private(&fixture.store.directory.join("123.json")).unwrap(),
        saved
    );
    new.try_commit(&profile).unwrap();
    assert_eq!(fixture.store.active_id().unwrap().as_deref(), Some("456"));
    assert_eq!(
        read_private(&fixture.store.directory.join("123.json")).unwrap(),
        saved
    );
    let lease = fixture.store.try_native_lease().unwrap();
    assert_eq!(lease.generation(), "new-attempt");
    assert_eq!(lease.refresh_token(), "B-refresh");
    assert_eq!(lease.checkpoint().unwrap().unwrap().as_slice(), b"B-proof");
    drop(lease);
    assert!(!fixture.store.pending_cache().exists());
    assert!(!fixture.store.directory.join(CHECKPOINT_FILE).exists());
    assert!(!fixture.store.directory.join(ATTEMPT_FILE).exists());
}

/// Browser interaction retains no file leases, and a later active revision retires the attempt.
#[test]
fn interactive_wait_releases_leases_and_cannot_overwrite_a_rotated_active_account() {
    let fixture = Fixture::new();
    fixture.selected();
    let pending = fixture
        .store
        .try_begin_native_sign_in("device-attempt", Some("123"))
        .unwrap();
    assert!(fixture.store.try_snapshot().is_ok());
    let mut active = fixture.store.try_native_lease().unwrap();
    active
        .publish_oauth("rotated-access", "rotated-refresh", "2099-01-01T00:00:00Z")
        .unwrap();
    drop(active);
    assert_eq!(
        pending
            .try_publish("device-access", "device-refresh", "2099-01-01T00:00:00Z")
            .unwrap_err()
            .kind(),
        io::ErrorKind::PermissionDenied
    );
    assert_eq!(
        fixture.store.try_native_lease().unwrap().refresh_token(),
        "rotated-refresh"
    );
    fixture
        .store
        .try_discard_native_pending("device-attempt")
        .unwrap();
    assert!(!fixture.store.pending_cache().exists());
}

/// A failed identity or proof write cannot replace a selected saved account's credentials.
#[test]
fn wrong_reauthentication_identity_and_failed_proof_keep_active_and_saved_account() {
    let fixture = Fixture::new();
    fixture.selected();
    let original = read_private(&fixture.store.active_cache).unwrap();
    let saved = read_private(&fixture.store.directory.join("123.json")).unwrap();
    let pending = fixture
        .store
        .try_begin_native_sign_in("reauthentication", Some("123"))
        .unwrap();
    pending
        .try_publish("acquired-access", "latest-refresh", "2099-01-01T00:00:00Z")
        .unwrap();
    fs::create_dir(fixture.store.directory.join(CHECKPOINT_FILE)).unwrap();
    assert!(pending.try_publish_checkpoint(b"proof").is_err());
    assert!(
        String::from_utf8(read_private(&fixture.store.pending_cache()).unwrap())
            .unwrap()
            .contains("latest-refresh")
    );
    fs::remove_dir(fixture.store.directory.join(CHECKPOINT_FILE)).unwrap();
    pending.try_publish_checkpoint(b"proof").unwrap();
    assert_eq!(
        pending
            .try_commit(&AccountProfile {
                id: "456".into(),
                gamertag: "Wrong account".into(),
                picture_path: None
            })
            .unwrap_err()
            .kind(),
        io::ErrorKind::PermissionDenied
    );
    assert_eq!(read_private(&fixture.store.active_cache).unwrap(), original);
    assert_eq!(
        read_private(&fixture.store.directory.join("123.json")).unwrap(),
        saved
    );
    assert!(!fixture.store.directory.join("456.json").exists());
    assert!(!super::super::native::checkpoint_path(&fixture.store, Some("456")).exists());
    fixture
        .store
        .try_discard_native_pending("reauthentication")
        .unwrap();
    assert!(!fixture.store.pending_cache().exists());
}
