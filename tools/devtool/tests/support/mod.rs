//! Bounded execution for compiler-wrapper regressions.
use std::{
    fs,
    process::{Command, Output, Stdio},
    time::Duration,
};
use wait_timeout::ChildExt;

/// Captures a fixture's output and terminates its own child if a wrapper loops.
pub fn bounded_output(command: &mut Command) -> Output {
    let stdout = tempfile::NamedTempFile::new().unwrap();
    let stderr = tempfile::NamedTempFile::new().unwrap();
    let mut child = command
        .stdout(Stdio::from(stdout.reopen().unwrap()))
        .stderr(Stdio::from(stderr.reopen().unwrap()))
        .spawn()
        .unwrap();
    let status = match child.wait_timeout(Duration::from_secs(10)).unwrap() {
        Some(status) => status,
        None => {
            child.kill().unwrap();
            child.wait().unwrap();
            panic!(
                "compiler wrapper timed out: {}",
                fs::read_to_string(stderr.path()).unwrap()
            );
        }
    };
    Output {
        status,
        stdout: fs::read(stdout.path()).unwrap(),
        stderr: fs::read(stderr.path()).unwrap(),
    }
}
