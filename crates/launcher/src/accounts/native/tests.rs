use super::*;

struct Fixture {
    root: PathBuf,
    store: AccountStore,
}

impl Fixture {
    /// Creates private synthetic account storage without reading the user's namespace.
    fn new() -> Self {
        let root = std::env::temp_dir().join(format!(
            "cinnabar-native-account-{}-{}",
            std::process::id(),
            TEMP_COUNTER.fetch_add(1, Ordering::Relaxed)
        ));
        private_directory(&root).unwrap();
        let store = AccountStore::new(root.join("microsoft-token.json"));
        Self { root, store }
    }

    /// Publishes a synthetic generation using the existing private atomic writer.
    fn sign_in(&self, generation: &str) {
        let bytes = serde_json::to_vec(
            &serde_json::json!({"refresh_token": "old-refresh", "access_token": "old-access",
            "cinnabar_sign_in_generation": generation, "future_metadata": {"kept": true}}),
        )
        .unwrap();
        write_private(&self.store.active_cache, &bytes).unwrap();
    }
}

impl Drop for Fixture {
    fn drop(&mut self) {
        let _ = fs::remove_dir_all(&self.root);
    }
}

/// Native and Go refresh share one lease; publication cannot resurrect retired sign-ins.
#[test]
fn rotation_is_persisted_and_generation_replacement_or_removal_cannot_be_undone() {
    let fixture = Fixture::new();
    fixture.sign_in("first");
    fixture
        .store
        .try_native_lease()
        .unwrap()
        .publish_active_checkpoint(b"temporary-proof")
        .unwrap();
    write_private(
        &derived_path(&fixture.store.active_cache),
        br#"{"opaque_go_proof":"unchanged"}"#,
    )
    .unwrap();
    fixture
        .store
        .remember_current("123", "Synthetic", None)
        .unwrap();
    let mut native = fixture.store.try_native_lease().unwrap();
    assert_eq!(
        fixture.store.try_snapshot().unwrap_err().kind(),
        io::ErrorKind::WouldBlock
    );
    assert_eq!(
        fixture.store.try_discard_pending().unwrap_err().kind(),
        io::ErrorKind::WouldBlock
    );
    assert_eq!(
        fixture.store.try_native_lease().err().unwrap().kind(),
        io::ErrorKind::WouldBlock
    );
    assert!(try_lease(&cache_lock_path(&fixture.store.active_cache)).is_err());
    native
        .publish_oauth("new-access", "rotated-refresh", "2099-01-01T00:00:00Z")
        .unwrap();
    native
        .publish_verified_identity("123", b"synthetic-proof-owner")
        .unwrap();
    let oauth: serde_json::Value =
        serde_json::from_slice(&read_private(&fixture.store.active_cache).unwrap()).unwrap();
    assert_eq!(oauth["refresh_token"], "rotated-refresh");
    assert_eq!(oauth["cinnabar_sign_in_generation"], "first");
    assert_eq!(oauth["future_metadata"]["kept"], true);
    assert_eq!(
        read_private(&derived_path(&fixture.store.active_cache)).unwrap(),
        br#"{"opaque_go_proof":"unchanged"}"#
    );
    drop(native);
    let mut native = fixture.store.try_native_lease().unwrap();
    assert_eq!(native.refresh_token(), "rotated-refresh");
    assert_eq!(
        native.checkpoint().unwrap().unwrap().as_slice(),
        b"synthetic-proof-owner"
    );

    fixture.sign_in("replacement");
    assert_eq!(
        native
            .publish_oauth("stale", "stale-refresh", "2099-01-01T00:00:00Z")
            .unwrap_err()
            .kind(),
        io::ErrorKind::PermissionDenied
    );
    assert_eq!(
        parse_oauth(&read_private(&fixture.store.active_cache).unwrap())
            .unwrap()
            .cinnabar_sign_in_generation,
        "replacement"
    );
    drop(native);
    let mut native = fixture.store.try_native_lease().unwrap();
    assert!(native.checkpoint().unwrap().is_none());
    remove_optional(&fixture.store.active_cache).unwrap();
    assert_eq!(
        native
            .publish_oauth("stale", "stale-refresh", "2099-01-01T00:00:00Z")
            .unwrap_err()
            .kind(),
        io::ErrorKind::NotFound
    );
    assert!(!fixture.store.active_cache.exists());
    drop(native);

    fixture.sign_in("first");
    fixture.store.sign_out().unwrap();
    assert!(!checkpoint_path(&fixture.store, Some("123")).exists());
    assert!(fixture.store.try_native_lease().is_err());
}

/// The native writer accepts exactly the Go reader's serialized bound and rejects escaped overflow.
#[test]
fn active_oauth_publication_preserves_go_reader_compatibility() {
    let fixture = Fixture::new();
    fixture.sign_in("boundary");
    let mut native = fixture.store.try_native_lease().unwrap();
    let expiry = "2099-01-01T00:00:00Z";
    native.publish_oauth("", "rotated-refresh", expiry).unwrap();
    let overhead = read_private(&fixture.store.active_cache).unwrap().len();
    let boundary = "a".repeat(MAX_OAUTH_CACHE_BYTES - overhead);
    native
        .publish_oauth(&boundary, "rotated-refresh", expiry)
        .unwrap();
    let previous =
        read_private_bounded(&fixture.store.active_cache, MAX_OAUTH_CACHE_BYTES as u64).unwrap();
    assert_eq!(previous.len(), MAX_OAUTH_CACHE_BYTES);
    assert_eq!(
        parse_oauth(&previous).unwrap().refresh_token.as_str(),
        "rotated-refresh"
    );
    for oversized in [
        format!("{boundary}a"),
        "\0".repeat(MAX_OAUTH_CACHE_BYTES / 6),
    ] {
        assert_eq!(
            native
                .publish_oauth(&oversized, "unpublished-refresh", expiry)
                .unwrap_err()
                .kind(),
            io::ErrorKind::FileTooLarge
        );
        assert_eq!(read_private(&fixture.store.active_cache).unwrap(), previous);
        assert_eq!(native.refresh_token(), "rotated-refresh");
    }
    native
        .publish_oauth("small", "next-refresh", expiry)
        .unwrap();
    assert_eq!(native.refresh_token(), "next-refresh");
}

/// A failed proof promotion cannot leave the saved account behind the active refresh rotation.
#[test]
fn failed_verified_proof_promotion_keeps_the_saved_refresh_current() {
    let fixture = Fixture::new();
    fixture.sign_in("current");
    fixture
        .store
        .remember_current("123", "Saved", None)
        .unwrap();
    let mut native = fixture.store.try_native_lease().unwrap();
    native
        .publish_oauth("new-access", "rotated-refresh", "2099-01-01T00:00:00Z")
        .unwrap();
    let blocked_proof = checkpoint_path(&fixture.store, Some("123"));
    private_directory(&blocked_proof).unwrap();
    assert!(
        native
            .publish_verified_identity("123", b"current-proof")
            .is_err()
    );
    native.publish_active_checkpoint(b"current-proof").unwrap();
    drop(native);
    fs::remove_dir(&blocked_proof).unwrap();
    remove_optional(&fixture.store.active_cache).unwrap();
    fixture.store.activate("123").unwrap();
    let reopened = fixture.store.try_native_lease().unwrap();
    assert_eq!(reopened.refresh_token(), "rotated-refresh");
    assert_eq!(
        reopened.checkpoint().unwrap().unwrap().as_slice(),
        b"current-proof"
    );
}

/// The newest active checkpoint suppresses a saved backup that still contains a refused issuance.
#[test]
fn active_checkpoint_replacement_survives_reopening_a_selected_account() {
    let fixture = Fixture::new();
    fixture.sign_in("selected");
    fixture
        .store
        .remember_current("123", "Saved", None)
        .unwrap();
    let mut lease = fixture.store.try_native_lease().unwrap();
    lease
        .publish_verified_identity("123", b"includes-refused-issuance")
        .unwrap();
    let previous = lease.binding();
    lease
        .publish_active_checkpoint(b"refused-issuance-removed")
        .unwrap();
    assert_ne!(lease.binding(), previous);
    drop(lease);

    let reopened = AccountStore::new(fixture.store.active_cache.clone());
    let lease = reopened.try_native_lease().unwrap();
    assert_eq!(lease.selected_id(), Some("123"));
    assert_eq!(lease.refresh_token(), "old-refresh");
    assert_eq!(
        lease.checkpoint().unwrap().unwrap().as_slice(),
        b"refused-issuance-removed"
    );
}

/// A crash before final publication must leave both account slots unable to reuse refused entries.
#[test]
fn suppressed_checkpoint_preserves_proof_after_account_switch_and_oauth_change() {
    let fixture = Fixture::new();
    fixture.sign_in("selected");
    fixture
        .store
        .remember_current("123", "Saved", None)
        .unwrap();
    let mut lease = fixture.store.try_native_lease().unwrap();
    lease
        .publish_verified_identity("123", b"opaque-proof-and-cache")
        .unwrap();
    lease.suppress_checkpoint().unwrap();
    let suppressed = lease.binding();
    drop(lease);

    // Switching away replaces the active slot's account; only the saved backup can restore A.
    fixture.sign_in("other");
    fixture
        .store
        .remember_current("456", "Other", None)
        .unwrap();
    fixture
        .store
        .try_native_lease()
        .unwrap()
        .publish_verified_identity("456", b"other-account-proof")
        .unwrap();
    fixture.store.activate("123").unwrap();
    let mut lease = fixture.store.try_native_lease().unwrap();
    assert_eq!(lease.binding(), suppressed);
    assert!(!lease.checkpoint_cache_allowed());
    assert_eq!(
        lease.checkpoint().unwrap().unwrap().as_slice(),
        b"opaque-proof-and-cache"
    );
    lease
        .publish_oauth("new-access", "current-refresh", "2099-01-01T00:00:00Z")
        .unwrap();
    assert!(!lease.checkpoint_cache_allowed());
    drop(lease);
    let lease = fixture.store.try_native_lease().unwrap();
    assert_eq!(lease.refresh_token(), "current-refresh");
    assert!(!lease.checkpoint_cache_allowed());
    assert_eq!(
        lease.checkpoint().unwrap().unwrap().as_slice(),
        b"opaque-proof-and-cache"
    );
}

/// Generation-only records migrate without treating their optional SDK bytes as admitted cache.
#[test]
fn legacy_checkpoint_retains_proof_but_requires_conservative_cache_restore() {
    let fixture = Fixture::new();
    fixture.sign_in("legacy");
    let mut bytes = b"CNAC\x01".to_vec();
    bytes.extend_from_slice(&u16::try_from(b"legacy".len()).unwrap().to_le_bytes());
    bytes.extend_from_slice(b"legacy");
    bytes.extend_from_slice(b"original-proof");
    private_directory(&fixture.store.directory).unwrap();
    write_private(&checkpoint_path(&fixture.store, None), &bytes).unwrap();
    let mut lease = fixture.store.try_native_lease().unwrap();
    assert!(!lease.checkpoint_cache_allowed());
    let original = lease.checkpoint().unwrap().unwrap();
    assert_eq!(original.as_slice(), b"original-proof");
    lease.publish_active_checkpoint(&original).unwrap();
    drop(lease);
    let lease = fixture.store.try_native_lease().unwrap();
    assert!(lease.checkpoint_cache_allowed());
    assert_eq!(
        lease.checkpoint().unwrap().unwrap().as_slice(),
        original.as_slice()
    );
}
