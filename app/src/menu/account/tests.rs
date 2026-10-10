use std::{
    ffi::OsString,
    fs,
    io::Write,
    path::{Path, PathBuf},
    process::{Command, Stdio},
    thread,
    time::{Duration, Instant, SystemTime, UNIX_EPOCH},
};

use launcher::install_layout::{InstallEnvironment, Platform};
use launcher_host::core_process::core_command_for_address;
use {
    super::*,
    launcher::install_layout::InstallLayout,
    launcher::menu::auth::AuthState,
    launcher::menu::{MenuAction, MenuDialog},
};

#[test]
fn new_core_prompts_take_primary_focus_and_repeats_keep_cancel_selected() {
    let mut menu = MenuRuntime::new(true, 2, "Offline Player".into());
    let code = |value: &str| AuthState::AwaitingCode {
        uri: "https://example.invalid".into(),
        code: value.into(),
    };
    menu.focused = 7;
    menu.apply_control_auth(code("FIRST"));
    assert_eq!(menu.view().focused_action, Some(MenuAction::OpenSignInLink));
    menu.move_focus(1);
    menu.apply_control_auth(code("FIRST"));
    assert_eq!(menu.view().focused_action, Some(MenuAction::CancelSignIn));
    menu.apply_control_auth(code("SECOND"));
    assert_eq!(menu.view().focused_action, Some(MenuAction::OpenSignInLink));
    menu.move_focus(1);
    menu.apply_control_auth(AuthState::Failed("Try again.".into()));
    assert_eq!(menu.view().focused_action, Some(MenuAction::StartSignIn));
}

#[test]
fn completed_add_account_control_preserves_cancel_focus() {
    let mut menu = MenuRuntime::new(true, 2, "Offline Player".into());
    menu.dialog = Some(MenuDialog::Accounts);
    menu.feeds.account_adding = true;
    menu.apply_control_auth(AuthState::AwaitingCode {
        uri: "https://example.invalid".into(),
        code: "TEST-CODE".into(),
    });
    menu.move_focus(1);
    assert_eq!(menu.focused, 1);
    assert_eq!(menu.view().focused_action, Some(MenuAction::CancelSignIn));
    menu.apply_control_auth(AuthState::Authenticated);
    assert_eq!(menu.view().focused_action, Some(MenuAction::CancelSignIn));
}

#[test]
fn completed_add_account_helper_preserves_cancel_focus() {
    let (mut child, directory) = event_child_waiting(
        &[
            r#"{"v":1,"event":"checking_cache"}"#,
            r#"{"v":1,"event":"device_code","verification_uri":"https://example.invalid","user_code":"TEST-CODE"}"#,
        ],
        &[r#"{"v":1,"event":"authenticated","method":"device_code"}"#],
    );
    let mut input = child.stdin.take().expect("injected helper input");
    let mut menu = MenuRuntime::new(true, 2, "Offline Player".into());
    menu.dialog = Some(MenuDialog::Accounts);
    menu.feeds.account_adding = true;
    menu.auth_process = Some(AuthSupervisor::from_child(child).unwrap());
    let deadline = Instant::now() + Duration::from_secs(5);
    while !matches!(
        menu.current_auth().as_ref(),
        AuthState::AwaitingCode { .. } | AuthState::AwaitingXboxSignup { .. }
    ) && Instant::now() < deadline
    {
        menu.poll_sign_in();
        thread::sleep(Duration::from_millis(5));
    }
    assert!(matches!(
        menu.current_auth().as_ref(),
        AuthState::AwaitingCode { .. } | AuthState::AwaitingXboxSignup { .. }
    ));
    menu.move_focus(1);
    assert_eq!(menu.focused, 1);
    assert_eq!(menu.view().focused_action, Some(MenuAction::CancelSignIn));
    input.write_all(b"finish\n").unwrap();
    input.flush().unwrap();
    let deadline = Instant::now() + Duration::from_secs(5);
    while !matches!(menu.current_auth().as_ref(), AuthState::Authenticated)
        && Instant::now() < deadline
    {
        menu.poll_sign_in();
        thread::sleep(Duration::from_millis(5));
    }
    assert_eq!(menu.current_auth().as_ref(), &AuthState::Authenticated);
    menu.poll_accounts();
    let focused = menu.view().focused_action;
    assert!(menu.accounts.pending_ready);
    drop(input);
    drop(menu);
    fs::remove_dir_all(directory).unwrap();
    assert_eq!(focused, Some(MenuAction::CancelSignIn));
}

#[test]
fn failed_helper_start_survives_signed_out_core_status() {
    assert_failed_start_prompt_survives(AuthState::SignedOut);
}

#[test]
fn failed_helper_start_survives_authenticated_core_status() {
    assert_failed_start_prompt_survives(AuthState::Authenticated);
}

/// Core status cannot dismiss a launcher-start failure or restore an unfinished Add account.
fn assert_failed_start_prompt_survives(status: AuthState) {
    for adding in [false, true] {
        let mut menu = MenuRuntime::new(true, 2, "Offline Player".into());
        menu.layout.core_executable = menu.layout.user_data_root.join("missing-helper");
        menu.feeds.account_adding = adding;
        menu.dialog = adding.then_some(MenuDialog::Accounts);
        menu.start_sign_in();
        let failure = menu.current_auth().into_owned();
        assert!(matches!(failure, AuthState::Failed(_)));
        menu.apply_control_auth(status.clone());
        menu.poll_accounts();
        assert_eq!(menu.current_auth().as_ref(), &failure);
        assert!(menu.sign_in_requested);
        assert_eq!(menu.view().focused_action, Some(MenuAction::StartSignIn));
        assert!(menu.accounts.operation.is_none());
        assert_eq!(menu.control_auth.as_ref(), Some(&status));
        let hidden_prompt = AuthState::AwaitingCode {
            uri: "https://example.invalid".into(),
            code: "HIDDEN-CODE".into(),
        };
        menu.apply_control_auth(hidden_prompt.clone());
        menu.update_sign_in_browser(false);
        assert_eq!(
            menu.sign_in_browser.state(&hidden_prompt),
            launcher::menu::sign_in::BrowserState::Waiting
        );
        menu.apply_control_auth(status.clone());
        menu.stop_sign_in();
        assert!(!menu.sign_in_requested);
        assert_eq!(menu.current_auth().as_ref(), &status);
        menu.start_sign_in();
        assert!(matches!(menu.current_auth().as_ref(), AuthState::Failed(_)));
    }
}

#[test]
fn cancelled_core_prompts_do_not_return_on_repeated_status() {
    let mut menu = MenuRuntime::new(true, 2, "Offline Player".into());
    let code = AuthState::AwaitingCode {
        uri: "https://example.invalid".into(),
        code: "TEST-CODE".into(),
    };
    menu.apply_control_auth(code.clone());
    menu.activate(MenuAction::CancelSignIn);
    menu.focused = 1;
    for state in [code, AuthState::Failed("Try again.".into())] {
        menu.apply_control_auth(state);
        menu.update_sign_in_browser(false);
        assert_eq!(menu.focused, 1);
        assert_eq!(menu.view().auth_state, AuthState::SignedOut);
        assert!(!menu.view().popup_open());
    }
    menu.apply_control_auth(AuthState::Authenticated);
    assert_eq!(menu.view().auth_state, AuthState::Authenticated);
    menu.start_sign_in();
    assert!(!menu.sign_in_cancelled);
    assert!(menu.sign_in_requested);
}

#[test]
fn cached_validation_keeps_home_focus_until_a_device_code_arrives() {
    let mut menu = MenuRuntime::new(true, 2, "Offline Player".into());
    menu.focused = 1;
    for state in [AuthState::Checking, AuthState::Failed("Try again.".into())] {
        menu.apply_control_auth(state);
        assert_eq!(menu.focused, 1);
        assert!(!menu.view().sign_in_prompt_open());
        assert!(menu.sign_in_focus().is_none());
    }
    menu.apply_control_auth(AuthState::AwaitingCode {
        uri: "https://example.invalid".into(),
        code: "TEST-CODE".into(),
    });
    assert!(menu.view().sign_in_prompt_open());
    assert_eq!(menu.view().focused_action, Some(MenuAction::OpenSignInLink));
}

#[test]
fn catalog_failures_have_safe_recovery_copy_and_preserve_partial_success() {
    let mut menu = MenuRuntime::new(true, 2, "Player".to_owned());
    let catalog: CatalogFile = serde_json::from_str(
            r#"{"featured":[{"name":"Available server","address":"example.test:19132","caption":"Live","image_path":""}],"errors":["Realms: POST https://example.test/?token=synthetic-secret: 503 response-body-sentinel"]}"#,
        ).unwrap();
    menu.apply_catalog(catalog);
    assert_eq!(menu.featured.len(), 1);
    assert_eq!(menu.featured[0].name, "Available server");
    assert_eq!(
        menu.catalog_message.as_deref(),
        Some("Social: Refresh to try again.")
    );
    menu.apply_catalog(CatalogFile::default());
    assert!(menu.catalog_message.is_none());
}

fn launch_layout_with_spaces() -> InstallLayout {
    InstallLayout::resolve(
        Platform::Linux,
        &InstallEnvironment {
            executable: PathBuf::from("/opt/Cinnabar Client/bin/bedrock-client"),
            user_root: None,
            home: Some(PathBuf::from("/home/Player One")),
            local_app_data: None,
            xdg_config_home: Some(PathBuf::from("/cfg/Player One")),
            xdg_data_home: Some(PathBuf::from("/data/Player One")),
            xdg_runtime_dir: Some(PathBuf::from("/run/user/1000")),
        },
    )
    .unwrap()
}

#[test]
fn cached_validation_runs_once_but_cancel_is_sticky() {
    let mut menu = MenuRuntime::new(true, 2, "Offline Player".to_owned());
    assert!(menu.should_auto_start_sign_in(true));
    menu.stop_sign_in();
    assert!(!menu.should_auto_start_sign_in(true));
    assert!(!menu.should_auto_start_sign_in(false));
}

#[test]
fn failed_spawn_is_not_retried_by_frame_polling() {
    let mut menu = MenuRuntime::new(true, 2, "Offline Player".to_owned());
    menu.auth_attempted = true;
    menu.auth_process = None;
    menu.message = Some("Could not start sign-in helper.".to_owned());

    for _ in 0..100 {
        assert!(!menu.should_auto_start_sign_in(true));
        assert!(menu.auth_attempted && menu.auth_process.is_none());
    }
    assert_eq!(
        menu.message.as_deref(),
        Some("Could not start sign-in helper.")
    );
}

#[test]
fn active_cancel_is_sticky_and_does_not_block_the_menu_frame() {
    let mut command = if cfg!(windows) {
        let mut command = Command::new("cmd");
        command.args(["/C", "ping -n 30 127.0.0.1 >NUL"]);
        command
    } else {
        let mut command = Command::new("sh");
        command.args(["-c", "exec sleep 30"]);
        command
    };
    let child = command.stdout(Stdio::piped()).spawn().unwrap();
    let mut menu = MenuRuntime::new(true, 2, "Offline Player".to_owned());
    menu.auth_process = Some(AuthSupervisor::from_child(child).unwrap());

    let started = Instant::now();
    menu.stop_sign_in();
    assert!(started.elapsed() < Duration::from_millis(100));
    assert!(!menu.auth_restart_requested);
    assert!(!menu.should_auto_start_sign_in(true));
    assert!(matches!(
        menu.auth_process.as_ref().map(AuthSupervisor::state),
        Some(AuthState::SignedOut)
    ));

    for _ in 0..8 {
        assert!(!menu.auth_restart_requested);
        assert!(!menu.should_auto_start_sign_in(true));
    }

    menu.start_sign_in();
    assert!(menu.auth_restart_requested);
    assert!(menu.auth_attempted);
}

#[test]
fn only_validated_authentication_selects_the_cache_for_a_connection() {
    let layout = launcher::test_support::checkout();
    assert_eq!(validated_auth_cache(&layout, None), None);
    assert_eq!(
        validated_auth_cache(&layout, Some(&AuthState::SignedOut)),
        None
    );
    assert_eq!(
        validated_auth_cache(&layout, Some(&AuthState::Checking)),
        None
    );
    let failed = AuthState::Failed("validation failed".to_owned());
    assert_eq!(validated_auth_cache(&layout, Some(&failed)), None);
    assert_eq!(
        validated_auth_cache(&layout, Some(&AuthState::Authenticated)),
        Some(layout.auth_cache())
    );
}

#[test]
fn core_child_args_are_offline_unless_authentication_was_validated() {
    let layout = launch_layout_with_spaces();
    let offline = core_command_for_address(
        &layout,
        Path::new("bedrock-core"),
        Path::new("run with spaces"),
        "example.test:19132",
        None,
        false,
    );
    let offline_args = offline.get_args().map(OsString::from).collect::<Vec<_>>();
    assert_eq!(
        offline_args,
        [
            OsString::from("-control-status"),
            OsString::from("-socket-dir"),
            OsString::from("run with spaces"),
            OsString::from("-upstream"),
            OsString::from("example.test:19132"),
            OsString::from("-resource-pack-cache-dir"),
            layout.resource_pack_cache_dir().into_os_string(),
        ]
    );
    assert_eq!(
        offline_args
            .iter()
            .filter(|arg| *arg == "-resource-pack-cache-dir")
            .count(),
        1
    );
    assert!(
        !offline_args
            .iter()
            .any(|arg| arg == "-resource-pack-cache-quota-bytes")
    );

    let authenticated = core_command_for_address(
        &layout,
        Path::new("bedrock-core"),
        Path::new("run with spaces"),
        "example.test:19132",
        Some(Path::new("validated token.json")),
        false,
    );
    let authenticated_args = authenticated
        .get_args()
        .map(OsString::from)
        .collect::<Vec<_>>();
    assert_eq!(
        authenticated_args,
        [
            OsString::from("-control-status"),
            OsString::from("-socket-dir"),
            OsString::from("run with spaces"),
            OsString::from("-upstream"),
            OsString::from("example.test:19132"),
            OsString::from("-resource-pack-cache-dir"),
            layout.resource_pack_cache_dir().into_os_string(),
            OsString::from("-auth-cache"),
            OsString::from("validated token.json"),
        ]
    );
}

// The upstream client-cache advertisement is opt-in per spawn: the exact
// `-upstream-client-cache` argument appears only when the caller proves
// blob-cache ownership, and it rides after the pack cache directory so a
// default spawn keeps today's byte-exact core arguments.
#[test]
fn core_child_args_advertise_upstream_client_cache_only_when_enabled() {
    let layout = launch_layout_with_spaces();
    let enabled = core_command_for_address(
        &layout,
        Path::new("bedrock-core"),
        Path::new("run with spaces"),
        "example.test:19132",
        None,
        true,
    );
    let enabled_args = enabled.get_args().map(OsString::from).collect::<Vec<_>>();
    assert_eq!(
        enabled_args,
        [
            OsString::from("-control-status"),
            OsString::from("-socket-dir"),
            OsString::from("run with spaces"),
            OsString::from("-upstream"),
            OsString::from("example.test:19132"),
            OsString::from("-resource-pack-cache-dir"),
            layout.resource_pack_cache_dir().into_os_string(),
            OsString::from("-upstream-client-cache"),
        ]
    );
}

#[test]
fn offline_connect_waits_for_cancelled_sign_in_to_reap() {
    let mut command = if cfg!(windows) {
        let mut command = Command::new("cmd");
        command.args(["/Q", "/C", "set /p hold="]);
        command
    } else {
        let mut command = Command::new("sh");
        command.args(["-c", "IFS= read -r hold"]);
        command
    };
    let child = command
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .spawn()
        .unwrap();
    let mut menu = MenuRuntime::new(true, 2, "Offline Player".to_owned());
    menu.auth_process = Some(AuthSupervisor::from_child(child).unwrap());

    menu.request_connect("offline.example:19132".to_owned());
    assert!(menu.take_join_intent().is_none());
    assert!(matches!(
        menu.auth_process.as_ref().map(AuthSupervisor::state),
        Some(AuthState::SignedOut)
    ));

    let deadline = Instant::now() + Duration::from_secs(5);
    while !menu
        .auth_process
        .as_ref()
        .is_some_and(AuthSupervisor::cleanup_complete)
        && Instant::now() < deadline
    {
        menu.poll_sign_in();
        thread::sleep(Duration::from_millis(5));
    }
    assert!(
        menu.auth_process
            .as_ref()
            .is_some_and(AuthSupervisor::cleanup_complete),
        "cancelled sign-in helper was not reaped"
    );
    let pending = menu.take_join_intent().expect("offline connection");
    assert_eq!(pending.address, "offline.example:19132");
    assert_eq!(pending.auth_cache, None);
}

#[test]
fn authenticated_connect_waits_for_sign_in_reap_and_keeps_validated_cache() {
    let (child, directory) = event_child_holding(&[
        r#"{"v":1,"event":"checking_cache"}"#,
        r#"{"v":1,"event":"authenticated","method":"cached"}"#,
    ]);
    let mut menu = MenuRuntime::new(true, 2, "Offline Player".to_owned());
    menu.auth_process = Some(AuthSupervisor::from_child(child).unwrap());

    let authenticated_deadline = Instant::now() + Duration::from_secs(5);
    while !matches!(
        menu.auth_process.as_ref().map(AuthSupervisor::state),
        Some(AuthState::Authenticated)
    ) && Instant::now() < authenticated_deadline
    {
        menu.poll_sign_in();
        thread::sleep(Duration::from_millis(5));
    }
    assert!(matches!(
        menu.auth_process.as_ref().map(AuthSupervisor::state),
        Some(AuthState::Authenticated)
    ));

    menu.request_connect("authenticated.example:19132".to_owned());
    assert!(menu.take_join_intent().is_none());
    let deadline = Instant::now() + Duration::from_secs(5);
    while !menu
        .auth_process
        .as_ref()
        .is_some_and(AuthSupervisor::cleanup_complete)
        && Instant::now() < deadline
    {
        menu.poll_sign_in();
        thread::sleep(Duration::from_millis(5));
    }
    assert!(
        menu.auth_process
            .as_ref()
            .is_some_and(AuthSupervisor::cleanup_complete),
        "authenticated sign-in helper was not reaped"
    );
    let pending = menu.take_join_intent().expect("authenticated connection");
    assert_eq!(pending.address, "authenticated.example:19132");
    assert_eq!(pending.auth_cache, Some(menu.layout.auth_cache()));
    fs::remove_dir_all(directory).unwrap();
}

#[test]
fn validation_failure_releases_the_queued_connection_offline() {
    let (child, directory) = event_child_holding(&[
        r#"{"v":1,"event":"checking_cache"}"#,
        r#"{"v":1,"event":"error","stage":"cache","message":"validation failed"}"#,
    ]);
    let mut menu = MenuRuntime::new(true, 2, "Offline Player".to_owned());
    menu.auth_process = Some(AuthSupervisor::from_child(child).unwrap());

    let failed_deadline = Instant::now() + Duration::from_secs(5);
    while !matches!(
        menu.auth_process.as_ref().map(AuthSupervisor::state),
        Some(AuthState::Failed(_))
    ) && Instant::now() < failed_deadline
    {
        menu.poll_sign_in();
        thread::sleep(Duration::from_millis(5));
    }
    assert!(matches!(
        menu.auth_process.as_ref().map(AuthSupervisor::state),
        Some(AuthState::Failed(_))
    ));

    menu.request_connect("offline-after-failure.example:19132".to_owned());
    let deadline = Instant::now() + Duration::from_secs(5);
    while !menu
        .auth_process
        .as_ref()
        .is_some_and(AuthSupervisor::cleanup_complete)
        && Instant::now() < deadline
    {
        menu.poll_sign_in();
        thread::sleep(Duration::from_millis(5));
    }
    assert!(
        menu.auth_process
            .as_ref()
            .is_some_and(AuthSupervisor::cleanup_complete),
        "failed sign-in helper was not reaped"
    );
    let pending = menu.take_join_intent().expect("offline connection");
    assert_eq!(pending.address, "offline-after-failure.example:19132");
    assert_eq!(pending.auth_cache, None);
    fs::remove_dir_all(directory).unwrap();
}

/// Emits fixed events and keeps the injected helper alive until cleanup.
fn event_child_holding(lines: &[&str]) -> (std::process::Child, PathBuf) {
    event_child_waiting(lines, &[])
}

/// Releases completion events only after the test acknowledges the initial prompt.
fn event_child_waiting(lines: &[&str], completion: &[&str]) -> (std::process::Child, PathBuf) {
    let unique = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap()
        .as_nanos();
    let directory = std::env::temp_dir().join(format!(
        "cinnabar-account-auth-helper-{}-{unique}",
        std::process::id()
    ));
    fs::create_dir_all(&directory).unwrap();
    let mut command = if cfg!(windows) {
        let script = directory.join("events.cmd");
        let body = format!(
            "@echo off\r\n{}\r\nset /p hold=\r\n{}\r\nset /p hold=\r\n",
            lines
                .iter()
                .map(|line| format!("echo {line}"))
                .collect::<Vec<_>>()
                .join("\r\n"),
            completion
                .iter()
                .map(|line| format!("echo {line}"))
                .collect::<Vec<_>>()
                .join("\r\n")
        );
        fs::write(&script, body).unwrap();
        let mut command = Command::new("cmd");
        command.args(["/Q", "/D", "/C"]).arg(script);
        command
    } else {
        let script = directory.join("events.sh");
        let body = format!(
            "#!/bin/sh\n{}\nIFS= read -r hold\n{}\nIFS= read -r hold\n",
            lines
                .iter()
                .map(|line| format!("printf '%s\\n' '{line}'"))
                .collect::<Vec<_>>()
                .join("\n"),
            completion
                .iter()
                .map(|line| format!("printf '%s\\n' '{line}'"))
                .collect::<Vec<_>>()
                .join("\n")
        );
        fs::write(&script, body).unwrap();
        let mut command = Command::new("sh");
        command.arg(script);
        command
    };
    let child = command
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .spawn()
        .unwrap();
    (child, directory)
}

#[test]
fn xbox_signup_owns_focus_and_can_be_cancelled() {
    use {crate::menu::MenuRuntime, launcher::menu::MenuAction};
    let mut menu = MenuRuntime::new(true, 2, "Fixture Player".into());
    menu.apply_control_auth(AuthState::AwaitingXboxSignup {
        uri: "https://sisu.xboxlive.com/signup?signature=fixture".into(),
    });
    assert!(menu.view().sign_in_prompt_open());
    assert_eq!(
        menu.sign_in_focus(),
        Some(vec![MenuAction::OpenSignInLink, MenuAction::CancelSignIn])
    );
    menu.activate(MenuAction::CancelSignIn);
    assert_eq!(menu.current_auth().as_ref(), &AuthState::SignedOut);
    assert!(!menu.view().sign_in_prompt_open());
}
