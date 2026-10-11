//! Checks real carrier diagnostics and the panic hook without a terminal or window.
use std::{
    fs,
    path::PathBuf,
    process::{Command, Stdio},
};

const FIXTURE_LOG_LIMIT: u64 = 8192;

extern crate self as launcher;

mod install_layout {
    use std::path::PathBuf;
    pub struct InstallLayout(pub PathBuf);
    impl InstallLayout {
        /// Returns the fixture's log directory.
        pub fn log_dir(&self) -> PathBuf {
            self.0.clone()
        }
        /// Returns the fixture's crash directory.
        pub fn crash_dir(&self) -> PathBuf {
            self.0.join("crashes")
        }
    }
}
#[allow(dead_code, reason = "exercises the production panic hook in isolation")]
#[path = "../src/lifecycle/crash.rs"]
mod crash;
#[path = "../src/particles/carrier.rs"]
mod particle_carrier;

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
    diagnostics::console::initialize_log(&root.join("client.log"), limit).unwrap();
    eprintln!("unrelated integration crate raw stderr fixture");
    if std::env::var_os("CLIENT_LOGGING_ROTATE").is_some() {
        use std::io::Write;
        std::io::stderr()
            .write_all(&vec![b'x'; FIXTURE_LOG_LIMIT as usize * 3])
            .unwrap();
        eprintln!("raw stderr rotation tail");
        assert!(diagnostics::console::flush_before_exit());
        return;
    }
    crash::install_panic_hook(&install_layout::InstallLayout(root.clone()));
    assert!(particle_carrier::load_optional_carrier(&root.join("missing-world")).is_none());
    panic!("client logging panic fixture");
}

#[test]
fn startup_carrier_and_panic_diagnostics_reach_the_file_without_a_terminal() {
    let root = tempfile::tempdir().unwrap();
    let status = Command::new(std::env::current_exe().unwrap())
        .args(["--exact", "client_logging_child", "--nocapture"])
        .env("CLIENT_LOGGING_FIXTURE", root.path())
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .status()
        .unwrap();
    assert!(!status.success());
    let text = fs::read_to_string(root.path().join("client.log")).unwrap();
    assert!(
        text.contains("unrelated integration crate raw stderr fixture"),
        "lost unrelated raw stderr: {text}"
    );
    assert!(
        text.contains("particle carrier not found"),
        "lost startup stderr: {text}"
    );
    assert!(
        text.contains("client logging panic fixture"),
        "lost panic stderr: {text}"
    );
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
    for name in ["client.log", "client.log.1"] {
        assert!(fs::metadata(root.path().join(name)).unwrap().len() <= FIXTURE_LOG_LIMIT);
    }
    assert!(
        fs::read_to_string(root.path().join("client.log"))
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
    assert_eq!(
        terminal.matches("client logging panic fixture").count(),
        1,
        "{terminal}"
    );
    let log = fs::read_to_string(root.path().join("client.log")).unwrap();
    assert!(
        log.contains("client logging panic fixture"),
        "lost terminal panic mirror: {log}"
    );
}
