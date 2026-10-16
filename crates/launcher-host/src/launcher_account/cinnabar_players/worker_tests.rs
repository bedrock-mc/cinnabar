//! Exercises roster delivery and publication through the real local control transport.

use super::*;
use std::{
    io::{Read, Write},
    os::unix::net::UnixListener,
};

/// A successful control answer reaches the shared badge set without another roster update.
#[test]
fn a_lookup_answer_publishes_without_roster_churn() {
    let dir = std::env::temp_dir().join(format!("badge-worker-{}", std::process::id()));
    std::fs::create_dir_all(&dir).unwrap();
    let listener = UnixListener::bind(dir.join("control.sock")).unwrap();
    listener.set_nonblocking(true).unwrap();
    let fixture = thread::spawn(move || {
        let mut accepted = None;
        test_time::eventually("the badge lookup control request", || {
            match listener.accept() {
                Ok((stream, _)) => {
                    accepted = Some(stream);
                    true
                }
                Err(error) if error.kind() == std::io::ErrorKind::WouldBlock => false,
                Err(error) => panic!("badge control fixture: {error}"),
            }
        });
        let mut stream = accepted.unwrap();
        stream.set_nonblocking(false).unwrap();
        stream
            .set_read_timeout(Some(Duration::from_secs(3)))
            .unwrap();
        let mut size = [0; 4];
        stream.read_exact(&mut size).unwrap();
        let mut bytes = vec![0; u32::from_be_bytes(size) as usize];
        stream.read_exact(&mut bytes).unwrap();
        let request: serde_json::Value = serde_json::from_slice(&bytes).unwrap();
        assert_eq!(request["method"], bridge::contract::METHOD_CINNABAR_LOOKUP);
        assert_eq!(request["params"]["xuids"], serde_json::json!(["11"]));
        let reply = serde_json::to_vec(&serde_json::json!({
            "jsonrpc":"2.0", "id":request["id"], "result": {
                "schema_version":1, "resolved":["11"], "cinnabar":["11"],
                "cache_ttl_ms": CINNABAR_API_CACHE_TTL_MS
            }
        }))
        .unwrap();
        stream
            .write_all(&(reply.len() as u32).to_be_bytes())
            .unwrap();
        stream.write_all(&reply).unwrap();
    });
    let shared = Arc::new(Mutex::new(Snapshot {
        cinnabar_roster_key: Some((1, 1)),
        ..Default::default()
    }));
    let (sender, requests) = unbounded();
    let (worker_dir, worker_shared) = (dir.clone(), Arc::clone(&shared));
    let worker = thread::spawn(move || run(&worker_dir, &worker_shared, &requests));
    sender
        .send(Roster {
            session: 1,
            identities: vec!["11".into()],
        })
        .unwrap();
    let published = test_time::wait_until(Duration::from_secs(3), || {
        shared.lock().unwrap().cinnabar_players.contains("11")
    });
    drop(sender);
    worker.join().unwrap();
    fixture.join().unwrap();
    std::fs::remove_dir_all(dir).unwrap();
    assert!(published, "lookup answer never reached the badge set");
}
