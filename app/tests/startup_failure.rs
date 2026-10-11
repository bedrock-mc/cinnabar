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
        /// Starts file logging, then reproduces a preparation failure before the client runs.
        pub fn before_run(_: bool) -> std::io::Result<bool> {
            let path = std::env::var_os("STARTUP_FAILURE_LOG").unwrap();
            diagnostics::console::initialize_log(std::path::Path::new(&path), 1024).unwrap();
            Err(std::io::Error::other("first-time setup fixture failed"))
        }
    }
    /// A startup failure must exit before opening the game window.
    pub fn run(_: args::ClientArgs) -> std::io::Result<()> {
        unreachable!()
    }
}

#[allow(dead_code)]
mod entry {
    use crate::fixture_client as bedrock_client;
    include!("../src/main.rs");
    /// Invokes the production dispatch with the fixture lifecycle boundary.
    pub fn invoke() {
        main();
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
    let path = std::env::temp_dir().join(format!(
        "cinnabar-startup-failure-{}.log",
        std::process::id()
    ));
    let output = Command::new(std::env::current_exe().unwrap())
        .args(["--exact", "startup_failure_child", "--nocapture"])
        .env("STARTUP_FAILURE_LOG", &path)
        .output()
        .unwrap();
    let text = fs::read_to_string(&path).unwrap();
    fs::remove_file(path).unwrap();
    assert_eq!(output.status.code(), Some(1));
    assert!(text.contains("bedrock-client failed: first-time setup fixture failed"));
}
