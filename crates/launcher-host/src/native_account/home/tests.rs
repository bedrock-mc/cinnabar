//! Protected synthetic-account controls; these do not authenticate or contact a service.

use super::*;
use std::{
    future::Future,
    task::{Context, Waker},
};

/// Uses the existing synthetic protected account and the actual SDK's generated messaging session.
async fn prepared() -> (tempfile::TempDir, Arc<NativeAccount>, NativeHome) {
    let (directory, account) = super::super::profile::tests::account();
    let mut operation = Arc::clone(&account).acquire_verified().await.unwrap();
    operation.promote().unwrap();
    let scope = NativeHomeScope {
        identity: ReadIdentity::capture(&operation),
        language: bridge::contract::DEFAULT_SERVICE_LANGUAGE.to_owned(),
        session: operation
            .minecraft()
            .unwrap()
            .player_messaging()
            .session_id(),
    };
    operation.persist().unwrap();
    (
        directory,
        account,
        NativeHome {
            operation,
            scope,
            invites: None,
            treatments: None,
            messages: None,
            head: None,
        },
    )
}

/// Partial parts retain the actual previous Home; an entirely failed read is not a cache success.
#[tokio::test]
async fn native_home_partial_parts_and_original_lease_survive_publication() {
    let (directory, account, mut loaded) = prepared().await;
    let images = crate::remote_images::ImageDirectory::new(
        directory.path().join("artwork"),
        crate::remote_images::LAUNCHER_ART,
    );
    let previous = bridge::Home {
        realm_invites: 4,
        treatments: vec!["kept".into()],
        messages: vec![bridge::Message {
            id: "kept".into(),
            ..Default::default()
        }],
        ..Default::default()
    };
    let (unchanged, any_success) = loaded.apply(&previous, &images).unwrap();
    assert!(!any_success);
    assert_eq!(unchanged, previous);
    loaded.invites = Some(0);
    let (partial, any_success) = loaded.apply(&previous, &images).unwrap();
    assert!(any_success);
    assert_eq!(partial.realm_invites, 0);
    assert_eq!(partial.treatments, previous.treatments);
    assert_eq!(partial.messages, previous.messages);
    assert_eq!(
        account.store.try_native_lease().err().unwrap().kind(),
        io::ErrorKind::WouldBlock
    );
    drop(loaded);
    assert!(account.store.try_native_lease().is_ok());
}

/// Reports retain the shown SDK session and actual lease; later acquisition cannot replace it.
#[tokio::test]
async fn native_home_report_preparation_is_bound_to_original_sdk_session_and_account() {
    let (_directory, account, loaded) = prepared().await;
    let scope = loaded.scope.clone();
    drop(loaded);
    let prepared = Arc::clone(&account)
        .prepare_home_report(&scope)
        .await
        .unwrap()
        .unwrap();
    assert_eq!(
        account.store.try_native_lease().err().unwrap().kind(),
        io::ErrorKind::WouldBlock
    );
    let mut next = Box::pin(Arc::clone(&account).prepare_home_report(&scope));
    assert!(
        next.as_mut()
            .poll(&mut Context::from_waker(Waker::noop()))
            .is_pending()
    );
    drop(next);
    assert_eq!(
        account.store.try_native_lease().err().unwrap().kind(),
        io::ErrorKind::WouldBlock
    );
    drop(prepared);

    let (_other_directory, other) = super::super::profile::tests::account();
    assert!(other.prepare_home_report(&scope).await.unwrap().is_none());
    let mut operation = Arc::clone(&account).acquire_verified().await.unwrap();
    // Actual checkpoint restoration starts another SDK marketing session on the same account proof.
    let restored =
        restore::restore_account(operation.lease.as_ref().unwrap(), &authentication_policy())
            .unwrap();
    operation.cached.as_mut().unwrap().owner = Owner::Xbox(Box::new(restored));
    operation.promote().unwrap();
    let replacement = operation
        .minecraft()
        .unwrap()
        .player_messaging()
        .session_id();
    assert_ne!(replacement, scope.session);
    operation.persist().unwrap();
    drop(operation);
    assert!(
        Arc::clone(&account)
            .prepare_home_report(&scope)
            .await
            .unwrap()
            .is_none(),
        "old IDs must never report in a replacement marketing session"
    );
}
