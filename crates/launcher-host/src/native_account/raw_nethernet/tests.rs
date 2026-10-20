use super::*;
use std::{
    future::Future,
    task::{Context, Poll, Waker},
};

/// Raw preparation retains the real busy Operation; syntax failure must not wait for that lease.
#[tokio::test]
async fn raw_preparation_cancellation_and_invalid_syntax_preserve_the_selected_owner() {
    let directory = tempfile::tempdir().unwrap();
    let account = Arc::new(NativeAccount::new(
        AccountStore::new(directory.path().join("synthetic-oauth.json")),
        directory.path().join("synthetic-device.json"),
        None,
    ));
    let held = account.begin().unwrap();
    let address = format!(
        "{}{}/42",
        bridge::contract::NETHERNET_TARGET_PREFIX,
        bridge::contract::NETHERNET_SIGNALING_WEBSOCKET
    );
    let mut preparing = Box::pin(Arc::clone(&account).prepare_connection(
        protocol::NativeConnectionTarget::new(address, protocol::NativeTransport::RakNet),
        serde_json::json!({}),
        protocol::ClientRandomId::new(1).unwrap(),
    ));
    assert!(
        preparing
            .as_mut()
            .poll(&mut Context::from_waker(Waker::noop()))
            .is_pending()
    );
    drop(preparing);
    assert_eq!(
        account.begin().err().unwrap().kind(),
        io::ErrorKind::WouldBlock
    );
    let invalid = format!(
        "{}{}/host.invalid",
        bridge::contract::NETHERNET_TARGET_PREFIX,
        bridge::contract::NETHERNET_SIGNALING_WEBSOCKET
    );
    let mut preparing = Box::pin(Arc::clone(&account).prepare_connection(
        protocol::NativeConnectionTarget::new(invalid, protocol::NativeTransport::RakNet),
        serde_json::json!({}),
        protocol::ClientRandomId::new(1).unwrap(),
    ));
    let Poll::Ready(Err(error)) = preparing
        .as_mut()
        .poll(&mut Context::from_waker(Waker::noop()))
    else {
        panic!("malformed raw service target waited for credentials or reached transport work");
    };
    assert_eq!(error.kind(), io::ErrorKind::InvalidInput);
    drop(preparing);
    drop(held);
    drop(account.begin().unwrap());
}
