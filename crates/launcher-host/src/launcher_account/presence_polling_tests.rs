//! Presence reporting must leave the account event worker running.
use super::*;
use std::{
    io::{Read, Write},
    os::unix::net::UnixListener,
    sync::atomic::{AtomicBool, Ordering},
};

#[test]
fn presence_reporting_keeps_account_events_running() {
    let dir = std::env::temp_dir().join(format!("presence-poll-{}", std::process::id()));
    std::fs::create_dir_all(&dir).unwrap();
    let listener = UnixListener::bind(dir.join("control.sock")).unwrap();
    listener.set_nonblocking(true).unwrap();
    let done = Arc::new(AtomicBool::new(false));
    let stopping = Arc::clone(&done);
    let (methods, received) = bounded(2);
    let fixture = thread::spawn(move || {
        while !stopping.load(Ordering::Relaxed) {
            let mut stream = match listener.accept() {
                Ok((stream, _)) => stream,
                Err(error) if error.kind() == std::io::ErrorKind::WouldBlock => {
                    test_time::idle();
                    continue;
                }
                Err(error) => panic!("presence fixture: {error}"),
            };
            stream
                .set_read_timeout(Some(Duration::from_secs(2)))
                .unwrap();
            let mut size = [0; 4];
            stream.read_exact(&mut size).unwrap();
            let mut bytes = vec![0; u32::from_be_bytes(size) as usize];
            stream.read_exact(&mut bytes).unwrap();
            let request: serde_json::Value = serde_json::from_slice(&bytes).unwrap();
            let reply = serde_json::json!({"jsonrpc":"2.0", "id":request["id"],
                "result":{"schema_version":1, "auth":{"state":"signed_in"}}});
            let bytes = serde_json::to_vec(&reply).unwrap();
            stream
                .write_all(&(bytes.len() as u32).to_be_bytes())
                .unwrap();
            stream.write_all(&bytes).unwrap();
            methods
                .send(request["method"].as_str().unwrap().to_owned())
                .unwrap();
        }
    });
    let shared = Arc::new(Mutex::new(Snapshot {
        joining: true,
        ..Default::default()
    }));
    let (requests, receiver) = bounded(1);
    let (worker_dir, worker_shared) = (dir.clone(), Arc::clone(&shared));
    let worker = thread::spawn(move || poll_events(&worker_dir, &worker_shared, &receiver));
    let first = received.recv_timeout(Duration::from_secs(2));
    let second = received.recv_timeout(Duration::from_secs(2));
    drop(requests);
    let completed = worker.join();
    done.store(true, Ordering::Relaxed);
    fixture.join().unwrap();
    std::fs::remove_dir_all(dir).unwrap();
    assert!(
        completed.is_ok(),
        "presence reporting stopped the account worker"
    );
    assert_eq!(first.unwrap(), "presence.v1");
    assert_eq!(second.unwrap(), "events.v1");
    assert_eq!(
        shared.lock().unwrap().account.as_ref().unwrap().state,
        CoreAuth::SignedIn
    );
}
