//! A retired original presence generation must fail before restoring or using another account.
use super::*;
use std::{fs, io::Write};

/// Creates an already verified synthetic account on the real protected account/SDK owners.
pub(crate) fn verified_account(namespace: &std::path::Path) -> Arc<NativeAccount> {
    let path = namespace.join("synthetic-presence-oauth.json");
    let mut options = fs::OpenOptions::new();
    options.create_new(true).write(true);
    #[cfg(unix)]
    {
        use std::os::unix::fs::OpenOptionsExt;
        options.mode(0o600);
    }
    options
        .open(&path)
        .unwrap()
        .write_all(
            serde_json::to_vec(&serde_json::json!({
                "access_token":"synthetic-access", "refresh_token":"synthetic-refresh",
                "cinnabar_sign_in_generation":"original-presence"
            }))
            .unwrap()
            .as_slice(),
        )
        .unwrap();
    let account = Arc::new(NativeAccount::new(
        AccountStore::new(path),
        namespace.join("device.json"),
        None,
    ));
    let lease = account.store.try_native_lease().unwrap();
    let policy = authentication_policy();
    let credentials = Credentials::from_tokens(
        policy.client_id.clone(),
        "synthetic-access".into(),
        "synthetic-refresh".into(),
        UNIX_EPOCH + Duration::from_secs(4_000_000_000),
    )
    .unwrap();
    let sdk = MicrosoftAuth::new(policy)
        .unwrap()
        .into_account(credentials)
        .unwrap();
    account.state.lock().unwrap().cached = Some(Cached {
        provider: None,
        generation: lease.generation().into(),
        binding: lease.binding(),
        checkpoint_revision: lease.checkpoint_revision(),
        owner: Owner::Xbox(Box::new(sdk)),
        identity: Some(AccountIdentity {
            xuid: "123".into(),
            gamertag: "Synthetic".into(),
            current_credential: true,
        }),
    });
    drop(lease);
    account
}

/// Holds the same actual operation and protected lease as another catalog/service phase.
pub(crate) fn hold_operation(account: &Arc<NativeAccount>) -> Box<dyn FnOnce()> {
    let mut operation = account.begin().unwrap();
    operation.lease = Some(account.store.try_native_lease().unwrap());
    Box::new(move || drop(operation))
}

#[tokio::test]
async fn replacement_generation_cannot_receive_original_presence_update_or_removal() {
    let directory = tempfile::tempdir().unwrap();
    let path = directory.path().join("synthetic-presence-oauth.json");
    let mut options = fs::OpenOptions::new();
    options.create_new(true).write(true);
    #[cfg(unix)]
    {
        use std::os::unix::fs::OpenOptionsExt;
        options.mode(0o600);
    }
    let mut file = options.open(&path).unwrap();
    file.write_all(
        serde_json::to_vec(&serde_json::json!({
            "refresh_token": "synthetic-refresh", "cinnabar_sign_in_generation": "replacement"
        }))
        .unwrap()
        .as_slice(),
    )
    .unwrap();
    drop(file);
    let account = Arc::new(NativeAccount::new(
        AccountStore::new(path),
        directory.path().join("device.json"),
        None,
    ));
    // Reaching restoration would fail with a different error, without contacting a service.
    account
        .store
        .try_native_lease()
        .unwrap()
        .publish_active_checkpoint(b"synthetic-invalid-sdk-checkpoint")
        .unwrap();
    let scope = PresenceScope {
        generation: "original".into(),
        xuid: "123".into(),
    };
    for removal in [false, true] {
        let mut dispatched = false;
        let result = if removal {
            Arc::clone(&account).remove_title_presence(&scope).await
        } else {
            Arc::clone(&account)
                .update_title_presence(
                    &scope,
                    &bridge::XboxPresenceState::default(),
                    &mut dispatched,
                )
                .await
                .map(|_| ())
        };
        assert_eq!(result.unwrap_err().kind(), io::ErrorKind::PermissionDenied);
        assert!(!dispatched, "retired account reached title dispatch");
        assert!(account.state.lock().unwrap().cached.is_none());
        assert!(account.store.try_native_lease().is_ok());
    }
}
