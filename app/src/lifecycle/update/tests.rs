//! Fixture-only regressions for the update policy and cached menu state.

use super::*;

/// Makes an advisory stage without creating an installer or using the network.
fn ready() -> Ready {
    Ready {
        current: env!("CARGO_PKG_VERSION").into(),
        latest: "9.0.0".into(),
        stage: PathBuf::from("/unused/stage"),
        notes_url: "https://example.test/notes".into(),
    }
}

#[test]
fn restart_is_impossible_mid_session_or_before_verified_readiness() {
    let mut state = State {
        enabled: true,
        ..Default::default()
    };
    for status in [
        Status::Idle,
        Status::Checking,
        Status::Downloading {
            downloaded: 100,
            total: 100,
        },
        Status::Error("bad hash".into()),
    ] {
        state.status = status;
        assert!(!state.request_restart(true));
    }
    state.status = Status::Ready(ready());
    assert!(
        !state.request_restart(false),
        "a live or connecting session must not restart"
    );
    assert!(state.request_restart(true));
    state.enabled = false;
    assert!(!state.request_restart(true));
}

#[test]
fn opt_out_invalidates_workers_and_clears_apply_intent() {
    let mut state = State {
        enabled: true,
        running: true,
        generation: 3,
        status: Status::Ready(ready()),
        restart: true,
        ..Default::default()
    };
    state.disable();
    assert!(!state.enabled && !state.running && !state.restart);
    assert_eq!(state.generation, 4);
    assert!(!state.view().ready);
}

#[test]
fn progress_and_errors_never_offer_a_restart() {
    let mut state = State {
        enabled: true,
        status: Status::Downloading {
            downloaded: 12,
            total: 24,
        },
        ..Default::default()
    };
    assert!(state.view().message.contains("50%"));
    assert!(!state.view().ready && !state.view().retry);
    state.status = Status::Error("checksum mismatch".into());
    assert!(state.view().retry && !state.view().ready);
    state.status = Status::Ready(ready());
    assert!(state.view().notes && state.view().ready);
}

#[test]
fn release_notes_reject_shell_and_non_https_destinations() {
    for raw in [
        "javascript:alert(1)",
        "file:///tmp/notes",
        "http://example.test",
        "https://user:pass@example.test",
    ] {
        assert!(notes_url(raw).is_none());
    }
    assert!(notes_url("https://example.test/releases?v=1&channel=stable").is_some());
}

#[test]
fn saved_preferences_and_ready_state_stay_in_the_user_directory() {
    let layout = crate::install_layout::scratch("updater-state");
    assert!(storage::enabled(&layout));
    storage::save_enabled(&layout, false).unwrap();
    assert!(!storage::enabled(&layout));
    storage::save_enabled(&layout, true).unwrap();
    assert!(storage::enabled(&layout));
    let mut ready = ready();
    ready.stage = storage::directory(&layout).join("update-fixture");
    std::fs::create_dir_all(&ready.stage).unwrap();
    assert!(storage::is_stage(&layout, &ready.stage));
    assert!(!storage::is_stage(&layout, &ready.stage.join("..")));
    assert!(!storage::is_stage(&layout, &layout.user_data_root));
    storage::record(&layout, Some(&ready)).unwrap();
    assert!(matches!(storage::restore(&layout), Status::Ready(_)));
    std::fs::write(ready.stage.join("apply-error.txt"), "permission denied").unwrap();
    assert!(matches!(storage::restore(&layout), Status::Error(_)));
    std::fs::remove_file(ready.stage.join("apply-error.txt")).unwrap();
    ready.current = "old-build".into();
    storage::record(&layout, Some(&ready)).unwrap();
    assert!(matches!(storage::restore(&layout), Status::Idle));
    std::fs::remove_dir_all(&layout.user_data_root).unwrap();
}

#[test]
fn platform_names_match_packaging() {
    assert_eq!(
        platform_key("macos", "aarch64").as_deref(),
        Some("macos-arm64")
    );
    assert_eq!(
        platform_key("windows", "x86_64").as_deref(),
        Some("windows-x86_64")
    );
    assert_eq!(
        platform_key("linux", "x86_64").as_deref(),
        Some("linux-x86_64")
    );
    assert!(platform_key("linux", "riscv64").is_none());
}
