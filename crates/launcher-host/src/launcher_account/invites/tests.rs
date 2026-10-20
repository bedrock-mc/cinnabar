use super::*;

#[cfg(unix)]
use tokio::{
    io::{AsyncReadExt, AsyncWriteExt},
    net::{UnixListener, UnixStream},
};

/// Reads one real framed bridge request after the existing client actually dispatched it.
#[cfg(unix)]
async fn request(listener: &UnixListener) -> (UnixStream, serde_json::Value) {
    let (mut stream, _) = listener.accept().await.unwrap();
    let length = stream.read_u32().await.unwrap();
    let mut bytes = vec![0; length as usize];
    stream.read_exact(&mut bytes).await.unwrap();
    let request: serde_json::Value = serde_json::from_slice(&bytes).unwrap();
    assert_eq!(request["method"], bridge::contract::METHOD_FRIENDS_PEOPLE);
    (stream, request)
}

/// Core remains the default reader and its real framed result reaches the same artwork/presentation owner.
#[cfg(unix)]
#[tokio::test]
async fn core_invite_people_keep_the_existing_rpc_and_rows() {
    let directory = tempfile::tempdir().unwrap();
    let listener = UnixListener::bind(directory.path().join("control.sock")).unwrap();
    let shared = Mutex::new(Snapshot::default());
    let selected = Selection::capture(&shared.lock().unwrap());
    let (wake, mut changed) = watch::channel(Wake::Account);
    let images = artwork::people_images(&directory.path().join("artwork"));
    let reading = people(
        Some(directory.path()),
        &images,
        &shared,
        &selected,
        &mut changed,
    );
    let server = async {
        let (mut stream, request) = request(&listener).await;
        retire(&wake);
        let body = serde_json::to_vec(&serde_json::json!({
            "jsonrpc":"2.0", "id":request["id"], "result": {
                "schema_version":bridge::contract::CONTROL_SCHEMA_VERSION,
                "friends":[{"xuid":"456", "gamertag":"Original", "online":true}]
            }
        }))
        .unwrap();
        stream.write_u32(body.len() as u32).await.unwrap();
        stream.write_all(&body).await.unwrap();
    };
    let (people, ()) =
        tokio::time::timeout(RESPONSE_TIMEOUT, async { tokio::join!(reading, server) })
            .await
            .unwrap();
    let people = people.unwrap();
    assert_eq!(people.len(), 1);
    assert_eq!(people[0].xuid, "456");
    assert_eq!(people[0].gamertag, "Original");
    assert!(people[0].online);
}

/// A generation wake cancels the original live RPC, rather than starting it again for another account.
#[cfg(unix)]
#[tokio::test]
async fn account_retirement_cancels_the_actual_invite_people_rpc() {
    let directory = tempfile::tempdir().unwrap();
    let listener = UnixListener::bind(directory.path().join("control.sock")).unwrap();
    let (wake, mut changed) = watch::channel(Wake::Account);
    let shared = Mutex::new(Snapshot {
        people_wake: Some(wake),
        ..Default::default()
    });
    let selected = Selection::capture(&shared.lock().unwrap());
    let images = artwork::people_images(&directory.path().join("artwork"));
    let reading = people(
        Some(directory.path()),
        &images,
        &shared,
        &selected,
        &mut changed,
    );
    tokio::pin!(reading);
    let (mut stream, _) = tokio::select! {
        request = request(&listener) => request,
        _ = &mut reading => panic!("actual request must remain pending before retirement"),
    };
    shared.lock().unwrap().retire_account_data();
    assert!(reading.await.is_err());
    let mut byte = [0];
    assert_eq!(
        tokio::time::timeout(RESPONSE_TIMEOUT, stream.read(&mut byte))
            .await
            .unwrap()
            .unwrap(),
        0,
        "retirement must close the actual RPC"
    );
    assert!(shared.lock().unwrap().people.is_none());
}

/// Terminal worker cancellation closes the actual pending stream and cannot be undone by a late account wake.
#[cfg(unix)]
#[tokio::test]
async fn stopping_invite_worker_cancels_the_actual_people_rpc() {
    let directory = tempfile::tempdir().unwrap();
    let listener = UnixListener::bind(directory.path().join("control.sock")).unwrap();
    let (wake, mut changed) = watch::channel(Wake::Account);
    let (requests, _queued) = crossbeam_channel::unbounded();
    let owner = Requests {
        requests,
        wake: wake.clone(),
    };
    let shared = Mutex::new(Snapshot::default());
    let selected = Selection::capture(&shared.lock().unwrap());
    let images = artwork::people_images(&directory.path().join("artwork"));
    let reading = people(
        Some(directory.path()),
        &images,
        &shared,
        &selected,
        &mut changed,
    );
    tokio::pin!(reading);
    let (mut stream, _) = tokio::select! {
        request = request(&listener) => request,
        _ = &mut reading => panic!("actual request must remain pending before stop"),
    };
    drop(owner);
    retire(&wake);
    assert!(*wake.borrow() == Wake::Stop);
    assert!(reading.await.is_err());
    assert_eq!(
        tokio::time::timeout(RESPONSE_TIMEOUT, stream.read(&mut [0]))
            .await
            .unwrap()
            .unwrap(),
        0
    );
}

/// A missing selected native owner cannot disclose a different Go account's people.
#[cfg(all(unix, feature = "native-networking", not(target_arch = "wasm32")))]
#[tokio::test]
async fn missing_native_people_owner_never_falls_back_to_core() {
    let directory = tempfile::tempdir().unwrap();
    let listener =
        std::os::unix::net::UnixListener::bind(directory.path().join("control.sock")).unwrap();
    listener.set_nonblocking(true).unwrap();
    assert!(
        Source::Native(None)
            .fetch(Some(directory.path()))
            .await
            .is_err()
    );
    assert_eq!(
        listener.accept().unwrap_err().kind(),
        std::io::ErrorKind::WouldBlock
    );
}

/// A selected native host without an acknowledged incarnation cannot dispatch through an unrelated Go host.
#[cfg(all(unix, feature = "native-networking", not(target_arch = "wasm32")))]
#[tokio::test]
async fn unavailable_native_invitation_never_dispatches_core_rpc() {
    let directory = tempfile::tempdir().unwrap();
    let listener =
        std::os::unix::net::UnixListener::bind(directory.path().join("control.sock")).unwrap();
    listener.set_nonblocking(true).unwrap();
    assert!(
        Destination::Native(None)
            .send(Some(directory.path()), "456")
            .await
            .is_err()
    );
    assert_eq!(
        listener.accept().unwrap_err().kind(),
        std::io::ErrorKind::WouldBlock
    );
}
