//! Real crashes of a fixture client, reported by a real crash monitor: this test binary plays
//! both, as subprocesses, with the crash window suppressed.

use std::{
    fs,
    io::{BufRead, BufReader},
    path::{Path, PathBuf},
    process::{Child, Command, Stdio},
};

use ::protocol::packet_tap::{
    PacketBody, PacketDirection, PacketName, PacketOutcome, carries_secrets,
};
use bytes::Bytes;
use crash_report::{
    folder,
    report::{Identity, Kind, Thread},
};
use launcher_host::lifecycle::crash::{
    self, LogHooks, heartbeat, monitor,
    protocol::{self, Stage},
};

const ROOT_ENV: &str = "CINNABAR_CRASH_FIXTURE";
const KIND_ENV: &str = "CINNABAR_CRASH_FIXTURE_KIND";
/// When set, the monitor fixture starts listening only once this file exists.
const GO_ENV: &str = "CINNABAR_CRASH_FIXTURE_GO";
/// Written when the monitor finishes, holding its optional folder as JSON.
const MONITOR_DONE: &str = "monitor-done";
/// Printed by the client fixture once its monitor connection attempt settled.
const SETTLED: &str = "CRASH_FIXTURE_SETTLED";
/// How long the fixture client may stall before its monitor reports a hang.
const STALL_AFTER: std::time::Duration = std::time::Duration::from_millis(1_000);
/// Monitor samples that span two stall windows.
const TWO_WINDOWS_OF_CHECKS: u64 = 2 * crash_monitor::HANG_SAMPLES_PER_WINDOW + 5;

fn layout(root: &Path) -> launcher::install_layout::InstallLayout {
    let mut layout = launcher::test_support::checkout();
    layout.user_data_root = root.to_owned();
    layout
}

/// The monitor half: serves the client fixture, then records the folder it showed.
#[test]
fn monitor_fixture() {
    let Some(root) = std::env::var_os(ROOT_ENV) else {
        return;
    };
    let root = PathBuf::from(root);
    if let Some(go) = std::env::var_os(GO_ENV) {
        let go = PathBuf::from(go);
        assert!(test_time::wait_until(test_time::DEFAULT_TIMEOUT, || go.exists()));
    }
    let args: Vec<std::ffi::OsString> = std::env::args_os().collect();
    let args = crash_monitor::Args::parse(&args).expect("monitor arguments");
    diagnostics::logging::install(diagnostics::logging::Options::default()).unwrap();
    // A real layout: its own log, and the crash window when the UI is not suppressed.
    let shown = crash_monitor::run(&args, Some(&layout(&root))).expect("the monitor runs");
    let shown = shown.map(|folder| folder.to_string_lossy().into_owned());
    fs::write(root.join(MONITOR_DONE), serde_json::to_vec(&shown).unwrap()).unwrap();
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
        stall_after: STALL_AFTER,
    };
    crash::install(&layout(&root), identity, hooks, Some(&command));
    record_traffic();
    if kind == "early_panic" {
        // The monitor listens only after this panic, so it is written in-process.
        panic_on_a_thread();
        if let Some(go) = std::env::var_os(GO_ENV) {
            fs::write(go, b"").unwrap();
        }
        monitor::wait_for_connection(test_time::DEFAULT_TIMEOUT);
        println!("{SETTLED}");
        std::process::abort();
    }
    assert!(monitor::wait_for_connection(test_time::DEFAULT_TIMEOUT));
    println!("{SETTLED}");
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
            "thread_panic_then_clean" => {
                panic_on_a_thread();
                crash::announce_exit(0);
            }
            "thread_panic_then_segfault" => {
                panic_on_a_thread();
                sadness_generator::raise_segfault();
            }
            "intruder" => intrude(),
            "startup_failure" | "runtime_failure" => {
                let stage = if kind == "startup_failure" {
                    Stage::Startup
                } else {
                    Stage::Runtime
                };
                assert!(crash::report_failure(stage, "fixture failure"));
                crash::announce_exit(1);
                std::process::exit(1);
            }
            "hang_then_kill" => {
                heartbeat::tick();
                let _ = std::io::stdin().read_line(&mut String::new());
            }
            "loading_stall" => {
                heartbeat::tick();
                let loading = heartbeat::Loading::begin();
                let checks = heartbeat::monitor_checks();
                test_time::eventually("the monitor watched two windows", || {
                    heartbeat::monitor_checks() >= checks + TWO_WINDOWS_OF_CHECKS
                });
                drop(loading);
                crash::announce_exit(0);
            }
            "hang_recover_hang" => {
                let crashes = root.join("crashes");
                heartbeat::tick();
                test_time::eventually("the first hang report", || {
                    folder::list(&crashes).len() == 1
                });
                let checks = heartbeat::monitor_checks();
                test_time::eventually("the monitor saw progress", || {
                    heartbeat::tick();
                    heartbeat::monitor_checks() >= checks + 2
                });
                test_time::eventually("the second hang report", || {
                    folder::list(&crashes).len() == 2
                });
                crash::announce_exit(0);
            }
            other => panic!("unknown fixture kind {other}"),
        }
    }
}

/// A panic the process survives.
fn panic_on_a_thread() {
    let panicked = std::thread::Builder::new()
        .name("fixture-worker".into())
        .spawn(|| panic!("fixture thread panic"))
        .unwrap()
        .join();
    assert!(panicked.is_err());
}

/// The packet ID the fixture sends: the first whose body samples keep.
fn fixture_packet() -> u32 {
    (1..0x400)
        .find(|id| !carries_secrets(*id))
        .expect("a packet without secrets")
}

/// Logs a debug line only the flight recorder keeps and sends three packets through a session
/// tap, rejecting one, after moving both rings into the crash staging directory as the client
/// does.
fn record_traffic() {
    diagnostics::logging::install(diagnostics::logging::Options {
        base_filter: "info".into(),
        flight_recorder: true,
        ..Default::default()
    })
    .unwrap();
    let staging = crash::staging_dir().expect("a crash staging directory");
    diagnostics::logging::flight::attach(staging).unwrap();
    client_session::packet_log::global()
        .attach(staging)
        .unwrap();
    tracing::debug!(target: "client_session::fixture", "fixture flight line");
    let tap = client_session::packet_log::fixture_tap();
    let body = Bytes::from_static(&[1, 2, 3]);
    let packet = fixture_packet();
    tap.record(
        PacketDirection::Clientbound,
        packet,
        PacketBody::whole(&body),
    );
    let rejected = tap.record(
        PacketDirection::Clientbound,
        packet,
        PacketBody::whole(&body),
    );
    tap.reject(
        rejected,
        packet,
        PacketOutcome::Failed,
        PacketBody::whole(&body),
    );
    tap.record(
        PacketDirection::Serverbound,
        packet,
        PacketBody::whole(&body),
    );
    client_session::packet_log::fixture_flush_samples();
}

/// Another process connects to the monitor beside the client and claims a panic, a failure
/// and the client's identity, with a wrong token and with none.
fn intrude() {
    let socket = monitor::socket().expect("the monitor's socket");
    let intruder = minidumper::Client::with_name(protocol::socket_name(socket)).unwrap();
    let hello = serde_json::to_vec(&protocol::Hello {
        identity: Identity {
            session_id: "fedcba9876543210".into(),
            started_at_ms: 0,
            release: "6.6.6".into(),
            commit: "forged".into(),
            build_profile: "release".into(),
        },
        client_log: PathBuf::from("forged.log"),
        core_log: PathBuf::from("forged.log"),
        full_dump: false,
        staging: None,
    })
    .unwrap();
    let panic = serde_json::to_vec(&protocol::Panic {
        message: "forged panic".into(),
        thread: Thread::default(),
        backtrace: String::new(),
        uptime_ms: 0,
    })
    .unwrap();
    let wrong = [0x5a_u8; protocol::TOKEN_BYTES];
    for (kind, payload) in [
        (protocol::HELLO, hello.as_slice()),
        (protocol::PANIC, panic.as_slice()),
        (protocol::RUNTIME_FAILURE, b"forged failure".as_slice()),
    ] {
        intruder.send_message(kind, payload).unwrap();
        intruder
            .send_message(kind, [wrong.as_slice(), payload].concat())
            .unwrap();
    }
    crash::announce_exit(0);
}

/// What one fixture run left behind.
struct Run {
    root: tempfile::TempDir,
    folders: Vec<PathBuf>,
    /// The folder the monitor showed, if any.
    shown: Option<PathBuf>,
}

impl Drop for Run {
    /// Prints fixture diagnostics before a failed run removes its temporary files.
    #[expect(
        clippy::print_stderr,
        reason = "keeps crash fixture logs in test failure output"
    )]
    fn drop(&mut self) {
        if std::thread::panicking() {
            for folder in &self.folders {
                eprintln!("REPORT: {:?}", folder::read(folder));
            }
            for log in fs::read_dir(layout(self.root.path()).log_dir())
                .into_iter()
                .flatten()
                .flatten()
            {
                if let Ok(contents) = fs::read_to_string(log.path()) {
                    eprintln!("{}:\n{contents}", log.path().display());
                }
            }
        }
    }
}

/// Runs the client fixture for `kind` until its monitor finishes; `kill` kills the client once
/// it is connected.
fn crash(kind: &str) -> Run {
    let root = tempfile::tempdir().unwrap();
    let logs = root.path().join("logs");
    fs::create_dir_all(&logs).unwrap();
    fs::write(
        logs.join("client.log"),
        "CLIENT_SESSION_START session=0123456789abcdef\nfixture client line\n",
    )
    .unwrap();
    let mut client = Command::new(std::env::current_exe().unwrap());
    client
        .args(["--exact", "client_fixture", "--nocapture"])
        .env(ROOT_ENV, root.path())
        .env(KIND_ENV, kind)
        .env(crash_monitor::CRASH_UI_ENV, "0")
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::null());
    if kind == "early_panic" {
        client.env(GO_ENV, root.path().join("go"));
    }
    let mut client: Child = client.spawn().unwrap();
    let mut lines = BufReader::new(client.stdout.take().unwrap()).lines();
    let settled = lines
        .by_ref()
        .any(|line| line.is_ok_and(|line| line.contains(SETTLED)));
    assert!(settled, "{kind}: the client fixture never settled");
    // The harness keeps printing; a closed pipe would fail it after the fixture returns.
    let draining = std::thread::spawn(move || lines.for_each(drop));
    if kind == "hang_then_kill" {
        let crashes = root.path().join("crashes");
        test_time::eventually("the hang report", || !folder::list(&crashes).is_empty());
    }
    if kind == "kill" || kind == "hang_then_kill" {
        client.kill().unwrap();
    }
    drop(client.stdin.take());
    let _ = client.wait();
    let _ = draining.join();
    let done = root.path().join(MONITOR_DONE);
    let mut shown = None;
    test_time::eventually(&format!("{kind}: the monitor's complete result"), || {
        fs::read(&done)
            .ok()
            .and_then(|bytes| serde_json::from_slice::<Option<PathBuf>>(&bytes).ok())
            .is_some_and(|result| {
                shown = result;
                true
            })
    });
    Run {
        folders: folder::list(&root.path().join("crashes")),
        shown,
        root,
    }
}

/// Asserts `folder` holds a `kind` report, with a dump when `dumped`, the client's log section,
/// and what [`record_traffic`] left in the staging directory.
fn assert_report(folder: &Path, kind: Kind, dumped: bool) {
    let report = folder::read(folder).unwrap();
    assert_eq!(report.kind, kind, "{}", report.message);
    assert_eq!(report.session_id, "0123456789abcdef");
    assert_eq!(
        folder.join(folder::DUMP_FILE).is_file(),
        dumped,
        "{kind:?}: crash.dmp"
    );
    assert!(
        fs::read_to_string(folder.join(folder::CLIENT_LOG_FILE))
            .unwrap()
            .contains("fixture client line")
    );
    assert_traffic(folder);
}

/// Asserts `folder` carries the packets, sample and log line [`record_traffic`] recorded.
fn assert_traffic(folder: &Path) {
    let packets = fs::read_to_string(folder.join(folder::PACKETS_FILE)).unwrap();
    let name = format!("{} ({})", PacketName(fixture_packet()), fixture_packet());
    assert!(
        packets.starts_with("packet headers: 3 recorded, last 3 kept\n"),
        "{packets}"
    );
    assert_eq!(packets.matches(&name).count(), 3, "{packets}");
    assert!(
        packets.contains("serverbound") && packets.contains("failed"),
        "{packets}"
    );
    let index =
        fs::read_to_string(folder.join(folder::DECODE_SAMPLES_DIR).join("index.txt")).unwrap();
    assert!(index.contains("1 kept (3 bytes)"), "{index}");
    let flight = fs::read_to_string(folder.join(folder::FLIGHT_LOG_FILE)).unwrap();
    assert!(
        flight.contains("DEBUG") && flight.contains("fixture flight line"),
        "{flight}"
    );
}

/// Exactly one folder of `kind`, with a dump when `dumped`, which the monitor showed.
fn assert_one(fixture: &str, kind: Kind, dumped: bool) {
    let run = crash(fixture);
    assert_eq!(run.folders.len(), 1, "{fixture}: {:?}", run.folders);
    assert_report(&run.folders[0], kind, dumped);
    assert_eq!(run.shown.as_ref(), Some(&run.folders[0]), "{fixture}");
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
    let run = crash("clean");
    assert!(run.folders.is_empty(), "{:?}", run.folders);
    assert_eq!(run.shown, None);
}

#[test]
fn a_panic_the_client_survives_is_saved_without_a_window() {
    let run = crash("thread_panic_then_clean");
    assert_eq!(run.folders.len(), 1, "{:?}", run.folders);
    assert_report(&run.folders[0], Kind::Panic, true);
    assert_eq!(run.shown, None);
}

#[test]
fn a_fault_after_a_survived_panic_gets_its_own_report() {
    let run = crash("thread_panic_then_segfault");
    assert_eq!(run.folders.len(), 2, "{:?}", run.folders);
    let kinds: Vec<Kind> = run
        .folders
        .iter()
        .map(|folder| folder::read(folder).unwrap().kind)
        .collect();
    let panic = kinds.iter().position(|kind| *kind == Kind::Panic).unwrap();
    let native = kinds.iter().position(|kind| *kind == Kind::Native).unwrap();
    assert_report(&run.folders[panic], Kind::Panic, true);
    assert_report(&run.folders[native], Kind::Native, true);
    assert_eq!(run.shown.as_ref(), Some(&run.folders[native]));
}

#[test]
fn a_crash_before_the_monitor_connects_is_reported_once() {
    let run = crash("early_panic");
    assert_eq!(run.folders.len(), 1, "{:?}", run.folders);
    let report = folder::read(&run.folders[0]).unwrap();
    assert_eq!(report.kind, Kind::Panic);
    // Written in-process, with the same attachments the monitor adds.
    assert_traffic(&run.folders[0]);
    assert_eq!(run.shown, None);
}

#[test]
fn messages_from_another_connection_or_with_a_wrong_token_are_ignored() {
    let run = crash("intruder");
    assert!(run.folders.is_empty(), "{:?}", run.folders);
    assert_eq!(run.shown, None);
}

#[test]
fn failures_before_and_after_startup_are_reported_by_stage() {
    for (fixture, kind) in [
        ("startup_failure", Kind::Startup),
        ("runtime_failure", Kind::Runtime),
    ] {
        let run = crash(fixture);
        assert_eq!(run.folders.len(), 1, "{fixture}: {:?}", run.folders);
        let report = folder::read(&run.folders[0]).unwrap();
        assert_eq!(report.kind, kind);
        assert_eq!(report.message, "fixture failure");
        assert_eq!(run.shown.as_ref(), Some(&run.folders[0]));
    }
}

/// The monitor dumps a live client on Windows and Linux; macOS denies it the client's task.
const HANG_DUMPED: bool = cfg!(any(windows, target_os = "linux"));

/// Asserts `folder` holds a hang report whose dump, where there is one, holds the watched
/// thread among the others.
fn assert_hang(folder: &Path) {
    assert_report(folder, Kind::Hang, HANG_DUMPED);
    let report = folder::read(folder).unwrap();
    if !HANG_DUMPED {
        assert!(report.message.contains("no minidump"), "{}", report.message);
        return;
    }
    let dump = minidump::Minidump::read_path(folder.join(folder::DUMP_FILE)).unwrap();
    let threads = dump.get_stream::<minidump::MinidumpThreadList>().unwrap();
    let watched: u32 = report.thread.id.parse().unwrap();
    assert!(threads.threads.len() > 1, "a dump of every thread");
    assert!(
        threads
            .threads
            .iter()
            .any(|thread| thread.raw.thread_id == watched),
        "the watched thread {watched} is in the dump"
    );
}

#[test]
fn a_hung_client_killed_by_the_player_is_reported_once_as_a_hang() {
    let run = crash("hang_then_kill");
    assert_eq!(run.folders.len(), 1, "{:?}", run.folders);
    assert_hang(&run.folders[0]);
    assert_eq!(run.shown.as_ref(), Some(&run.folders[0]));
}

#[test]
fn a_stall_while_loading_is_not_a_hang() {
    let run = crash("loading_stall");
    assert_eq!(run.folders, Vec::<PathBuf>::new());
    assert_eq!(run.shown, None);
}

#[test]
fn a_stall_after_recovery_gets_its_own_report_and_neither_shows_a_window() {
    let run = crash("hang_recover_hang");
    assert_eq!(run.folders.len(), 2, "{:?}", run.folders);
    for folder in &run.folders {
        assert_hang(folder);
    }
    assert_eq!(run.shown, None);
}
