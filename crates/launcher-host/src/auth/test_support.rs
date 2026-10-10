//! Scripted child fixtures for auth and menu integration tests.
use super::*;
use std::{
    fs,
    path::PathBuf,
    process::Child,
    time::{SystemTime, UNIX_EPOCH},
};

/// Starts an auth event script that exits after writing its lines.
pub fn event_child(lines: &[&str]) -> (Child, PathBuf) {
    event_child_with_policy(lines, false)
}

/// Starts an auth event script that waits for its input pipe to close.
pub fn event_child_holding(lines: &[&str]) -> (Child, PathBuf) {
    event_child_with_policy(lines, true)
}

/// Creates an isolated script fixture with the requested lifetime.
fn event_child_with_policy(lines: &[&str], hold_open: bool) -> (Child, PathBuf) {
    let unique = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap()
        .as_nanos();
    let directory = std::env::temp_dir().join(format!(
        "cinnabar-auth-helper-{}-{unique}",
        std::process::id()
    ));
    fs::create_dir_all(&directory).unwrap();
    let mut command = if cfg!(windows) {
        let script = directory.join("events.cmd");
        let hold = if hold_open { "set /p hold=\r\n" } else { "" };
        let body = format!(
            "@echo off\r\n{}\r\n{hold}",
            lines
                .iter()
                .map(|line| format!("echo {line}"))
                .collect::<Vec<_>>()
                .join("\r\n")
        );
        fs::write(&script, body).unwrap();
        let mut command = Command::new("cmd");
        command.args(["/Q", "/C"]).arg(script);
        command
    } else {
        let script = directory.join("events.sh");
        let hold = if hold_open { "IFS= read -r hold\n" } else { "" };
        let body = format!(
            "#!/bin/sh\n{}\n{hold}",
            lines
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
        .stdin(if hold_open {
            Stdio::piped()
        } else {
            Stdio::null()
        })
        .stdout(Stdio::piped())
        .spawn()
        .unwrap();
    (child, directory)
}
/// Wraps a scripted helper at the requested auth state without changing production admission.
pub fn supervisor(child: Child, state: AuthState, terminal: bool) -> AuthSupervisor {
    let mut supervisor = AuthSupervisor::from_child(child).unwrap();
    supervisor.state = state;
    supervisor.terminal = terminal;
    supervisor
}
impl AuthSupervisor {
    /// Reports whether a menu action requested cancellation of its fixture helper.
    pub fn test_cancel_requested(&self) -> bool {
        self.cancel_requested
    }
}

/// Attaches the normal event reader to a scripted child in its initial checking state.
pub fn from_child(child: Child) -> Result<AuthSupervisor> {
    AuthSupervisor::from_child(child)
}
