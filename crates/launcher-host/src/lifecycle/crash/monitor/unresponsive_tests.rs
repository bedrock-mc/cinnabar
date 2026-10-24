//! An owned monitor fixture that keeps its socket open without receiving dump metadata.

use super::*;
use std::io::{Read, Seek, SeekFrom};
use std::sync::atomic::AtomicBool;

const FIXTURE: &str = "CINNABAR_STALLED_CRASH_MONITOR_FIXTURE";
const DONE: &str = "monitor-done";
const CLIENT_STARTED: &str = "client-started";
const CLIENT_INSTALLED: &str = "client-installed";
const CLIENT_CONNECTED: &str = "client-connected";
const CLIENT_ABORT_READY: &str = "client-abort-ready";
const MONITOR_READY: &str = "monitor-ready";
const MONITOR_HELLO: &str = "monitor-hello";
const CAPTURE_BYTES: u64 = 8 * 1024;

/// Keeps the first introduced connection alive until its client dies, without draining it.
struct StalledMonitor {
    root: PathBuf,
}

impl minidumper::ServerHandler for StalledMonitor {
    /// This fixture never receives a Mach context while it is stalled on the introduction.
    fn create_minidump_file(&self) -> io::Result<(std::fs::File, PathBuf)> {
        Err(io::Error::other("stalled monitor cannot dump"))
    }

    /// Leaves cleanup to the client's lifeline rather than pretending a dump succeeded.
    fn on_minidump_created(
        &self,
        _: Result<minidumper::MinidumpBinary, minidumper::Error>,
    ) -> minidumper::LoopAction {
        minidumper::LoopAction::Continue
    }

    /// Stops reading socket messages after registration; EOF proves the fixture client died.
    fn on_message(&self, kind: u32, _: Vec<u8>) {
        if kind == protocol::HELLO {
            mark(&self.root, MONITOR_HELLO);
            let _ = io::copy(&mut io::stdin(), &mut io::sink());
        }
    }

    /// Exits once the owned client has closed its last connection.
    fn on_client_disconnected(&self, open: usize) -> minidumper::LoopAction {
        if open == 0 {
            minidumper::LoopAction::Exit
        } else {
            minidumper::LoopAction::Continue
        }
    }
}

/// Registers the minidumper client, then stops serving its messages without closing the socket.
#[test]
fn stalled_monitor_fixture() {
    let Some(root) = std::env::var_os(FIXTURE) else {
        return;
    };
    let args: Vec<_> = std::env::args_os().collect();
    let socket = &args[args.len() - 3];
    let mut launch = String::new();
    io::stdin().read_line(&mut launch).unwrap();
    let _: protocol::Launch = serde_json::from_str(&launch).unwrap();
    let mut server =
        minidumper::Server::with_name(protocol::socket_name(Path::new(socket))).unwrap();
    println!("{}", protocol::READY);
    io::stdout().flush().unwrap();
    mark(Path::new(&root), MONITOR_READY);
    server
        .run(
            Box::new(StalledMonitor {
                root: PathBuf::from(&root),
            }),
            &AtomicBool::new(false),
            None,
        )
        .unwrap();
    std::fs::write(Path::new(&root).join(DONE), b"").unwrap();
}

/// Crashes only this disposable client after its monitor accepts the connection.
#[test]
fn stalled_client_fixture() {
    let Some(root) = std::env::var_os(FIXTURE) else {
        return;
    };
    let root = PathBuf::from(root);
    mark(&root, CLIENT_STARTED);
    let mut layout = launcher::test_support::checkout();
    layout.user_data_root = root.clone();
    let command = MonitorCommand {
        program: std::env::current_exe().unwrap(),
        args: [
            "--exact",
            "lifecycle::crash::monitor::unresponsive_tests::stalled_monitor_fixture",
            "--nocapture",
        ]
        .into_iter()
        .map(Into::into)
        .collect(),
        stall_after: heartbeat::STALL_AFTER,
    };
    super::super::install(
        &layout,
        crash_report::report::Identity {
            session_id: "0123456789abcdef".into(),
            started_at_ms: 0,
            release: "fixture".into(),
            commit: "fixture".into(),
            build_profile: "debug".into(),
        },
        super::super::LogHooks {
            log_panic: |_| {},
            flush: || {},
            drain_on_signal: || {},
        },
        Some(&command),
    );
    mark(&root, CLIENT_INSTALLED);
    let connected = wait_for_connection(test_time::DEFAULT_TIMEOUT);
    if connected {
        mark(&root, CLIENT_CONNECTED);
    }
    assert!(connected);
    mark(&root, CLIENT_ABORT_READY);
    std::process::abort();
}

/// A nonresponsive monitor cannot prevent a fatal client from exiting and writing its fallback.
#[test]
fn nonresponsive_monitor_cannot_freeze_native_crash_capture() {
    let root = tempfile::tempdir().unwrap();
    let mut stdout = tempfile::tempfile().unwrap();
    let mut stderr = tempfile::tempfile().unwrap();
    let mut client = Command::new(std::env::current_exe().unwrap())
        .args([
            "--exact",
            "lifecycle::crash::monitor::unresponsive_tests::stalled_client_fixture",
            "--nocapture",
        ])
        .env(FIXTURE, root.path())
        .stdout(stdout.try_clone().unwrap())
        .stderr(stderr.try_clone().unwrap())
        .spawn()
        .unwrap();
    let exited = test_time::wait_until(test_time::DEFAULT_TIMEOUT, || {
        client.try_wait().unwrap().is_some()
    });
    let at_cutoff = (!exited).then(|| fixture_state(root.path()));
    if !exited {
        // This handle belongs to the disposable client spawned immediately above.
        client.kill().unwrap();
    }
    client.wait().unwrap();
    let monitor_done = test_time::wait_until(test_time::DEFAULT_TIMEOUT, || {
        root.path().join(DONE).exists()
    });
    let mut diagnostics = || {
        format!(
            "at_exit_cutoff={at_cutoff:?}; after_cleanup={}; stdout_tail={:?}; stderr_tail={:?}",
            fixture_state(root.path()),
            captured_tail(&mut stdout),
            captured_tail(&mut stderr),
        )
    };
    assert!(monitor_done, "monitor did not finish: {}", diagnostics());
    assert!(
        exited,
        "crash handling blocked on a nonresponsive monitor (fixture pid {}): {}",
        client.id(),
        diagnostics(),
    );
    let folders = crash_report::folder::list(&root.path().join("crashes"));
    assert_eq!(
        folders.len(),
        1,
        "the in-process fallback report: {}",
        diagnostics()
    );
    let report = crash_report::folder::read(&folders[0])
        .unwrap_or_else(|error| panic!("fallback report: {error}; {}", diagnostics()));
    assert_eq!(
        report.kind,
        crash_report::report::Kind::Native,
        "{}",
        diagnostics()
    );
}

/// Records fixed fixture boundaries without changing a verdict if diagnostics cannot be written.
fn mark(root: &Path, phase: &str) {
    let _ = std::fs::write(root.join(phase), b"");
}

/// Distinguishes setup from abort readiness without claiming a written report finished its fsyncs.
fn fixture_state(root: &Path) -> String {
    let phases = [
        CLIENT_STARTED,
        CLIENT_INSTALLED,
        CLIENT_CONNECTED,
        CLIENT_ABORT_READY,
        MONITOR_READY,
        MONITOR_HELLO,
        DONE,
    ]
    .map(|phase| (phase, root.join(phase).is_file()));
    let folders = crash_report::folder::list(&root.join("crashes"));
    let readable_native = folders
        .iter()
        .filter_map(|folder| crash_report::folder::read(folder).ok())
        .filter(|report| report.kind == crash_report::report::Kind::Native)
        .count();
    format!(
        "phases={phases:?}, crash_folders={}, readable_native_reports={readable_native}",
        folders.len()
    )
}

/// Reads only the bounded output tail after the owned client has been reaped.
fn captured_tail(file: &mut std::fs::File) -> io::Result<String> {
    let length = file.metadata()?.len();
    file.seek(SeekFrom::Start(length.saturating_sub(CAPTURE_BYTES)))?;
    let mut bytes = Vec::new();
    file.take(CAPTURE_BYTES).read_to_end(&mut bytes)?;
    Ok(String::from_utf8_lossy(&bytes).into_owned())
}
