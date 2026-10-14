//! Real crashes of a fixture client, reported by a real crash monitor: this test binary plays
//! both, as subprocesses, with the crash window suppressed.

use std::{
    fs,
    io::{BufRead, BufReader},
    path::{Path, PathBuf},
    process::{Child, Command, Stdio},
};

use crash_report::{
    folder,
    report::{Identity, Kind},
};
use launcher_host::lifecycle::crash::{self, LogHooks, monitor};

const ROOT_ENV: &str = "CINNABAR_CRASH_FIXTURE";
const KIND_ENV: &str = "CINNABAR_CRASH_FIXTURE_KIND";
/// Written by the monitor fixture once it has finished, so the test knows it exited.
const MONITOR_DONE: &str = "monitor-done";
/// Printed by the client fixture once its monitor is connected.
const CONNECTED: &str = "CRASH_FIXTURE_CONNECTED";

fn layout(root: &Path) -> launcher::install_layout::InstallLayout {
    let mut layout = launcher::test_support::checkout();
    layout.user_data_root = root.to_owned();
    layout
}

/// The monitor half: serves the client fixture, then marks that it has finished.
#[test]
fn monitor_fixture() {
    let Some(root) = std::env::var_os(ROOT_ENV) else {
        return;
    };
    let root = PathBuf::from(root);
    let args: Vec<std::ffi::OsString> = std::env::args_os().collect();
    let args = crash_monitor::Args::parse(&args).expect("monitor arguments");
    // A real layout: its own log, and the crash window when the UI is not suppressed.
    crash_monitor::run(&args, Some(&layout(&root)));
    fs::write(root.join(MONITOR_DONE), b"").unwrap();
}

/// The client half: installs crash capture with the monitor fixture, waits for the connection,
/// then ends as its kind says.
#[test]
fn client_fixture() {
    let (Some(root), Some(kind)) = (std::env::var_os(ROOT_ENV), std::env::var(KIND_ENV).ok())
    else {
        return;
    };
    let root = PathBuf::from(root);
    let identity = Identity {
        session_id: "0123456789abcdef".into(),
        started_at_ms: 0,
        release: "1.2.3".into(),
        commit: "abc123".into(),
        build_profile: "debug".into(),
    };
    let hooks = LogHooks {
        log_panic: |_| {},
        flush: || {},
        drain_on_signal: || {},
    };
    let command = monitor::MonitorCommand {
        program: std::env::current_exe().unwrap(),
        args: ["--exact", "monitor_fixture", "--nocapture"]
            .into_iter()
            .map(Into::into)
            .collect(),
    };
    crash::install(&layout(&root), identity, hooks, Some(&command));
    assert!(monitor::wait_for_connection(test_time::DEFAULT_TIMEOUT));
    println!("{CONNECTED}");
    // SAFETY: Each fault ends this disposable process, which is what the fixture is for.
    unsafe {
        match kind.as_str() {
            "panic" => panic!("fixture panic"),
            "segfault" => sadness_generator::raise_segfault(),
            "stack_overflow" => sadness_generator::raise_stack_overflow(),
            "abort" => std::process::abort(),
            "kill" => {
                let _ = std::io::stdin().read_line(&mut String::new());
            }
            "clean" => {}
            other => panic!("unknown fixture kind {other}"),
        }
    }
}

/// Runs the client fixture for `kind` until its monitor finishes; `kill` kills the client once
/// it is connected. Returns the crash folders.
fn crash(kind: &str) -> (tempfile::TempDir, Vec<PathBuf>) {
    let root = tempfile::tempdir().unwrap();
    let logs = root.path().join("logs");
    fs::create_dir_all(&logs).unwrap();
    fs::write(
        logs.join("client.log"),
        "CLIENT_SESSION_START session=0123456789abcdef\nfixture client line\n",
    )
    .unwrap();
    let mut client: Child = Command::new(std::env::current_exe().unwrap())
        .args(["--exact", "client_fixture", "--nocapture"])
        .env(ROOT_ENV, root.path())
        .env(KIND_ENV, kind)
        .env(crash_monitor::CRASH_UI_ENV, "0")
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::null())
        .spawn()
        .unwrap();
    let mut lines = BufReader::new(client.stdout.take().unwrap()).lines();
    let connected = lines
        .by_ref()
        .any(|line| line.is_ok_and(|line| line.contains(CONNECTED)));
    assert!(connected, "{kind}: the client fixture never connected");
    // The harness keeps printing; a closed pipe would fail it after the fixture returns.
    let draining = std::thread::spawn(move || lines.for_each(drop));
    if kind == "kill" {
        client.kill().unwrap();
    }
    drop(client.stdin.take());
    let _ = client.wait();
    let _ = draining.join();
    let done = root.path().join(MONITOR_DONE);
    assert!(
        test_time::wait_until(test_time::DEFAULT_TIMEOUT, || done.exists()),
        "{kind}: the monitor did not exit with its client"
    );
    let folders = folder::list(&root.path().join("crashes"));
    (root, folders)
}

/// Exactly one folder of `kind`, with a dump when `dumped`, holding the client's log section.
fn assert_one(kind: &str, expected: Kind, dumped: bool) {
    let (_root, folders) = crash(kind);
    assert_eq!(folders.len(), 1, "{kind}: {folders:?}");
    let report = folder::read(&folders[0]).unwrap();
    assert_eq!(report.kind, expected, "{kind}: {}", report.message);
    assert_eq!(report.session_id, "0123456789abcdef");
    assert_eq!(
        folders[0].join(folder::DUMP_FILE).is_file(),
        dumped,
        "{kind}: crash.dmp"
    );
    assert!(
        fs::read_to_string(folders[0].join(folder::CLIENT_LOG_FILE))
            .unwrap()
            .contains("fixture client line")
    );
}

#[test]
fn a_panic_is_reported_with_a_dump() {
    assert_one("panic", Kind::Panic, true);
}

#[test]
fn an_access_violation_is_reported_with_a_dump() {
    assert_one("segfault", Kind::Native, true);
}

#[test]
fn a_stack_overflow_is_reported_with_a_dump() {
    assert_one("stack_overflow", Kind::Native, true);
}

/// Windows ends `abort` with a fail-fast that no handler sees; Unix raises `SIGABRT`.
#[test]
fn an_abort_is_reported() {
    if cfg!(windows) {
        assert_one("abort", Kind::Abort, false);
    } else {
        assert_one("abort", Kind::Native, true);
    }
}

#[test]
fn an_external_kill_is_reported_as_an_abort() {
    assert_one("kill", Kind::Abort, false);
}

#[test]
fn a_clean_exit_reports_nothing_and_the_monitor_exits_with_it() {
    let (_root, folders) = crash("clean");
    assert!(folders.is_empty(), "{folders:?}");
}
