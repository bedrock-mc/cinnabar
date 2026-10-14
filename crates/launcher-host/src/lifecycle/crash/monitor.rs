//! Starts this client's crash monitor and keeps the connection to it.
//!
//! The monitor is a separate process that outlives every exit path of this one: it watches its
//! stdin, which only this process holds open, and writes the report after this process is gone.

use std::{
    io::{BufRead, BufReader},
    path::Path,
    process::{ChildStdin, ChildStdout, Command, Stdio},
    sync::{Condvar, Mutex, OnceLock, PoisonError},
    time::Duration,
};

use super::protocol::{self, Hello};
use crate::lifecycle::children;

/// How to start the monitor: a program and the arguments before `<ipc name> <parent pid>
/// <crash dir>`.
#[derive(Clone, Debug)]
pub struct MonitorCommand {
    pub program: std::path::PathBuf,
    pub args: Vec<std::ffi::OsString>,
}

impl MonitorCommand {
    /// This executable re-run with [`super::MONITOR_FLAG`].
    pub fn current_exe() -> std::io::Result<Self> {
        Ok(Self {
            program: std::env::current_exe()?,
            args: vec![super::MONITOR_FLAG.into()],
        })
    }
}

/// The connection, set once the monitor accepted it and the hello was sent.
static CONNECTION: OnceLock<minidumper::Client> = OnceLock::new();
/// The monitor's process id once it started.
static PID: OnceLock<u32> = OnceLock::new();
/// The monitor's stdin, held open for this process's lifetime; its end tells the monitor this
/// process is gone.
static LIFELINE: OnceLock<Mutex<ChildStdin>> = OnceLock::new();
static PROGRESS: Mutex<Progress> = Mutex::new(Progress::Starting);
static SETTLED: Condvar = Condvar::new();

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
enum Progress {
    Starting,
    Connected,
    Unavailable,
}

/// Starts the monitor and connects to it on a background thread; reports fall back to this
/// process until the connection is up.
pub(super) fn start(command: &MonitorCommand, crash_dir: &Path, hello: Hello) {
    let pid = std::process::id();
    let name = protocol::ipc_name(pid, nonce());
    let mut monitor = Command::new(&command.program);
    monitor
        .args(&command.args)
        .arg(&name)
        .arg(pid.to_string())
        .arg(crash_dir)
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::null());
    detach_from_terminal(&mut monitor);
    let spawned = match children::spawn_outliving(&mut monitor) {
        Ok(spawned) => spawned,
        Err(error) => {
            tracing::warn!("crash monitor unavailable, reports stay in-process: {error}");
            settle(Progress::Unavailable);
            return;
        }
    };
    let _ = PID.set(spawned.id());
    if let Some(stdin) = spawned.take_stdin() {
        let _ = LIFELINE.set(Mutex::new(stdin));
    }
    let Some(stdout) = spawned.take_stdout() else {
        settle(Progress::Unavailable);
        return;
    };
    let connecting = std::thread::Builder::new()
        .name("crash-monitor".into())
        .spawn(move || connect(stdout, &name, &hello));
    if connecting.is_err() {
        settle(Progress::Unavailable);
    }
}

/// Waits for the monitor's ready line, connects and introduces this client, then drains the
/// monitor's stdout until it exits so its writes never fail.
fn connect(stdout: ChildStdout, name: &str, hello: &Hello) {
    let mut lines = BufReader::new(stdout).lines();
    let ready = lines
        .by_ref()
        .any(|line| line.is_ok_and(|line| line.trim_end() == protocol::READY));
    let connected = ready
        .then(|| minidumper::Client::with_name(protocol::socket_name(name)))
        .transpose()
        .map_err(|error| error.to_string())
        .and_then(|client| client.ok_or_else(|| "it exited before listening".to_owned()))
        .and_then(|client| {
            let hello = serde_json::to_vec(hello).map_err(|error| error.to_string())?;
            client
                .send_message(protocol::HELLO, hello)
                .map_err(|error| error.to_string())?;
            Ok(client)
        });
    match connected {
        Ok(client) => {
            let _ = CONNECTION.set(client);
            if let Some(pid) = PID.get() {
                super::handler::allow_dumps_by(*pid);
            }
            super::state_changed();
            settle(Progress::Connected);
            tracing::info!("crash monitor connected");
        }
        Err(error) => {
            tracing::warn!("crash monitor unavailable, reports stay in-process: {error}");
            settle(Progress::Unavailable);
        }
    }
    for _ in lines {}
}

fn settle(progress: Progress) {
    *PROGRESS.lock().unwrap_or_else(PoisonError::into_inner) = progress;
    SETTLED.notify_all();
}

/// The connection, once up; lock-free, so crash handlers may call it.
pub(super) fn connection() -> Option<&'static minidumper::Client> {
    CONNECTION.get()
}

/// Sends one message; false when there is no connection or the send failed.
pub(super) fn send(kind: u32, bytes: &[u8]) -> bool {
    connection().is_some_and(|client| client.send_message(kind, bytes).is_ok())
}

/// Waits up to `timeout` for the connection attempt to settle; true when connected.
pub fn wait_for_connection(timeout: Duration) -> bool {
    let progress = PROGRESS.lock().unwrap_or_else(PoisonError::into_inner);
    let (progress, _) = SETTLED
        .wait_timeout_while(progress, timeout, |progress| {
            *progress == Progress::Starting && LIFELINE.get().is_some()
        })
        .unwrap_or_else(PoisonError::into_inner);
    *progress == Progress::Connected
}

/// The monitor's process id, once started.
pub fn pid() -> Option<u32> {
    PID.get().copied()
}

/// Keeps the monitor out of this process's console and process group, so Ctrl+C in a terminal
/// reaches only the client, whose exit the monitor then sees.
fn detach_from_terminal(command: &mut Command) {
    #[cfg(unix)]
    {
        use std::os::unix::process::CommandExt;
        command.process_group(0);
    }
    #[cfg(windows)]
    crate::desktop::windows::hide_console(command);
}

/// A per-launch value that keeps IPC names from colliding with a stale socket.
fn nonce() -> u64 {
    let mut bytes = [0_u8; 8];
    if getrandom::fill(&mut bytes).is_err() {
        return u64::from(std::process::id());
    }
    u64::from_le_bytes(bytes)
}
