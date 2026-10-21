//! Checks real carrier diagnostics and the panic hook without a terminal or window.
use diagnostics::{
    console::SessionFiles,
    session_log::{Family, Retention},
};
use std::{
    fs,
    path::PathBuf,
    process::{Command, Stdio},
};

const FIXTURE_LOG_LIMIT: u64 = 8192;
const RAW_STDERR_MARKER: &str = "unrelated integration crate raw stderr fixture";
const QUEUED_STDERR_MARKER: &str = "queued tracing fixture";
const PANIC_MARKER: &str = "client logging panic fixture";
const LOG_MARKERS: [&str; 4] = [
    RAW_STDERR_MARKER,
    QUEUED_STDERR_MARKER,
    "particle carrier not found",
    PANIC_MARKER,
];

#[allow(dead_code)]
#[path = "../src/lifecycle/client_logs.rs"]
mod client_logs;
#[path = "../src/particles/carrier.rs"]
mod particle_carrier;

/// Uses the real installation layout while keeping every fixture file beneath its temporary root.
fn logging_layout(root: &std::path::Path) -> launcher::install_layout::InstallLayout {
    use launcher::install_layout::Platform;
    let platform = if cfg!(windows) {
        Platform::Windows
    } else if cfg!(target_os = "macos") {
        Platform::MacOs
    } else {
        Platform::Linux
    };
    launcher::test_support::scratch_at(platform, root)
}

/// Returns the fixture's client log using the production layout's directory policy.
fn client_log(root: &std::path::Path) -> PathBuf {
    client_logs(root).current()
}

/// The fixture's client session files.
fn client_logs(root: &std::path::Path) -> Family {
    Family::new(logging_layout(root).log_dir(), "client", "log")
}

#[test]
fn client_logging_child() {
    let Some(root) = std::env::var_os("CLIENT_LOGGING_FIXTURE") else {
        return;
    };
    let root = PathBuf::from(root);
    let limit = if std::env::var_os("CLIENT_LOGGING_ROTATE").is_some() {
        FIXTURE_LOG_LIMIT
    } else {
        FIXTURE_LOG_LIMIT * 4
    };
    let dir = logging_layout(&root).log_dir();
    let header = format!(
        "{} session={}",
        crash_report::section::CLIENT_SESSION_START,
        diagnostics::session::id()
    );
    diagnostics::console::initialize_log(&SessionFiles {
        dir: &dir,
        stem: "client",
        header: &header,
        json: false,
        retention: Retention {
            session_bytes: limit,
            ..Retention::DEFAULT
        },
    })
    .unwrap();
    eprintln!("{RAW_STDERR_MARKER}");
    diagnostics::log_stderr!("{QUEUED_STDERR_MARKER}");
    if std::env::var_os("CLIENT_LOGGING_ROTATE").is_some() {
        use std::io::Write;
        std::io::stderr()
            .write_all(&vec![b'x'; FIXTURE_LOG_LIMIT as usize * 3])
            .unwrap();
        eprintln!("raw stderr rotation tail");
        assert!(diagnostics::console::flush_before_exit());
        return;
    }
    client_logs::install_crash_capture(&logging_layout(&root), None);
    assert!(particle_carrier::load_optional_carrier(&root.join("missing-world")).is_none());
    panic!("{PANIC_MARKER}");
}

#[test]
fn startup_carrier_and_panic_diagnostics_reach_the_file_without_a_terminal() {
    let root = tempfile::tempdir().unwrap();
    let output = Command::new(std::env::current_exe().unwrap())
        .args(["--exact", "client_logging_child", "--nocapture"])
        .env("CLIENT_LOGGING_FIXTURE", root.path())
        .stdout(Stdio::null())
        .output()
        .unwrap();
    assert!(!output.status.success());
    let text = fs::read_to_string(client_log(root.path())).unwrap();
    let original = String::from_utf8_lossy(&output.stderr);
    for marker in LOG_MARKERS {
        assert_eq!(
            original.matches(marker).count(),
            1,
            "lost or repeated stderr: {original}"
        );
        assert_eq!(
            text.matches(marker).count(),
            1,
            "lost or repeated file output: {text}"
        );
    }
    let crashes = crash_report::folder::list(&logging_layout(root.path()).crash_dir());
    assert_eq!(crashes.len(), 1);
    let snapshot =
        fs::read_to_string(crashes[0].join(crash_report::folder::CLIENT_LOG_FILE)).unwrap();
    assert_eq!(
        snapshot.matches(PANIC_MARKER).count(),
        1,
        "missing or repeated panic in snapshot: {snapshot}"
    );
}

#[test]
fn redirected_stderr_file_retains_startup_and_panic_output() {
    let root = tempfile::tempdir().unwrap();
    let session = root.path().join("mcp-session.log");
    let status = Command::new(std::env::current_exe().unwrap())
        .args(["--exact", "client_logging_child", "--nocapture"])
        .env("CLIENT_LOGGING_FIXTURE", root.path())
        .stdout(Stdio::null())
        .stderr(Stdio::from(fs::File::create(&session).unwrap()))
        .status()
        .unwrap();
    assert!(!status.success());
    let original = fs::read_to_string(session).unwrap();
    let rotating = fs::read_to_string(client_log(root.path())).unwrap();
    for marker in LOG_MARKERS {
        assert_eq!(
            original.matches(marker).count(),
            1,
            "lost or repeated stderr: {original}"
        );
        assert_eq!(
            rotating.matches(marker).count(),
            1,
            "lost or repeated file output: {rotating}"
        );
    }
}

#[test]
fn raw_stderr_uses_the_same_bounded_rotation() {
    let root = tempfile::tempdir().unwrap();
    let status = Command::new(std::env::current_exe().unwrap())
        .args(["--exact", "client_logging_child", "--nocapture"])
        .env("CLIENT_LOGGING_FIXTURE", root.path())
        .env("CLIENT_LOGGING_ROTATE", "1")
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .status()
        .unwrap();
    assert!(status.success());
    let archives = client_logs(root.path()).archives().unwrap();
    assert!(!archives.is_empty(), "an oversized session must rotate");
    for path in archives.iter().chain([&client_log(root.path())]) {
        assert!(fs::metadata(path).unwrap().len() <= FIXTURE_LOG_LIMIT);
    }
    assert!(
        fs::read_to_string(client_log(root.path()))
            .unwrap()
            .contains("raw stderr rotation tail")
    );
}

#[cfg(unix)]
#[test]
fn terminal_panic_keeps_the_file_mirror_without_duplicate_terminal_output() {
    let root = tempfile::tempdir().unwrap();
    let output = Command::new("python3")
        .args([
            "-c",
            r#"
import os, pty, subprocess, sys, threading
master, slave = pty.openpty()
output = bytearray()
def drain():
    """Consume fixture terminal output until its only writer exits."""
    try:
        while True:
            data = os.read(master, 4096)
            if not data:
                break
            output.extend(data)
    except OSError:
        pass
thread = threading.Thread(target=drain, daemon=True)
thread.start()
try:
    result = subprocess.run(sys.argv[1:], stdout=subprocess.DEVNULL, stderr=slave, timeout=10)
finally:
    os.close(slave)
thread.join(timeout=1)
if thread.is_alive():
    raise RuntimeError("terminal reader did not finish")
os.close(master)
print(output.decode(errors='replace'))
sys.exit(result.returncode)
"#,
        ])
        .arg(std::env::current_exe().unwrap())
        .args(["--exact", "client_logging_child", "--nocapture"])
        .env("CLIENT_LOGGING_FIXTURE", root.path())
        .output()
        .unwrap();
    assert!(!output.status.success());
    let terminal = String::from_utf8_lossy(&output.stdout);
    assert_eq!(terminal.matches(PANIC_MARKER).count(), 1, "{terminal}");
    let log = fs::read_to_string(client_log(root.path())).unwrap();
    assert!(
        log.contains(PANIC_MARKER),
        "lost terminal panic mirror: {log}"
    );
}
