use super::*;
use std::{
    io::{Read, Write},
    os::unix::net::UnixListener,
};

/// Waits for an asynchronous answer, with a deadline only to bound fixture failures.
fn answer(shared: &Arc<Mutex<Snapshot>>) {
    test_time::wait_until(Duration::from_secs(2), || {
        shared.lock().unwrap().realm_membership.is_some()
    });
    assert!(shared.lock().unwrap().realm_membership.is_some());
}

#[test]
fn accepted_membership_survives_the_account_catalog_snapshot() {
    let dir = std::env::temp_dir().join(format!("realm-worker-{}", std::process::id()));
    std::fs::create_dir_all(&dir).unwrap();
    let listener = UnixListener::bind(dir.join("control.sock")).unwrap();
    let fixture = thread::spawn(move || {
        for accept in [false, true] {
            let (mut stream, _) = listener.accept().unwrap();
            let mut size = [0; 4];
            stream.read_exact(&mut size).unwrap();
            let mut bytes = vec![0; u32::from_be_bytes(size) as usize];
            stream.read_exact(&mut bytes).unwrap();
            let request: serde_json::Value = serde_json::from_slice(&bytes).unwrap();
            assert_eq!(request["method"], "realm_membership.v1");
            assert_eq!(request["params"]["accept"], accept);
            let response = serde_json::to_vec(&serde_json::json!({
                "jsonrpc":"2.0", "id":request["id"], "result":{
                    "schema_version":1, "code":"invitation", "realm":{
                        "name":"Accepted Realm", "state":"OPEN", "target":"realm_id/7", "member":true
                    }
                }
            })).unwrap();
            stream
                .write_all(&(response.len() as u32).to_be_bytes())
                .unwrap();
            stream.write_all(&response).unwrap();
        }
    });
    let shared = Arc::new(Mutex::new(Snapshot {
        realms: Some(Vec::new()),
        ..Default::default()
    }));
    let runtime = worker_runtime();
    let tasks = TaskTracker::new();
    let requests = start(
        Some(dir.clone()),
        Arc::clone(&shared),
        runtime.handle().clone(),
        tasks.clone(),
    );
    assert!(requests.run(&shared, 1, "invitation".into(), false));
    answer(&shared);
    {
        let mut snapshot = shared.lock().unwrap();
        assert!(
            snapshot.realms.as_ref().unwrap().is_empty(),
            "preview must not grant membership"
        );
        snapshot.realm_membership = None;
    }
    assert!(requests.run(&shared, 2, "invitation".into(), true));
    answer(&shared);
    {
        let snapshot = shared.lock().unwrap();
        let realms = snapshot.realms.as_ref().unwrap();
        assert_eq!(
            realms.len(),
            1,
            "catalog polling must retain the accepted Realm on subsequent frames"
        );
        assert_eq!(realms[0].target, "realm_id/7");
    }
    drop(requests);
    fixture.join().unwrap();
    finish_workers(runtime, tasks);
    std::fs::remove_dir_all(dir).unwrap();
}

/// A selected native failure must not send an invitation to another account's Go owner.
#[cfg(all(feature = "native-networking", not(target_arch = "wasm32")))]
#[tokio::test]
async fn missing_native_realm_membership_owner_never_falls_back_to_core() {
    let directory = tempfile::tempdir().unwrap();
    let listener = UnixListener::bind(directory.path().join("control.sock")).unwrap();
    listener.set_nonblocking(true).unwrap();
    assert!(Source::Native(None).prepare("fixture").await.is_err());
    assert_eq!(
        listener.accept().unwrap_err().kind(),
        std::io::ErrorKind::WouldBlock
    );
}

/// The actual worker rejects an already retired queued intent before dispatching its RPC.
#[tokio::test]
async fn realm_membership_queue_keeps_the_requesting_account_generation() {
    let directory = tempfile::tempdir().unwrap();
    let listener = UnixListener::bind(directory.path().join("control.sock")).unwrap();
    listener.set_nonblocking(true).unwrap();
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
    worker::run(
        Some(directory.path().to_owned()),
        Arc::clone(&shared),
        receiver,
        changed,
    )
    .await;
    assert!(shared.lock().unwrap().realm_membership.is_none());
    assert_eq!(
        listener.accept().unwrap_err().kind(),
        std::io::ErrorKind::WouldBlock
    );
}
