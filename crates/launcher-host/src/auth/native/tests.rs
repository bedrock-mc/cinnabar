use super::*;
use std::time::UNIX_EPOCH;

/// Cancellation of a later wait persists the concrete account's acquired refresh in the pending slot only.
#[test]
fn cancelled_later_exchange_preserves_pending_refresh_and_discards_late_ui_completion() {
    let layout = launcher::test_support::scratch("native-signin-cancel");
    let store = AccountStore::new(layout.auth_cache());
    let pending = store
        .try_begin_native_sign_in("synthetic-attempt", None)
        .unwrap();
    let policy = authentication_policy();
    let credentials = Credentials::from_tokens(
        policy.client_id.clone(),
        "fixture-access".into(),
        "rotated-refresh".into(),
        UNIX_EPOCH + Duration::from_secs(4_000_000_000),
    )
    .unwrap();
    let mut account = MicrosoftAuth::new(policy)
        .unwrap()
        .into_account(credentials)
        .unwrap();
    let runtime = tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
        .unwrap();
    let (cancel, mut cancelled) = oneshot::channel();
    cancel.send(()).unwrap();
    let (presentation, _) = bounded(super::super::EVENT_CAPACITY);
    let result = runtime.block_on(authorize_pending(
        &mut account,
        &pending,
        &presentation,
        &mut cancelled,
    ));
    assert!(matches!(result, Err(Failure::Cancelled)));
    assert!(
        !layout.auth_cache().exists(),
        "an unverified sign-in became active"
    );
    let (sender, receiver) = bounded(super::super::EVENT_CAPACITY);
    let (cancel, _) = oneshot::channel();
    let mut owner = NativeAuth {
        receiver,
        cancel: Some(cancel),
        done: Arc::new(AtomicBool::new(true)),
        state: AuthState::AwaitingCode {
            uri: "https://example.invalid/device".into(),
            code: "FIXTURE".into(),
        },
        profile: None,
        cancelled: false,
        generation: "synthetic-attempt".into(),
    };
    owner.cancel(true);
    sender
        .try_send(Message::Verified(
            AccountProfile {
                id: "123".into(),
                gamertag: "Synthetic".into(),
                picture_path: None,
            },
            Some(pending),
        ))
        .unwrap_or_else(|_| panic!("fixture delivery failed"));
    owner.poll();
    assert_eq!(owner.state(), &AuthState::SignedOut);
    assert!(
        owner.take_profile().is_none(),
        "a cancelled UI queued an account commit"
    );
    let pending_store = AccountStore::new(store.pending_cache());
    let lease = pending_store.try_native_lease().unwrap();
    assert_eq!(lease.refresh_token(), "rotated-refresh");
    drop(lease);
    test_time::eventually(
        "the cancelled native attempt to release its pending files",
        || match store.try_discard_native_pending("synthetic-attempt") {
            Ok(()) => true,
            Err(error) if error.kind() == io::ErrorKind::WouldBlock => false,
            Err(error) => panic!("pending cleanup failed: {error}"),
        },
    );
    assert!(!store.pending_cache().exists());
}

/// An oversized access token fails sign-in while retaining a Go-readable refresh-only pending record.
#[test]
fn oversized_access_cannot_poison_shared_pending_oauth_reader() {
    let layout = launcher::test_support::scratch("native-signin-oauth-limit");
    let store = AccountStore::new(layout.auth_cache());
    let pending = store
        .try_begin_native_sign_in("limit-attempt", None)
        .unwrap();
    let credentials = Credentials::from_tokens(
        authentication_policy().client_id,
        "x".repeat(launcher::accounts::MAX_OAUTH_CACHE_BYTES),
        "latest-refresh".into(),
        UNIX_EPOCH + Duration::from_secs(4_000_000_000),
    )
    .unwrap();
    let runtime = tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
        .unwrap();
    assert!(runtime.block_on(persist(&pending, &credentials)).is_err());
    let bytes = std::fs::read(store.pending_cache()).unwrap();
    assert!(bytes.len() <= launcher::accounts::MAX_OAUTH_CACHE_BYTES);
    let json: serde_json::Value = serde_json::from_slice(&bytes).unwrap();
    assert_eq!(json["refresh_token"], "latest-refresh");
    assert_eq!(json["access_token"], "");
    assert!(!layout.auth_cache().exists());
    store.try_discard_native_pending("limit-attempt").unwrap();
}
