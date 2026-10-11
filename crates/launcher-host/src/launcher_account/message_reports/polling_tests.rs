//! Offline control-endpoint witness: a withheld message reply cannot stall join polling.

use super::super::{AccountControl, JoinStage, LauncherAccount, MessageEvent};
use std::{
    io::{Read, Write},
    os::unix::net::{UnixListener, UnixStream},
    sync::{
        Arc,
        atomic::{AtomicBool, Ordering},
    },
    thread,
    time::{Duration, Instant},
};

/// Replies to a framed launcher request, holding message reports until the fixture ends.
fn respond(mut stream: UnixStream, held: &mut Vec<UnixStream>) {
    // Accepted sockets inherit the listener's non-blocking mode on macOS.
    stream.set_nonblocking(false).unwrap();
    stream
        .set_read_timeout(Some(Duration::from_secs(5)))
        .unwrap();
    stream
        .set_write_timeout(Some(Duration::from_secs(5)))
        .unwrap();
    let mut size = [0; 4];
    stream.read_exact(&mut size).unwrap();
    let mut request = vec![0; u32::from_be_bytes(size) as usize];
    stream.read_exact(&mut request).unwrap();
    let request: serde_json::Value = serde_json::from_slice(&request).unwrap();
    if request["method"] == "message_event.v1" {
        held.push(stream);
        return;
    }
    let reply = serde_json::json!({
        "jsonrpc": "2.0", "id": request["id"],
        "result": {"schema_version": 1, "auth": {"state":"signed_in"},
            "connect": {"stage": if held.is_empty() {"connecting"} else {"realm"}},
            "realms": [], "friends": []}
    });
    let reply = serde_json::to_vec(&reply).unwrap();
    stream
        .write_all(&(reply.len() as u32).to_be_bytes())
        .unwrap();
    stream.write_all(&reply).unwrap();
}

#[test]
fn slow_inbox_report_does_not_block_join_polling() {
    let dir = std::env::temp_dir().join(format!("inbox-poll-{}", std::process::id()));
    std::fs::create_dir_all(&dir).unwrap();
    let endpoint = dir.join("control.sock");
    let listener = UnixListener::bind(&endpoint).unwrap();
    listener.set_nonblocking(true).unwrap();
    let done = Arc::new(AtomicBool::new(false));
    let stopping = Arc::clone(&done);
    let fixture = thread::spawn(move || {
        let mut held = Vec::new();
        while !stopping.load(Ordering::Relaxed) {
            match listener.accept() {
                Ok((stream, _)) => respond(stream, &mut held),
                Err(error) if error.kind() == std::io::ErrorKind::WouldBlock => {
                    thread::sleep(Duration::from_millis(5));
                }
                Err(error) => panic!("offline control fixture: {error}"),
            }
        }
    });
    let mut account = LauncherAccount::new(dir.clone(), dir.join("artwork"));
    account.set_joining(true);
    account.report_message(MessageEvent {
        event_type: "Delete".into(),
        ..Default::default()
    });
    let deadline = Instant::now() + Duration::from_secs(5);
    let mut stage = None;
    while Instant::now() < deadline {
        stage = account.join_stage();
        if stage == Some(JoinStage::Realm) {
            break;
        }
        thread::sleep(Duration::from_millis(10));
    }
    // Drop the link before releasing held replies, so no pending report retries during cleanup.
    drop(account);
    done.store(true, Ordering::Relaxed);
    fixture.join().unwrap();
    std::fs::remove_dir_all(dir).unwrap();
    assert_eq!(
        stage,
        Some(JoinStage::Realm),
        "join polling stalled behind the held message report"
    );
}
