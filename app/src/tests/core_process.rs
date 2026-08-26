use std::{
    fs,
    path::{Path, PathBuf},
    process::{Child, Command, Stdio},
    time::{Instant, SystemTime, UNIX_EPOCH},
};

use super::*;

/// Spawns a script child that blocks reading its piped stdin and exits as
/// soon as the pipe closes (stdin EOF), mirroring bedrock-core's own
/// cancellation path.
fn stdin_holding_child() -> (Child, PathBuf) {
    fixture_child(|windows_body, unix_body| {
        *windows_body = "@echo off\r\nset /p hold=\r\n".to_owned();
        *unix_body = "#!/bin/sh\nIFS= read -r hold\n".to_owned();
    })
}

/// Spawns a script child that never reads stdin and keeps running long past
/// any graceful deadline, forcing the kill fallback.
fn stdin_ignoring_child() -> (Child, PathBuf) {
    fixture_child(|windows_body, unix_body| {
        *windows_body = "@echo off\r\nping -n 30 127.0.0.1 > nul\r\n".to_owned();
        *unix_body = "#!/bin/sh\nsleep 30 </dev/null\n".to_owned();
    })
}

fn fixture_child(write_body: impl FnOnce(&mut String, &mut String)) -> (Child, PathBuf) {
    let unique = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap()
        .as_nanos();
    let directory = std::env::temp_dir().join(format!(
        "cinnabar-core-process-{}-{unique}",
        std::process::id()
    ));
    fs::create_dir_all(&directory).unwrap();
    let (script_path, interpreter_args, mut windows_body, mut unix_body) = if cfg!(windows) {
        (
            directory.join("body.cmd"),
            vec!["/Q".to_owned(), "/C".to_owned()],
            String::new(),
            String::new(),
        )
    } else {
        (
            directory.join("body.sh"),
            Vec::new(),
            String::new(),
            String::new(),
        )
    };
    write_body(&mut windows_body, &mut unix_body);
    let body = if cfg!(windows) {
        windows_body
    } else {
        unix_body
    };
    fs::write(&script_path, body).unwrap();
    let mut command = Command::new(if cfg!(windows) { "cmd" } else { "sh" });
    command
        .args(interpreter_args.iter().map(String::as_str))
        .arg(&script_path)
        .stdin(Stdio::piped())
        .stdout(Stdio::null())
        .stderr(Stdio::null());
    let child = command.spawn().unwrap();
    (child, directory)
}

fn remove_directory(directory: &Path) {
    let _ = fs::remove_dir_all(directory);
}

#[test]
fn core_guard_stops_gracefully_through_stdin_eof_without_kill() {
    let (child, directory) = stdin_holding_child();
    let mut guard = CoreProcessGuard::default();
    guard.replace(child);

    // Outcome only: a slow machine could legitimately observe EOF near the
    // deadline while still exiting gracefully, so no wall-clock bound here.
    // The kill-fallback test below proves the outcomes are distinguished.
    let outcome = guard.stop();

    assert_eq!(outcome, CoreStopOutcome::ExitedAfterGracefulClose);
    remove_directory(&directory);
}

#[test]
fn core_guard_kills_only_after_the_graceful_deadline() {
    let (child, directory) = stdin_ignoring_child();
    let mut guard = CoreProcessGuard::default();
    guard.replace(child);

    let started = Instant::now();
    let outcome = guard.stop_with_deadline(Duration::from_millis(50));
    let elapsed = started.elapsed();

    assert_eq!(outcome, CoreStopOutcome::KilledAfterGracefulTimeout);
    assert!(elapsed >= Duration::from_millis(50));
    remove_directory(&directory);
}

#[test]
fn core_guard_stop_without_a_child_reports_not_running() {
    let mut guard = CoreProcessGuard::default();
    assert_eq!(guard.stop(), CoreStopOutcome::NotRunning);
    assert_eq!(
        guard.stop_with_deadline(Duration::from_millis(10)),
        CoreStopOutcome::NotRunning
    );
}

#[test]
fn core_guard_replace_stops_the_previous_child_gracefully() {
    let (first, first_directory) = stdin_holding_child();
    let (second, second_directory) = stdin_holding_child();
    let mut guard = CoreProcessGuard::default();
    guard.replace(first);

    guard.replace(second);

    assert_eq!(guard.stop(), CoreStopOutcome::ExitedAfterGracefulClose);
    remove_directory(&first_directory);
    remove_directory(&second_directory);
}

#[test]
fn failed_setup_stops_the_core_before_releasing_session_ownership() {
    let (child, directory) = stdin_holding_child();
    let mut guard = CoreProcessGuard::default();
    guard.replace(child);
    let session_path = directory.join(format!("connect-{}-1", std::process::id()));
    let mut session = crate::session_cleanup::SessionDirectoryGuard::bind(session_path.clone())
        .expect("bind setup session directory");

    crate::menu::core_process::stop_core_then(&mut guard, |outcome| {
        assert_eq!(outcome, CoreStopOutcome::ExitedAfterGracefulClose);
        assert_eq!(
            session.release(),
            crate::session_cleanup::ReleaseOutcome::Removed,
        );
    });

    assert!(!session_path.exists());
    assert_eq!(guard.stop(), CoreStopOutcome::NotRunning);
    remove_directory(&directory);
}
