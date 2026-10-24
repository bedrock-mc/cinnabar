//! Exercises the real entry point with a failing startup boundary and no window or terminal.
use std::{fs, process::Command};

#[allow(dead_code)]
mod fixture_client {
    pub mod args {
        pub const HELP: &str = "fixture";
        pub struct ClientArgs {
            pub assets: Option<()>,
        }
        pub enum ParseOutcome {
            Help,
            Run(Box<ClientArgs>),
        }
        impl ClientArgs {
            /// Supplies valid arguments so the fixture reaches startup.
            pub fn parse_env() -> std::io::Result<ParseOutcome> {
                Ok(ParseOutcome::Run(Box::new(Self { assets: None })))
            }
        }
    }
    pub mod lifecycle {
        pub const FIRST_RUN_SETUP_FLAG: &str = "--fixture-setup";
        /// Keeps the setup subprocess outside this startup-error fixture.
        pub fn run_first_run_setup() -> i32 {
            unreachable!()
        }
        /// Keeps the crash monitor subprocess outside this startup-error fixture.
        pub fn run_crash_monitor() -> i32 {
            unreachable!()
        }
        /// The fixture runs with a console, where failures need no window.
        pub fn show_failure(_: &anyhow::Error) {}
        /// Starts file logging, then reproduces a preparation failure before the client runs.
        pub fn before_run(_: bool) -> anyhow::Result<bool> {
            let dir = std::path::PathBuf::from(std::env::var_os("STARTUP_FAILURE_LOG").unwrap());
            diagnostics::console::initialize_log(&diagnostics::console::SessionFiles {
                dir: &dir,
                stem: "client",
                header: "CLIENT_SESSION_START fixture",
                json: false,
                retention: diagnostics::session_log::Retention {
                    session_bytes: 1024,
                    ..diagnostics::session_log::Retention::DEFAULT
                },
            })
            .unwrap();
            if std::env::var_os("STARTUP_FAILURE_RENDERER").is_some() {
                return Ok(true);
            }
            Err(anyhow::anyhow!("first-time setup fixture failed"))
        }
    }
    /// Supplies a renderer failure through the same result the production app returns.
    pub fn run(_: args::ClientArgs) -> anyhow::Result<()> {
        Err(anyhow::anyhow!(
            "Renderer startup failed: GPU lacks VERTEX_STORAGE"
        ))
    }
}

#[allow(dead_code)]
mod entry {
    use crate::fixture_client as cinnabar;
    include!("../src/entry.rs");
    /// Invokes the production dispatch with the fixture lifecycle boundary.
    pub fn invoke() {
        dispatch();
    }
}

#[test]
fn startup_failure_child() {
    if std::env::var_os("STARTUP_FAILURE_LOG").is_some() {
        entry::invoke();
    }
}

#[test]
fn startup_errors_reach_client_log_before_process_exit() {
    let dir = tempfile::tempdir().unwrap();
    let output = Command::new(std::env::current_exe().unwrap())
        .args(["--exact", "startup_failure_child", "--nocapture"])
        .env("STARTUP_FAILURE_LOG", dir.path())
        .output()
        .unwrap();
    assert_eq!(
        output.status.code(),
        Some(1),
        "startup fixture failed: {}",
        String::from_utf8_lossy(&output.stderr)
    );
    let text = fs::read_to_string(dir.path().join("client.log")).unwrap();
    assert!(text.contains("cinnabar failed: first-time setup fixture failed"));
}

#[test]
fn renderer_errors_reach_the_entry_point_without_a_panic() {
    let dir = tempfile::tempdir().unwrap();
    let output = Command::new(std::env::current_exe().unwrap())
        .args(["--exact", "startup_failure_child", "--nocapture"])
        .env("STARTUP_FAILURE_LOG", dir.path())
        .env("STARTUP_FAILURE_RENDERER", "1")
        .output()
        .unwrap();
    assert_eq!(output.status.code(), Some(1));
    let text = fs::read_to_string(dir.path().join("client.log")).unwrap();
    assert!(text.contains("Renderer startup failed: GPU lacks VERTEX_STORAGE"));
    assert!(!String::from_utf8_lossy(&output.stderr).contains("panicked"));
}
