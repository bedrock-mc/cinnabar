//! Local control sockets reproduce missing and stalled Profile feeds without an account.

use std::{
    io::{Read, Write},
    os::unix::net::{UnixListener, UnixStream},
    sync::atomic::{AtomicBool, Ordering},
};
use {super::*, launcher::menu::auth::AuthState};

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
                Err(error) if error.kind() == std::io::ErrorKind::WouldBlock => {
                    thread::sleep(Duration::from_millis(5));
                }
                Err(error) => panic!("offline Profile fixture: {error}"),
            }
        }
    });
    let mut account = LauncherAccount::new(dir.clone());
    account.refresh_profile();
    let deadline = Instant::now() + Duration::from_secs(3);
    let mut profile = None;
    while Instant::now() < deadline {
        profile = account.profile();
        if profile.is_some() {
            break;
        }
        thread::sleep(Duration::from_millis(10));
    }
    drop(account);
    done.store(true, Ordering::Relaxed);
    fixture.join().unwrap();
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
fn profile_loading_ends_without_a_feed_worker_in_both_startup_modes() {
    for launcher in [false, true] {
        let mut menu = super::super::MenuRuntime::new_with_layout(
            launcher,
            Some(2),
            "Fixture".into(),
            crate::install_layout::scratch("profile-no-worker"),
            crate::player_skin::LocalPlayerSkin::generated_default("Fixture"),
        );
        menu.control_auth = Some(AuthState::Authenticated);
        menu.catalog_started = true;
        menu.enter(launcher::menu::MenuScreen::Profile);
        assert!(
            menu.feeds.profile_refresh_requested,
            "opening must request Profile"
        );
        menu.poll_catalog(false);
        assert!(
            menu.view().feeds.profile.unavailable,
            "no Profile worker must show unavailable, launcher={launcher}"
        );
        menu.activate(launcher::menu::MenuAction::RefreshProfile);
        menu.poll_catalog(false);
        assert!(
            menu.view().feeds.profile.unavailable,
            "retry without a worker hung"
        );
    }
}

#[test]
fn profile_loading_restarts_for_new_account_and_rejects_old_reply() {
    let (wake, requests) = bounded(1);
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
    assert!(requests.try_recv().is_ok(), "new account must wake Profile");
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
    let mut account = LauncherAccount::new(dir);
    let deadline = Instant::now() + Duration::from_secs(2);
    let mut profile = None;
    while Instant::now() < deadline {
        profile = account.profile();
        if profile.is_some() {
            break;
        }
        thread::sleep(Duration::from_millis(5));
    }
    assert!(profile.is_some_and(|profile| profile.loaded && profile.unavailable));
}

#[test]
fn profile_account_feed_does_not_prevent_cached_validation_in_direct_mode() {
    let layout = crate::install_layout::scratch("profile-cached-validation");
    let auth_path = layout.auth_cache();
    std::fs::create_dir_all(auth_path.parent().unwrap()).unwrap();
    // Presence alone triggers validation. No credentials or real core executable exist here.
    std::fs::write(&auth_path, []).unwrap();
    let mut menu = super::super::MenuRuntime::new_with_layout(
        false,
        Some(2),
        "Fixture".into(),
        layout,
        crate::player_skin::LocalPlayerSkin::generated_default("Fixture"),
    );
    menu.enter(launcher::menu::MenuScreen::Profile);
    menu.poll_catalog(true);
    assert!(
        menu.auth_attempted,
        "an attached signed-out core suppressed cached validation"
    );
    assert!(
        menu.auth_process.is_none(),
        "fixture must not start a real authentication helper"
    );
    std::fs::remove_file(auth_path).unwrap();
}
