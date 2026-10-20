//! Local control sockets reproduce missing and stalled Profile feeds without an account.

use super::*;
use std::{
    io::{Read, Write},
    os::unix::net::{UnixListener, UnixStream},
    sync::atomic::{AtomicBool, Ordering},
};

/// Answers Profile and auth, withholding Home so the old serial worker never reaches Profile.
fn respond(mut stream: UnixStream, held: &mut Vec<UnixStream>, fail: bool) {
    stream.set_nonblocking(false).unwrap();
    stream
        .set_read_timeout(Some(Duration::from_secs(2)))
        .unwrap();
    let mut size = [0; 4];
    stream.read_exact(&mut size).unwrap();
    let mut bytes = vec![0; u32::from_be_bytes(size) as usize];
    stream.read_exact(&mut bytes).unwrap();
    let request: serde_json::Value = serde_json::from_slice(&bytes).unwrap();
    if request["method"] == "home.v1" {
        held.push(stream);
        return;
    }
    let reply = if fail && request["method"] == "profile.v1" {
        serde_json::json!({"jsonrpc":"2.0", "id":request["id"],
            "error":{"code":-32000,"message":"Service unavailable"}})
    } else {
        serde_json::json!({"jsonrpc":"2.0", "id":request["id"], "result":{
            "schema_version":1, "auth":{"state":"signed_in"},
            "profile":{"gamertag":"Fixture"}, "realms":[], "friends":[]}})
    };
    let bytes = serde_json::to_vec(&reply).unwrap();
    let _ = stream.write_all(&(bytes.len() as u32).to_be_bytes());
    let _ = stream.write_all(&bytes);
}

/// Runs the production account worker against a local endpoint with a permanently pending Home.
fn profile_with_stalled_home(fail: bool) -> Option<MenuProfile> {
    let dir = std::env::temp_dir().join(format!("profile-poll-{}-{fail}", std::process::id()));
    std::fs::create_dir_all(&dir).unwrap();
    let listener = UnixListener::bind(dir.join("control.sock")).unwrap();
    listener.set_nonblocking(true).unwrap();
    let done = Arc::new(AtomicBool::new(false));
    let stopping = Arc::clone(&done);
    let fixture = thread::spawn(move || {
        let mut held = Vec::new();
        while !stopping.load(Ordering::Relaxed) {
            match listener.accept() {
                Ok((stream, _)) => respond(stream, &mut held, fail),
                Err(error) if error.kind() == std::io::ErrorKind::WouldBlock => test_time::idle(),
                Err(error) => panic!("offline Profile fixture: {error}"),
            }
        }
    });
    let runtime = worker_runtime();
    let tasks = TaskTracker::new();
    let mut account = LauncherAccount::new(
        dir.clone(),
        dir.join("artwork"),
        runtime.handle().clone(),
        tasks.clone(),
    );
    account.refresh_profile();
    let mut profile = None;
    test_time::wait_until(Duration::from_secs(3), || {
        profile = account.profile();
        profile.is_some()
    });
    drop(account);
    done.store(true, Ordering::Relaxed);
    fixture.join().unwrap();
    finish_workers(runtime, tasks);
    std::fs::remove_dir_all(dir).unwrap();
    profile
}

#[test]
fn profile_loading_ends_while_home_is_stalled() {
    let profile = profile_with_stalled_home(false).expect("Profile never started behind Home");
    assert!(profile.loaded && !profile.unavailable);
    assert_eq!(profile.gamertag, "Fixture");
}

#[test]
fn profile_loading_ends_on_control_failure_while_home_is_stalled() {
    let profile = profile_with_stalled_home(true).expect("Profile failure was never delivered");
    assert!(profile.loaded && profile.unavailable);
    assert!(profile.avatar_loaded && profile.featured_screenshot_loaded);
    assert!(profile.statistics_loaded && profile.achievements_loaded);
}

#[test]
fn profile_loading_restarts_for_new_account_and_rejects_old_reply() {
    let (wake, requests) = profile_worker::channel();
    let shared = Mutex::new(Snapshot {
        profile_wake: Some(wake),
        profile: Some(Ok(Profile::default())),
        ..Default::default()
    });
    let old_generation = auth_generation(&shared);
    publish(&shared, |snapshot| {
        snapshot.set_account(Account {
            state: CoreAuth::SignedIn,
            gamertag: Some("New fixture".into()),
            verification_uri: None,
            user_code: None,
            reason: None,
        })
    });
    assert!(
        requests.has_changed().unwrap(),
        "new account must wake Profile"
    );
    publish_account(&shared, old_generation, |snapshot| {
        snapshot.profile = Some(Ok(Profile::default()));
    });
    assert!(
        shared.lock().unwrap().profile.is_none(),
        "retired account reply leaked"
    );
}

#[test]
fn profile_loading_ends_when_endpoint_is_missing() {
    let dir = std::env::temp_dir().join(format!("profile-missing-{}", std::process::id()));
    let runtime = worker_runtime();
    let tasks = TaskTracker::new();
    let mut account = LauncherAccount::new(
        dir.clone(),
        dir.join("artwork"),
        runtime.handle().clone(),
        tasks.clone(),
    );
    let mut profile = None;
    test_time::wait_until(Duration::from_secs(2), || {
        profile = account.profile();
        profile.is_some()
    });
    assert!(profile.is_some_and(|profile| profile.loaded && profile.unavailable));
    drop(account);
    finish_workers(runtime, tasks);
}

/// Retiring an accepted old request must release the worker for the replacement identity.
#[test]
fn retired_account_cancels_pending_profile_before_loading_replacement() {
    let directory = tempfile::tempdir().unwrap();
    let listener = UnixListener::bind(directory.path().join("control.sock")).unwrap();
    listener.set_nonblocking(true).unwrap();
    let admitted = Arc::new(AtomicBool::new(false));
    let released = Arc::new(AtomicBool::new(false));
    let replacement = Arc::new(AtomicBool::new(false));
    let done = Arc::new(AtomicBool::new(false));
    let (accepted, closed, changed, stopping) = (
        Arc::clone(&admitted),
        Arc::clone(&released),
        Arc::clone(&replacement),
        Arc::clone(&done),
    );
    let fixture = thread::spawn(move || {
        let mut old = None;
        while !stopping.load(Ordering::Acquire) {
            if let Some(stream) = &mut old {
                let mut byte = [0];
                match std::io::Read::read(stream, &mut byte) {
                    Ok(0) => {
                        closed.store(true, Ordering::Release);
                        old = None;
                    }
                    Err(error) if error.kind() == std::io::ErrorKind::WouldBlock => {}
                    result => panic!("retired profile socket: {result:?}"),
                }
            }
            match listener.accept() {
                Ok((mut stream, _)) => {
                    stream.set_nonblocking(false).unwrap();
                    stream
                        .set_read_timeout(Some(Duration::from_secs(2)))
                        .unwrap();
                    let mut size = [0; 4];
                    stream.read_exact(&mut size).unwrap();
                    let mut bytes = vec![0; u32::from_be_bytes(size) as usize];
                    stream.read_exact(&mut bytes).unwrap();
                    let request: serde_json::Value = serde_json::from_slice(&bytes).unwrap();
                    if request["method"] == "profile.v1" && !accepted.swap(true, Ordering::AcqRel) {
                        stream.set_nonblocking(true).unwrap();
                        old = Some(stream);
                        continue;
                    }
                    let name = if changed.load(Ordering::Acquire) {
                        "Replacement"
                    } else {
                        "Original"
                    };
                    let reply = serde_json::json!({"jsonrpc":"2.0", "id":request["id"], "result":{
                        "schema_version":1, "auth":{"state":"signed_in", "gamertag":name},
                        "profile":{"gamertag":name, "xuid":"456"}, "realms":[], "friends":[]}});
                    let bytes = serde_json::to_vec(&reply).unwrap();
                    let _ = stream.write_all(&(bytes.len() as u32).to_be_bytes());
                    let _ = stream.write_all(&bytes);
                }
                Err(error) if error.kind() == std::io::ErrorKind::WouldBlock => test_time::idle(),
                Err(error) => panic!("profile retirement fixture: {error}"),
            }
        }
    });
    let runtime = worker_runtime();
    let tasks = TaskTracker::new();
    let mut account = LauncherAccount::new(
        directory.path().into(),
        directory.path().join("art"),
        runtime.handle().clone(),
        tasks.clone(),
    );
    // Always close the fixture on failure; an old sixty-second RPC must not escape this test.
    let outcome = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
        test_time::eventually_within(
            Duration::from_secs(2),
            "first profile RPC admission",
            || admitted.load(Ordering::Acquire),
        );
        replacement.store(true, Ordering::Release);
        account.with(|snapshot| {
            snapshot.set_account(Account {
                state: CoreAuth::SignedIn,
                gamertag: Some("Replacement".into()),
                verification_uri: None,
                user_code: None,
                reason: None,
            })
        });
        let mut fresh = false;
        test_time::eventually_within(
            Duration::from_secs(2),
            "retired profile socket EOF and replacement profile",
            || {
                fresh |= account.profile().is_some_and(|profile| {
                    profile.loaded
                        && !profile.unavailable
                        && profile.gamertag == "Replacement"
                        && profile.xuid == "456"
                });
                fresh && released.load(Ordering::Acquire)
            },
        );
    }));
    drop(account);
    done.store(true, Ordering::Release);
    fixture.join().unwrap();
    finish_workers(runtime, tasks);
    assert!(
        outcome.is_ok(),
        "retired profile RPC blocked the new account's presentation"
    );
}
