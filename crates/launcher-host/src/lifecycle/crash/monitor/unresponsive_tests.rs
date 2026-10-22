//! An owned monitor fixture that keeps its socket open without receiving dump metadata.

use super::*;
use std::sync::atomic::AtomicBool;

const FIXTURE: &str = "CINNABAR_STALLED_CRASH_MONITOR_FIXTURE";
const DONE: &str = "monitor-done";

/// Keeps the first introduced connection alive until its client dies, without draining it.
struct StalledMonitor;

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
    server
        .run(Box::new(StalledMonitor), &AtomicBool::new(false), None)
        .unwrap();
    std::fs::write(Path::new(&root).join(DONE), b"").unwrap();
}

/// Crashes only this disposable client after its monitor accepts the connection.
#[test]
fn stalled_client_fixture() {
    let Some(root) = std::env::var_os(FIXTURE) else {
        return;
    };
    let mut layout = launcher::test_support::checkout();
    layout.user_data_root = PathBuf::from(root);
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
    let started = std::time::Instant::now();
    let connected = wait_for_connection(test_time::DEFAULT_TIMEOUT);
    eprintln!("DIAG connected={connected} after {:?}", started.elapsed());
    assert!(connected);
    eprintln!("DIAG aborting");
    std::process::abort();
}

/// A nonresponsive monitor cannot prevent a fatal client from exiting and writing its fallback.
#[test]
fn nonresponsive_monitor_cannot_freeze_native_crash_capture() {
    let root = tempfile::tempdir().unwrap();
    let mut client = Command::new(std::env::current_exe().unwrap())
        .args([
            "--exact",
            "lifecycle::crash::monitor::unresponsive_tests::stalled_client_fixture",
            "--nocapture",
        ])
        .env(FIXTURE, root.path())
        .stdout(Stdio::null())
        .stderr(std::fs::File::create(root.path().join("client-stderr")).unwrap())
        .spawn()
        .unwrap();
    let started = std::time::Instant::now();
    let exited = test_time::wait_until(test_time::DEFAULT_TIMEOUT, || {
        client.try_wait().unwrap().is_some()
    });
    eprintln!("DIAG exited={exited} after {:?}", started.elapsed());
    if !exited {
        for (program, args) in [
            (
                "/bin/ps",
                vec!["-axo".to_owned(), "pid,ppid,stat,etime,command".to_owned()],
            ),
            (
                "/usr/bin/sample",
                vec![client.id().to_string(), "2".to_owned()],
            ),
        ] {
            if let Ok(output) = Command::new(program).args(&args).output() {
                let text = String::from_utf8_lossy(&output.stdout);
                for line in text.lines().filter(|line| {
                    program != "/bin/ps" || line.contains("stalled_") || line.contains("PID")
                }) {
                    eprintln!("DIAG {program}: {line}");
                }
            }
        }
        for entry in walk(root.path()) {
            eprintln!("DIAG file {}", entry.display());
        }
        // This handle belongs to the disposable client spawned immediately above.
        client.kill().unwrap();
    }
    client.wait().unwrap();
    assert!(test_time::wait_until(test_time::DEFAULT_TIMEOUT, || root
        .path()
        .join(DONE)
        .exists()));
    eprintln!(
        "DIAG client stderr: {}",
        std::fs::read_to_string(root.path().join("client-stderr")).unwrap_or_default()
    );
    assert!(
        exited,
        "crash handling blocked on a nonresponsive monitor (fixture pid {})",
        client.id()
    );
    let folders = crash_report::folder::list(&root.path().join("crashes"));
    assert_eq!(folders.len(), 1, "the in-process fallback report");
    let report = crash_report::folder::read(&folders[0]).unwrap();
    assert_eq!(report.kind, crash_report::report::Kind::Native);
}

fn walk(path: &Path) -> Vec<PathBuf> {
    let mut found = Vec::new();
    for entry in std::fs::read_dir(path).into_iter().flatten().flatten() {
        let path = entry.path();
        if path.is_dir() {
            found.extend(walk(&path));
        }
        found.push(path);
    }
    found
}
