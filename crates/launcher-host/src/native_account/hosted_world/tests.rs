use super::*;
use crate::local_worlds::test_support::NativeHostFixture;
use launcher::accounts::AccountStore;
use std::{
    future::Future,
    path::Path,
    task::{Context, Poll, Waker},
    time::Duration,
};

/// Installs only a synthetic already-verified SDK owner; no service traffic creates this test identity.
fn account(root: &Path) -> Arc<NativeAccount> {
    use super::super::{AccountIdentity, Cached, Owner, authentication_policy};
    use bedrock_services::oauth::{Credentials, MicrosoftAuth};
    let policy = authentication_policy();
    let sdk = MicrosoftAuth::new(policy.clone())
        .unwrap()
        .into_account(
            Credentials::from_tokens(
                policy.client_id,
                "synthetic-host-access".into(),
                "synthetic-host-refresh".into(),
                std::time::UNIX_EPOCH + Duration::from_secs(4_000_000_000),
            )
            .unwrap(),
        )
        .unwrap();
    let account = Arc::new(NativeAccount::new(
        AccountStore::new(root.join("synthetic-oauth.json")),
        root.join("synthetic-device.json"),
        None,
    ));
    account.state.lock().unwrap().cached = Some(Cached {
        generation: "synthetic-host-generation".into(),
        binding: [0; 32],
        checkpoint_revision: [0; 32],
        owner: Owner::Xbox(Box::new(sdk)),
        identity: Some(AccountIdentity {
            xuid: "123".into(),
            gamertag: "Synthetic host".into(),
            current_credential: true,
        }),
        provider: None,
    });
    account
}

/// Supplies a finite phase budget; no test depends on wall-clock delay or sleeping.
fn deadline() -> Instant {
    Instant::now() + super::super::ACQUISITION_TIMEOUT
}

#[tokio::test]
async fn canceled_host_work_retains_capacity_and_closed_drop_cannot_release_a_new_host() {
    let fixture = NativeHostFixture::new();
    let namespace = tempfile::tempdir().unwrap();
    let account = account(namespace.path());
    let mut first = fixture
        .manager
        .native_hosted_world(Arc::clone(&account), deadline())
        .unwrap();
    drop(first.advance());
    let operation = account.begin().unwrap();
    let mut preparation = Box::pin(first.advance());
    assert!(matches!(
        preparation
            .as_mut()
            .poll(&mut Context::from_waker(Waker::noop())),
        Poll::Pending
    ));
    drop(preparation);
    drop(operation);
    assert_eq!(
        fixture
            .manager
            .native_hosted_world(Arc::clone(&account), deadline())
            .err()
            .unwrap()
            .kind(),
        io::ErrorKind::WouldBlock
    );
    assert_eq!(
        fixture.manager.native_host_cleanup().err().unwrap().kind(),
        io::ErrorKind::WouldBlock
    );
    first.close(deadline()).await.unwrap();
    let second = fixture
        .manager
        .native_hosted_world(Arc::clone(&account), deadline())
        .unwrap();
    drop(first);
    assert_eq!(
        fixture
            .manager
            .native_hosted_world(Arc::clone(&account), deadline())
            .err()
            .unwrap()
            .kind(),
        io::ErrorKind::WouldBlock,
        "a closed old owner cannot release a newer reservation"
    );
    drop(second);
    assert!(fixture.manager.native_host_cleanup().unwrap().is_none());
    assert!(!fixture.runner.last().stopped());
    assert!(!namespace.path().join("synthetic-oauth.json").exists());
}

#[tokio::test]
async fn reopening_the_same_world_revokes_the_original_host_before_account_or_network_io() {
    let fixture = NativeHostFixture::new();
    let namespace = tempfile::tempdir().unwrap();
    let account = account(namespace.path());
    let mut host = fixture
        .manager
        .native_hosted_world(Arc::clone(&account), deadline())
        .unwrap();
    let original_target = fixture.manager.core_report().target.unwrap();
    fixture.manager.close();
    test_time::eventually("original world closed", || {
        fixture.manager.status().state == bridge::WorldState::Idle
    });
    fixture.manager.open(&fixture.world_id, None).unwrap();
    test_time::eventually("same world reopened", || {
        fixture.manager.status().state == bridge::WorldState::Running
    });
    let replacement_target = fixture.manager.core_report().target.unwrap();
    assert_eq!(original_target.address, replacement_target.address);
    assert_eq!(
        host.advance().await.unwrap_err().kind(),
        io::ErrorKind::NotConnected,
        "same world/address must not authorize the old opening"
    );
    assert!(!namespace.path().join("synthetic-oauth.json").exists());
    host.close(deadline()).await.unwrap();
    let replacement = fixture
        .manager
        .native_hosted_world(account, deadline())
        .unwrap();
    drop(replacement);
    assert!(!fixture.runner.last().stopped());
}

#[tokio::test]
async fn selected_host_generation_is_checked_before_restoring_or_authenticating_a_replacement() {
    let fixture = NativeHostFixture::new();
    let namespace = tempfile::tempdir().unwrap();
    let account = account(namespace.path());
    let mut host = fixture
        .manager
        .native_hosted_world(Arc::clone(&account), deadline())
        .unwrap();
    crate::local_worlds::write_private(
        &namespace.path().join("synthetic-oauth.json"),
        br#"{"access_token":"synthetic-replacement-access","refresh_token":"synthetic-replacement-refresh","cinnabar_sign_in_generation":"replacement"}"#,
    )
    .unwrap();
    assert_eq!(
        host.advance().await.unwrap_err().kind(),
        io::ErrorKind::PermissionDenied
    );
    assert!(account.store.try_native_lease().is_ok());
    drop(host);
    assert!(fixture.manager.native_host_cleanup().unwrap().is_none());
    assert!(!fixture.runner.last().stopped());
}
