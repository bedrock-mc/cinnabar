use super::*;
use std::{
    fs,
    io::Write,
    os::unix::fs::{OpenOptionsExt, PermissionsExt, symlink},
    path::{Path, PathBuf},
    time::UNIX_EPOCH,
};

const ORIGINAL: &[u8] =
    br#"{"refresh_token":"fixture-refresh","cinnabar_sign_in_generation":"original"}"#;

struct Fixture {
    root: tempfile::TempDir,
    active: PathBuf,
    store: AccountStore,
    saved: Vec<(PathBuf, Vec<u8>)>,
}

impl Fixture {
    /// Uses the real account store to select a synthetic identity in private temporary storage.
    fn new() -> Self {
        let root = tempfile::tempdir().unwrap();
        let active = root.path().join("token.json");
        write_private(&active, ORIGINAL);
        let store = AccountStore::new(active.clone());
        store.remember_current("123", "Fixture", None).unwrap();
        let saved = snapshot(store.pending_cache().parent().unwrap());
        Self {
            root,
            active,
            store,
            saved,
        }
    }

    /// Invokes the original saved-account gate without constructing any provider request.
    async fn admission(&self) -> Result<Option<AccountProfile>, Failure> {
        let account = Arc::new(NativeAccount::new(
            self.store.clone(),
            self.root.path().join("device.json"),
            Some("123".into()),
        ));
        let (sender, _receiver) = bounded(super::super::super::EVENT_CAPACITY);
        let (_cancel, mut cancelled) = oneshot::channel();
        saved_profile(&self.store, account, Some("123"), &sender, &mut cancelled).await
    }

    /// Checks every original saved file without duplicating the account store's file schema.
    fn assert_saved(&self) {
        for (path, bytes) in &self.saved {
            assert_eq!(fs::read(path).unwrap(), *bytes);
        }
        assert_eq!(self.store.active_id().unwrap().as_deref(), Some("123"));
    }

    /// Removes only the original fixture after all borrowed operations and file leases are gone.
    fn finish(self) {
        self.root.close().unwrap();
    }
}

/// Writes only a fixture-owned regular path with the account reader's actual private mode.
fn write_private(path: &Path, bytes: &[u8]) {
    let mut file = fs::OpenOptions::new()
        .write(true)
        .create(true)
        .truncate(true)
        .mode(0o600)
        .open(path)
        .unwrap();
    file.write_all(bytes).unwrap();
}

/// Captures actual original files rather than assuming private saved-record names or formats.
fn snapshot(directory: &Path) -> Vec<(PathBuf, Vec<u8>)> {
    let mut result = Vec::new();
    for entry in fs::read_dir(directory).unwrap() {
        let entry = entry.unwrap();
        let kind = entry.file_type().unwrap();
        if kind.is_dir() {
            result.extend(snapshot(&entry.path()));
        } else if kind.is_file() {
            result.push((entry.path(), fs::read(entry.path()).unwrap()));
        }
    }
    result
}

/// Malformed credential contents must reach the existing interactive transaction, not a fatal cache gate.
#[tokio::test]
async fn native_invalid_cache_admits_interactive_recovery_without_replacing_saved_state() {
    let mut admitted = Vec::new();
    for contents in [
        b"".as_slice(),
        br#"{"refresh_token":"fixture""#,
        br#"{"refresh_token":7,"cinnabar_sign_in_generation":"original"}"#,
        br#"{"refresh_token":"","cinnabar_sign_in_generation":"original"}"#,
        br#"{"refresh_token":"fixture"}"#,
        br#"{"refresh_token":"fixture","cinnabar_sign_in_generation":"original"} {}"#,
        b"null",
    ] {
        let fixture = Fixture::new();
        write_private(&fixture.active, contents);
        admitted.push(matches!(fixture.admission().await, Ok(None)));
        assert_eq!(fs::read(&fixture.active).unwrap(), contents);
        fixture.assert_saved();
        let pending = fixture
            .store
            .try_begin_native_sign_in("recovery", Some("123"))
            .unwrap();
        drop(pending);
        fixture
            .store
            .try_discard_native_pending("recovery")
            .unwrap();
        assert_eq!(fs::read(&fixture.active).unwrap(), contents);
        fixture.assert_saved();
        assert!(
            fixture.store.try_snapshot().is_ok(),
            "saved admission retained its lease"
        );
        fixture.finish();
    }
    assert!(
        admitted.into_iter().all(|admitted| admitted),
        "trusted invalid contents did not reach interactive sign-in"
    );
}

/// Failed or canceled pending recovery never promotes credentials, and stale attempts cannot write.
#[tokio::test]
async fn native_invalid_cache_pending_recovery_retains_identity_revision_and_original_bytes() {
    for cancelled in [false, true] {
        let fixture = Fixture::new();
        let invalid = br#"{"refresh_token":"original-invalid""#;
        write_private(&fixture.active, invalid);
        let pending = fixture
            .store
            .try_begin_native_sign_in("recover", Some("123"))
            .unwrap();
        let policy = authentication_policy();
        let credentials = Credentials::from_tokens(
            policy.client_id.clone(),
            "fixture-access".into(),
            "new-refresh".into(),
            UNIX_EPOCH + Duration::from_secs(4_000_000_000),
        )
        .unwrap();
        assert!(persist(&pending, &credentials).await.is_ok());
        if cancelled {
            let mut account = MicrosoftAuth::new(policy)
                .unwrap()
                .into_account(credentials)
                .unwrap();
            let (cancel, mut cancelled) = oneshot::channel();
            cancel.send(()).unwrap();
            let (sender, _receiver) = bounded(super::super::super::EVENT_CAPACITY);
            let result = authorize_pending(&mut account, &pending, &sender, &mut cancelled).await;
            assert!(matches!(result, Err(Failure::Cancelled)));
            drop(account);
        } else {
            let wrong = AccountProfile::from_identity("456", "Other fixture").unwrap();
            assert_eq!(
                pending.try_commit(&wrong).unwrap_err().kind(),
                io::ErrorKind::PermissionDenied
            );
        }
        assert_eq!(fs::read(&fixture.active).unwrap(), invalid);
        fixture.assert_saved();
        drop(pending);
        fixture.store.try_discard_native_pending("recover").unwrap();
        assert!(!fixture.store.pending_cache().exists());
        assert!(fixture.store.try_snapshot().is_ok());
        fixture.finish();
    }

    let fixture = Fixture::new();
    let invalid = br#"{"refresh_token":"original-invalid""#;
    write_private(&fixture.active, invalid);
    assert_eq!(
        fixture
            .store
            .try_begin_native_sign_in("wrong-selection", Some("456"))
            .unwrap_err()
            .kind(),
        io::ErrorKind::PermissionDenied
    );
    let old = fixture
        .store
        .try_begin_native_sign_in("old", Some("123"))
        .unwrap();
    let current = fixture
        .store
        .try_begin_native_sign_in("current", Some("123"))
        .unwrap();
    current
        .try_publish("current-access", "current-refresh", "2099-01-01T00:00:00Z")
        .unwrap();
    let pending_bytes = fs::read(fixture.store.pending_cache()).unwrap();
    assert_eq!(
        old.try_publish("late", "late-refresh", "2099-01-01T00:00:00Z")
            .unwrap_err()
            .kind(),
        io::ErrorKind::PermissionDenied
    );
    fixture.store.try_discard_native_pending("old").unwrap();
    assert_eq!(
        fs::read(fixture.store.pending_cache()).unwrap(),
        pending_bytes
    );
    assert_eq!(
        old.try_commit(&AccountProfile::from_identity("123", "Fixture").unwrap())
            .unwrap_err()
            .kind(),
        io::ErrorKind::PermissionDenied
    );
    assert_eq!(fs::read(&fixture.active).unwrap(), invalid);
    write_private(&fixture.active, ORIGINAL);
    assert_eq!(
        current
            .try_publish("stale", "stale-refresh", "2099-01-01T00:00:00Z")
            .unwrap_err()
            .kind(),
        io::ErrorKind::PermissionDenied
    );
    assert_eq!(fs::read(&fixture.active).unwrap(), ORIGINAL);
    fixture.assert_saved();
    drop((old, current));
    fixture.store.try_discard_native_pending("current").unwrap();
    let lease = fixture.store.try_native_lease().unwrap();
    assert_eq!(lease.generation(), "original");
    drop(lease);
    fixture.finish();
}

/// Existing unsafe-file and filesystem refusals must never be mistaken for invalid credential contents.
#[tokio::test]
async fn native_invalid_cache_unsafe_files_stay_terminal_and_release_the_original_leases() {
    let mut refused = Vec::new();
    for kind in [
        "directory",
        "symlink",
        "permissions",
        "oversized",
        "filesystem",
    ] {
        let mut fixture = Fixture::new();
        match kind {
            "directory" => {
                fs::remove_file(&fixture.active).unwrap();
                fs::create_dir(&fixture.active).unwrap();
            }
            "symlink" => {
                let target = fixture.root.path().join("target.json");
                write_private(&target, b"invalid-fixture");
                fs::remove_file(&fixture.active).unwrap();
                symlink(target, &fixture.active).unwrap();
            }
            "permissions" => {
                fs::set_permissions(&fixture.active, fs::Permissions::from_mode(0o644)).unwrap()
            }
            "oversized" => write_private(
                &fixture.active,
                &vec![b' '; launcher::accounts::MAX_OAUTH_CACHE_BYTES + 1],
            ),
            "filesystem" => {
                let blocked = fixture.root.path().join("not-a-directory");
                write_private(&blocked, b"fixture");
                fixture.active = blocked.join("token.json");
                fixture.store = AccountStore::new(fixture.active.clone());
            }
            _ => unreachable!(),
        }
        refused.push(matches!(fixture.admission().await, Err(Failure::Failed(_))));
        match kind {
            "directory" => fs::remove_dir(&fixture.active).unwrap(),
            "filesystem" => {
                let blocked = fixture.active.parent().unwrap();
                fs::remove_file(blocked).unwrap();
                fs::create_dir(blocked).unwrap();
            }
            _ => fs::remove_file(&fixture.active).unwrap(),
        }
        write_private(&fixture.active, ORIGINAL);
        let lease = fixture.store.try_native_lease().unwrap();
        assert_eq!(lease.generation(), "original");
        drop(lease);
        fixture.finish();
    }
    assert!(
        refused.into_iter().all(|refused| refused),
        "an unsafe cache reached interactive recovery"
    );
}
