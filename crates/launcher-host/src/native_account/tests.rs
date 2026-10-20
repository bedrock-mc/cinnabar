use super::*;
use std::{
    fs,
    future::Future,
    io::Write,
    task::{Context, Poll, Waker},
};

/// Creates only synthetic private OAuth bytes in a fresh test namespace.
fn seed(path: &std::path::Path, generation: &str) {
    seed_tokens(path, generation, "before-access", "before-refresh");
}

/// Writes synthetic OAuth revisions without touching any live account namespace.
fn seed_tokens(path: &std::path::Path, generation: &str, access: &str, refresh: &str) {
    let mut options = fs::OpenOptions::new();
    options.create(true).write(true).truncate(true);
    #[cfg(unix)]
    {
        use std::os::unix::fs::OpenOptionsExt;
        options.mode(0o600);
    }
    let mut file = options.open(path).unwrap();
    let bytes = serde_json::to_vec(&serde_json::json!({"access_token":access, "refresh_token":refresh, "cinnabar_sign_in_generation":generation})).unwrap();
    file.write_all(&bytes).unwrap();
}

/// A retired catalog owner is denied before restoring any SDK account or starting authentication.
#[tokio::test]
async fn catalog_generation_is_rejected_before_credential_restore_or_authentication() {
    let directory = tempfile::tempdir().unwrap();
    let path = directory.path().join("synthetic-oauth.json");
    seed(&path, "replacement-generation");
    let account = Arc::new(NativeAccount::new(
        AccountStore::new(path),
        directory.path().join("device.json"),
        None,
    ));
    // Reaching SDK restoration would fail locally with a different error, without contacting a service.
    account
        .store
        .try_native_lease()
        .unwrap()
        .publish_active_checkpoint(b"synthetic-invalid-sdk-checkpoint")
        .unwrap();
    let result = Arc::clone(&account)
        .acquire_verified_for(Some("retired-generation"))
        .await;
    let error = match result {
        Ok(_) => panic!("retired catalog generation reached authentication"),
        Err(error) => error,
    };
    assert_eq!(error.kind(), io::ErrorKind::PermissionDenied);
    assert!(account.state.lock().unwrap().cached.is_none());
    assert!(account.store.try_native_lease().is_ok());
    let error = Arc::clone(&account)
        .validate_generation("retired-generation")
        .await
        .unwrap_err();
    assert_eq!(error.kind(), io::ErrorKind::PermissionDenied);
    assert!(account.state.lock().unwrap().cached.is_none());
    assert!(account.store.try_native_lease().is_ok());
}

/// Dropping a later-stage acquisition preserves the SDK-visible refresh and retires replaced generations.
#[test]
fn cancelling_after_oauth_rotation_persists_before_releasing_the_lease() {
    let directory = tempfile::tempdir().unwrap();
    let path = directory.path().join("synthetic-oauth.json");
    seed(&path, "first");
    let account = Arc::new(NativeAccount::new(
        AccountStore::new(path.clone()),
        directory.path().join("device.json"),
        None,
    ));
    let mut operation = account.begin().unwrap();
    let lease = account.store.try_native_lease().unwrap();
    let policy = authentication_policy();
    let credentials = Credentials::from_tokens(
        policy.client_id.clone(),
        "acquired-access".into(),
        "rotated-refresh".into(),
        UNIX_EPOCH + Duration::from_secs(4_000_000_000),
    )
    .unwrap();
    let sdk = MicrosoftAuth::new(policy.clone())
        .unwrap()
        .into_account(credentials)
        .unwrap();
    let proof = sdk.checkpoint().unwrap();
    operation.cached = Some(Cached {
        provider: None,
        generation: lease.generation().into(),
        binding: lease.binding(),
        checkpoint_revision: lease.checkpoint_revision(),
        owner: Owner::Xbox(Box::new(sdk)),
        identity: None,
    });
    operation.lease = Some(lease);
    let mut pending = Box::pin(async move {
        let _operation = operation;
        std::future::pending::<()>().await;
    });
    assert!(matches!(
        pending
            .as_mut()
            .poll(&mut Context::from_waker(Waker::noop())),
        Poll::Pending
    ));
    assert_eq!(
        account.store.try_native_lease().err().unwrap().kind(),
        io::ErrorKind::WouldBlock
    );
    drop(pending);
    let lease = account.store.try_native_lease().unwrap();
    assert_eq!(lease.refresh_token(), "rotated-refresh");
    let bytes = lease.checkpoint().unwrap().unwrap();
    let restored = MicrosoftAuth::new(policy)
        .unwrap()
        .restore_account(xbox::Checkpoint::from_bytes(bytes).unwrap())
        .unwrap();
    assert_eq!(restored.checkpoint().unwrap().as_bytes(), proof.as_bytes());
    drop(lease);
    let oauth: Value = serde_json::from_slice(&fs::read(&path).unwrap()).unwrap();
    assert_eq!(oauth["access_token"], "acquired-access");
    assert_eq!(oauth["cinnabar_sign_in_generation"], "first");

    let mut operation = account.begin().unwrap();
    seed(&path, "replacement");
    operation.lease = Some(account.store.try_native_lease().unwrap());
    drop(operation);
    assert!(account.state.lock().unwrap().cached.is_none());
    let lease = account.store.try_native_lease().unwrap();
    assert_eq!(lease.generation(), "replacement");
    assert_eq!(lease.refresh_token(), "before-refresh");
    assert!(lease.checkpoint().unwrap().is_none());
}

/// A stale selected account never receives a different active account's OAuth or proof.
#[test]
fn unverified_rotation_does_not_overwrite_a_selected_saved_account() {
    for mismatch in [false, true] {
        let directory = tempfile::tempdir().unwrap();
        let path = directory.path().join("synthetic-oauth.json");
        seed(&path, "saved-a");
        let store = AccountStore::new(path.clone());
        store.remember_current("123", "Saved A", None).unwrap();
        store
            .try_native_lease()
            .unwrap()
            .publish_verified_identity("123", b"saved-a-proof")
            .unwrap();
        seed(&path, "active-b");
        let account = Arc::new(NativeAccount::new(
            store.clone(),
            directory.path().join("device.json"),
            Some("123".into()),
        ));
        let mut operation = account.begin().unwrap();
        let lease = store.try_native_lease().unwrap();
        let policy = authentication_policy();
        let sdk = MicrosoftAuth::new(policy.clone())
            .unwrap()
            .into_account(
                Credentials::from_tokens(
                    policy.client_id.clone(),
                    "acquired-b-access".into(),
                    "rotated-b-refresh".into(),
                    UNIX_EPOCH + Duration::from_secs(4_000_000_000),
                )
                .unwrap(),
            )
            .unwrap();
        operation.cached = Some(Cached {
            provider: None,
            generation: lease.generation().into(),
            binding: lease.binding(),
            checkpoint_revision: lease.checkpoint_revision(),
            owner: Owner::Xbox(Box::new(sdk)),
            identity: None,
        });
        operation.lease = Some(lease);
        if mismatch {
            assert_eq!(
                operation
                    .verify_identity("456", "Active B")
                    .unwrap_err()
                    .kind(),
                io::ErrorKind::PermissionDenied
            );
            drop(operation);
            assert!(account.state.lock().unwrap().cached.is_none());
        } else {
            let mut pending = Box::pin(async move {
                let _operation = operation;
                std::future::pending::<()>().await;
            });
            assert!(
                pending
                    .as_mut()
                    .poll(&mut Context::from_waker(Waker::noop()))
                    .is_pending()
            );
            drop(pending);
            assert!(
                account
                    .state
                    .lock()
                    .unwrap()
                    .cached
                    .as_ref()
                    .unwrap()
                    .identity
                    .is_none()
            );
        }
        let lease = store.try_native_lease().unwrap();
        assert_eq!(lease.generation(), "active-b");
        assert_eq!(lease.refresh_token(), "rotated-b-refresh");
        let checkpoint =
            xbox::Checkpoint::from_bytes(lease.checkpoint().unwrap().unwrap()).unwrap();
        let restored = MicrosoftAuth::new(policy)
            .unwrap()
            .restore_account(checkpoint)
            .unwrap();
        assert_eq!(restored.credentials().refresh_token(), "rotated-b-refresh");
        drop(lease);
        // Reopen A through the account store: neither its saved OAuth nor its proof was replaced by B.
        fs::remove_file(&path).unwrap();
        store.activate("123").unwrap();
        let lease = store.try_native_lease().unwrap();
        assert_eq!(lease.generation(), "saved-a");
        assert_eq!(lease.refresh_token(), "before-refresh");
        assert_eq!(
            lease.checkpoint().unwrap().unwrap().as_slice(),
            b"saved-a-proof"
        );
    }
}

/// An unusable large access token fails preparation while its latest bounded refresh remains durable.
#[test]
fn oversized_access_does_not_poison_go_fallback_or_lose_the_rotated_refresh() {
    let directory = tempfile::tempdir().unwrap();
    let path = directory.path().join("synthetic-oauth.json");
    seed(&path, "oversized");
    let account = Arc::new(NativeAccount::new(
        AccountStore::new(path.clone()),
        directory.path().join("device.json"),
        None,
    ));
    let mut operation = account.begin().unwrap();
    let lease = account.store.try_native_lease().unwrap();
    let policy = authentication_policy();
    let sdk = MicrosoftAuth::new(policy.clone())
        .unwrap()
        .into_account(
            Credentials::from_tokens(
                policy.client_id,
                "a".repeat(launcher::accounts::MAX_OAUTH_CACHE_BYTES),
                "rotated-refresh".into(),
                UNIX_EPOCH + Duration::from_secs(4_000_000_000),
            )
            .unwrap(),
        )
        .unwrap();
    operation.cached = Some(Cached {
        provider: None,
        generation: lease.generation().into(),
        binding: lease.binding(),
        checkpoint_revision: lease.checkpoint_revision(),
        owner: Owner::Xbox(Box::new(sdk)),
        identity: None,
    });
    operation.lease = Some(lease);
    assert_eq!(
        operation.persist().unwrap_err().kind(),
        io::ErrorKind::FileTooLarge
    );
    drop(operation);
    assert!(account.state.lock().unwrap().cached.is_none());
    let oauth: Value = serde_json::from_slice(&fs::read(&path).unwrap()).unwrap();
    assert_eq!(oauth["access_token"], "");
    assert_eq!(oauth["refresh_token"], "rotated-refresh");
    assert_eq!(oauth["expiry"], REFRESH_ONLY_EXPIRY);
    let lease = account.store.try_native_lease().unwrap();
    assert!(lease.checkpoint().unwrap().is_some());
}

/// Same-generation adoption preserves the service owner but denies minting for a changed XUID.
#[test]
fn changed_xuid_cannot_reuse_a_service_session_without_a_selected_account() {
    let directory = tempfile::tempdir().unwrap();
    let path = directory.path().join("synthetic-oauth.json");
    seed(&path, "one-generation");
    let account = Arc::new(NativeAccount::new(
        AccountStore::new(path.clone()),
        directory.path().join("device.json"),
        None,
    ));
    let policy = authentication_policy();
    let lease = account.store.try_native_lease().unwrap();
    let sdk = MicrosoftAuth::new(policy.clone())
        .unwrap()
        .into_account(import_credentials(&lease, &policy).unwrap())
        .unwrap();
    let config = minecraft::Config::desktop(
        protocol::GAME_VERSION,
        u32::try_from(protocol::PROTOCOL_VERSION).unwrap(),
        metadata::service_device_id("123").unwrap(),
    );
    let services = minecraft::Minecraft::new(sdk, config).unwrap();
    let session_id = services.session_id();
    let mut cached = Cached {
        provider: None,
        generation: lease.generation().into(),
        binding: lease.binding(),
        checkpoint_revision: lease.checkpoint_revision(),
        owner: Owner::Minecraft(Box::new(services)),
        identity: Some(AccountIdentity {
            xuid: "123".into(),
            gamertag: "Account A".into(),
            current_credential: true,
        }),
    };
    drop(lease);
    seed_tokens(
        &path,
        "one-generation",
        "current-b-access",
        "current-b-refresh",
    );
    let lease = account.store.try_native_lease().unwrap();
    cached
        .adopt_credentials(
            import_credentials(&lease, &policy).unwrap(),
            lease.binding(),
        )
        .unwrap();
    assert!(!cached.identity.as_ref().unwrap().current_credential);
    let Owner::Minecraft(services) = &cached.owner else {
        panic!("retained service owner")
    };
    assert_eq!(services.session_id(), session_id);
    let mut operation = account.begin().unwrap();
    operation.cached = Some(cached);
    operation.lease = Some(lease);
    assert_eq!(
        operation
            .verify_identity("456", "Account B")
            .unwrap_err()
            .kind(),
        io::ErrorKind::PermissionDenied
    );
    assert_eq!(
        operation.minecraft().err().unwrap().kind(),
        io::ErrorKind::PermissionDenied
    );
    drop(operation);
    assert!(account.state.lock().unwrap().cached.is_none());
    let lease = account.store.try_native_lease().unwrap();
    assert_eq!(lease.refresh_token(), "current-b-refresh");
    assert!(lease.checkpoint().unwrap().is_some());
}

/// A live process cannot republish its older cache after another lease persisted an eviction.
#[test]
fn stale_checkpoint_owner_cannot_undo_another_processes_rejection() {
    let directory = tempfile::tempdir().unwrap();
    let path = directory.path().join("synthetic-oauth.json");
    seed(&path, "one-generation");
    let account = Arc::new(NativeAccount::new(
        AccountStore::new(path),
        directory.path().join("device.json"),
        None,
    ));
    let sdk = MicrosoftAuth::new(authentication_policy())
        .unwrap()
        .into_account(
            Credentials::from_tokens(
                authentication_policy().client_id,
                "before-access".into(),
                "before-refresh".into(),
                UNIX_EPOCH + Duration::from_secs(4_000_000_000),
            )
            .unwrap(),
        )
        .unwrap();
    let mut lease = account.store.try_native_lease().unwrap();
    lease
        .publish_active_checkpoint(sdk.checkpoint().unwrap().as_bytes())
        .unwrap();
    let stale_binding = lease.binding();
    let stale_checkpoint_revision = lease.checkpoint_revision();
    drop(lease);
    account
        .store
        .try_native_lease()
        .unwrap()
        .publish_active_checkpoint(b"new-checkpoint-with-refused-issuance-removed")
        .unwrap();

    let mut operation = account.begin().unwrap();
    operation.cached = Some(Cached {
        generation: "one-generation".into(),
        binding: stale_binding,
        checkpoint_revision: stale_checkpoint_revision,
        owner: Owner::Xbox(Box::new(sdk)),
        identity: None,
        provider: None,
    });
    operation.lease = Some(account.store.try_native_lease().unwrap());
    assert_eq!(
        operation.persist().unwrap_err().kind(),
        io::ErrorKind::PermissionDenied
    );
    drop(operation);
    assert!(account.state.lock().unwrap().cached.is_none());
    let lease = account.store.try_native_lease().unwrap();
    assert_eq!(lease.refresh_token(), "before-refresh");
    assert_eq!(
        lease.checkpoint().unwrap().unwrap().as_slice(),
        b"new-checkpoint-with-refused-issuance-removed"
    );
}

/// A checkpoint cannot roll back protected OAuth, and conservative fallback retains its proof key.
#[test]
fn restore_keeps_the_proof_but_adopts_only_the_current_protected_refresh() {
    let directory = tempfile::tempdir().unwrap();
    let path = directory.path().join("synthetic-oauth.json");
    seed(&path, "one-generation");
    let store = AccountStore::new(path);
    let mut lease = store.try_native_lease().unwrap();
    lease
        .publish_oauth("current-access", "current-refresh", "2099-01-01T00:00:00Z")
        .unwrap();
    let mut original = MicrosoftAuth::new(authentication_policy())
        .unwrap()
        .into_account(
            Credentials::from_tokens(
                authentication_policy().client_id,
                "old-access".into(),
                "old-refresh".into(),
                UNIX_EPOCH + Duration::from_secs(4_000_000_000),
            )
            .unwrap(),
        )
        .unwrap();
    lease
        .publish_active_checkpoint(original.checkpoint().unwrap().as_bytes())
        .unwrap();
    let restored = restore::restore_account(&lease, &authentication_policy()).unwrap();
    assert_eq!(restored.credentials().access_token(), "");
    assert_eq!(restored.credentials().refresh_token(), "current-refresh");
    original
        .replace_credentials(import_credentials(&lease, &authentication_policy()).unwrap())
        .unwrap();
    assert_eq!(
        restored.checkpoint().unwrap().as_bytes(),
        original.checkpoint().unwrap().as_bytes()
    );
}

/// Shared preparation cannot borrow another selected account's acquired mutable owner.
#[tokio::test]
async fn borrowed_messaging_cannot_reuse_another_account_operation() {
    let directory = tempfile::tempdir().unwrap();
    let first = Arc::new(NativeAccount::new(
        AccountStore::new(directory.path().join("first.json")),
        directory.path().join("device.json"),
        None,
    ));
    let second = NativeAccount::new(
        AccountStore::new(directory.path().join("second.json")),
        directory.path().join("device.json"),
        None,
    );
    let mut operation = first.begin().unwrap();
    let target = protocol::NativeMessagingTarget::new(
        bedrock_net::messaging::Recipient::Messaging(uuid::Uuid::from_u128(1)),
        "synthetic-joined-challenge".into(),
    )
    .unwrap();
    let result = second
        .prepare_messaging_borrowed(
            &mut operation,
            target,
            serde_json::json!({}),
            protocol::ClientRandomId::new(1).unwrap(),
            tokio::time::Instant::now() + ACQUISITION_TIMEOUT,
        )
        .await;
    let error = match result {
        Ok(_) => panic!("another account's mutable operation was admitted"),
        Err(error) => error,
    };
    assert_eq!(error.kind(), io::ErrorKind::PermissionDenied);
}
