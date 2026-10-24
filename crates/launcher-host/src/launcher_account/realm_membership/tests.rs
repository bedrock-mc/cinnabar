use super::*;

/// An unavailable selected account cannot prepare a membership request.
#[cfg(all(feature = "native-networking", not(target_arch = "wasm32")))]
#[tokio::test]
async fn missing_native_realm_membership_owner_remains_unavailable() {
    assert!(Source(None).prepare("fixture").await.is_err());
}

/// The actual worker rejects an already retired queued intent before dispatching service work.
#[tokio::test]
async fn realm_membership_queue_keeps_the_requesting_account_generation() {
    let shared = Arc::new(Mutex::new(Snapshot {
        realm_ticket: Some(1),
        ..Default::default()
    }));
    let (sender, receiver) = tokio::sync::mpsc::unbounded_channel();
    let (_wake, changed) = watch::channel(Wake::Account);
    let selected = Selection::capture(&shared.lock().unwrap(), 1);
    sender
        .send(Request::Run {
            ticket: 1,
            code: "fixture".into(),
            accept: true,
            selected,
        })
        .unwrap();
    shared.lock().unwrap().retire_account_data();
    drop(sender);
    worker::run(Arc::clone(&shared), receiver, changed).await;
    assert!(shared.lock().unwrap().realm_membership.is_none());
}
