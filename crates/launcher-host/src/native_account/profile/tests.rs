use super::*;
use serde_json::json;
use std::{
    future::Future,
    io::Write,
    task::{Context, Poll, Waker},
};

/// Uses actual SDK-decoded optional data, without an account or service request.
#[test]
fn profile_projection_keeps_verified_identity_privacy_absence_and_precise_counts() {
    let mut profile = bridge::Profile {
        xuid: "123".into(),
        gamertag: "Verified".into(),
        ..Default::default()
    };
    let user = serde_json::from_value(json!({"xuid":"123", "gamertag":"Untrusted display",
        "realName":"  Private name  ", "presenceText":"  In menus ",
        "displayPicRaw":"https://images.example.test/avatar.png", "gamerScore":"9223372036854775808"})).unwrap();
    apply_user(&mut profile, user).unwrap();
    assert_eq!(profile.gamertag, "Verified");
    assert_eq!(profile.real_name, "Private name");
    assert_eq!(profile.presence_text, "In menus");
    assert_eq!(profile.gamerscore, None);
    assert!(profile.gamerpic.path.is_empty());
    assert!(profile.gamerpic.url.starts_with("https://"));
    assert_eq!(count(Ok(0)).unwrap(), Some(0));
    assert_eq!(
        count(Err(bedrock_services::Error::Invalid(
            "synthetic unavailable"
        )))
        .unwrap(),
        None
    );
    let wrong = serde_json::from_value(json!({"xuid":"456"})).unwrap();
    assert_eq!(
        apply_user(&mut profile, wrong).unwrap_err().kind(),
        io::ErrorKind::PermissionDenied
    );
    let private =
        serde_json::from_value(json!({"xuid":"123", "displayPicRaw":"file:///private/avatar"}))
            .unwrap();
    apply_user(&mut profile, private).unwrap();
    assert!(profile.real_name.is_empty());
    assert!(profile.gamerpic.url.is_empty());
}

/// Writes only synthetic protected credentials through the fixture's real bounded file boundary.
pub(in crate::native_account) fn seed(path: &std::path::Path, generation: &str) {
    let mut options = std::fs::OpenOptions::new();
    options.create(true).write(true).truncate(true);
    #[cfg(unix)]
    {
        use std::os::unix::fs::OpenOptionsExt;
        options.mode(0o600);
    }
    let mut file = options.open(path).unwrap();
    file.write_all(
        serde_json::to_vec(
            &json!({"refresh_token":"synthetic-refresh", "cinnabar_sign_in_generation":generation}),
        )
        .unwrap()
        .as_slice(),
    )
    .unwrap();
}

/// Supplies an actual protected lease and SDK checkpoint without sending any credential.
pub(in crate::native_account) fn account() -> (tempfile::TempDir, Arc<NativeAccount>) {
    let directory = tempfile::tempdir().unwrap();
    let path = directory.path().join("oauth.json");
    seed(&path, "original");
    let account = Arc::new(NativeAccount::new(
        AccountStore::new(path),
        PathBuf::new(),
        None,
    ));
    let policy = authentication_policy();
    let credentials = Credentials::from_tokens(
        policy.client_id.clone(),
        String::new(),
        "synthetic-refresh".into(),
        UNIX_EPOCH,
    )
    .unwrap();
    let sdk = MicrosoftAuth::new(policy)
        .unwrap()
        .into_account(credentials)
        .unwrap();
    let mut lease = account.store.try_native_lease().unwrap();
    lease
        .publish_active_checkpoint(sdk.checkpoint().unwrap().as_bytes())
        .unwrap();
    account.state.lock().unwrap().cached = Some(Cached {
        generation: "original".into(),
        binding: lease.binding(),
        checkpoint_revision: lease.checkpoint_revision(),
        owner: Owner::Xbox(Box::new(sdk)),
        provider: None,
        identity: Some(AccountIdentity {
            xuid: "123".into(),
            gamertag: "Verified".into(),
            current_credential: true,
        }),
    });
    drop(lease);
    (directory, account)
}

/// A refresh revision does not retire presentation; a replacement generation or XUID does.
#[test]
fn completed_profile_remains_bound_to_selected_generation_after_artwork() {
    let (directory, account) = account();
    let result = || NativeProfile {
        account: Arc::clone(&account),
        generation: "original".into(),
        partial: false,
        avatar: None,
        profile: bridge::Profile {
            xuid: "123".into(),
            ..Default::default()
        },
    };
    let mut lease = account.store.try_native_lease().unwrap();
    lease.suppress_checkpoint().unwrap();
    let mut state = account.state.lock().unwrap();
    let cached = state.cached.as_mut().unwrap();
    cached.binding = lease.binding();
    cached.checkpoint_revision = lease.checkpoint_revision();
    drop(state);
    drop(lease);
    assert!(result().into_current().is_some());
    seed(&directory.path().join("oauth.json"), "replacement");
    assert!(result().into_current().is_none());
}

/// Even a matching storage generation cannot authorize another Xbox identity's presentation.
#[test]
fn completed_profile_rejects_another_verified_xuid() {
    let (_directory, account) = account();
    let mut state = account.state.lock().unwrap();
    let cached = state.cached.as_mut().unwrap();
    cached.identity.as_mut().unwrap().xuid = "456".into();
    drop(state);
    assert!(
        NativeProfile {
            account,
            generation: "original".into(),
            partial: false,
            avatar: None,
            profile: bridge::Profile {
                xuid: "123".into(),
                ..Default::default()
            },
        }
        .into_current()
        .is_none()
    );
}

/// Completed profile fields survive the original account's real checkout after artwork.
#[tokio::test(start_paused = true)]
async fn completed_native_read_survives_same_account_checkout() {
    let (_directory, account) = account();
    let loaded = {
        let state = account.state.lock().unwrap();
        let cached = state.cached.as_ref().unwrap();
        NativeProfile {
            account: Arc::clone(&account),
            generation: cached.generation.clone(),
            partial: false,
            avatar: None,
            profile: bridge::Profile {
                xuid: cached.identity.as_ref().unwrap().xuid.clone(),
                gamertag: "Completed profile".into(),
                friends: Some(7),
                ..Default::default()
            },
        }
    };
    let contender = account.begin().unwrap();
    let checked_out = {
        let state = account.state.lock().unwrap();
        state.busy && state.cached.is_none()
    };
    let mut publishing = Box::pin(async { loaded.into_current() });
    let first = publishing
        .as_mut()
        .poll(&mut Context::from_waker(Waker::noop()));
    let retained = first.is_pending();
    drop(contender);
    let profile = match first {
        Poll::Pending => publishing.await,
        Poll::Ready(profile) => profile,
    };
    assert!(checked_out, "the actual account owner was not checked out");
    assert!(
        retained,
        "ordinary account checkout discarded the completed profile"
    );
    let profile = profile.expect("the completed original profile remains current");
    assert_eq!(profile.gamertag, "Completed profile");
    assert_eq!(profile.friends, Some(7));
}

/// Canceling a profile waiting for a live session's Operation never steals or retires that owner.
#[tokio::test]
async fn cancelled_profile_waiter_preserves_unrelated_account_operation() {
    let (_directory, account) = account();
    let operation = account.begin().unwrap();
    let mut waiting = Box::pin(Arc::clone(&account).read_profile());
    assert!(matches!(
        waiting
            .as_mut()
            .poll(&mut Context::from_waker(Waker::noop())),
        Poll::Pending
    ));
    drop(waiting);
    assert!(account.state.lock().unwrap().busy);
    drop(operation);
    assert!(!account.state.lock().unwrap().busy);
    assert!(account.state.lock().unwrap().cached.is_some());
}

/// The original optional budget drops a real account waiter while keeping the loaded overview and its live owner.
#[tokio::test(start_paused = true)]
async fn optional_profile_cutoff_preserves_loaded_data_and_unrelated_operation() {
    let (_directory, account) = account();
    let held = account.begin().unwrap();
    let mut loaded = NativeProfile {
        profile: bridge::Profile {
            xuid: "123".into(),
            gamertag: "Loaded".into(),
            friends: Some(7),
            ..Default::default()
        },
        partial: false,
        account: Arc::clone(&account),
        generation: "original".into(),
        avatar: None,
    };
    let deadline = tokio::time::Instant::now() + Duration::from_secs(1);
    let mut enriching = Box::pin(loaded.enrich(deadline));
    assert!(
        enriching
            .as_mut()
            .poll(&mut Context::from_waker(Waker::noop()))
            .is_pending()
    );
    tokio::time::advance(Duration::from_secs(1)).await;
    enriching.await.unwrap();
    assert_eq!(loaded.profile.gamertag, "Loaded");
    assert_eq!(loaded.profile.friends, Some(7));
    assert!(loaded.partial);
    assert!(account.state.lock().unwrap().busy);
    drop(held);
    assert!(loaded.into_current().is_some());
}

/// Optional enrichment cannot continue under a replaced sign-in even when its prior DTO remains loaded.
#[tokio::test]
async fn optional_profile_facet_rejects_replacement_generation_before_authentication() {
    let (directory, account) = account();
    let mut loaded = NativeProfile {
        profile: bridge::Profile {
            xuid: "123".into(),
            ..Default::default()
        },
        partial: false,
        account,
        generation: "original".into(),
        avatar: None,
    };
    seed(&directory.path().join("oauth.json"), "replacement");
    let error = loaded
        .enrich(tokio::time::Instant::now() + Duration::from_secs(1))
        .await
        .unwrap_err();
    assert_eq!(error.kind(), io::ErrorKind::PermissionDenied);
    assert!(loaded.into_current().is_none());
}
