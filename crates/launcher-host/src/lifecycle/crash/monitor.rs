//! Starts this client's crash monitor and keeps the connection to it.
//!
//! The monitor is a separate process that outlives every exit path of this one: it watches its
//! stdin, which only this process holds open, and writes the report after this process is gone.
//! Its socket sits in a directory only this user can enter, and every message carries the
//! token this process handed it on that stdin.

mod slot;
#[cfg(all(test, target_os = "macos"))]
mod unresponsive_tests;

use std::{
    io::{self, BufRead, BufReader, Write},
    path::{Path, PathBuf},
    process::{ChildStdin, ChildStdout, Command, Stdio},
    sync::{
        Condvar, Mutex, OnceLock, PoisonError,
        atomic::{AtomicU8, Ordering},
    },
    time::Duration,
};

use super::{
    heartbeat,
    protocol::{self, HangWatch, Hello, Launch, Restart, TOKEN_BYTES, Token},
};
use crate::lifecycle::children;

/// How to start the monitor: a program and the arguments before `<socket> <parent pid>
/// <crash dir>`, and how long the main thread may stall before the monitor reports a hang.
#[derive(Clone, Debug)]
pub struct MonitorCommand {
    pub program: PathBuf,
    pub args: Vec<std::ffi::OsString>,
    pub stall_after: Duration,
}

impl MonitorCommand {
    /// This executable re-run with [`super::MONITOR_FLAG`], reporting hangs after
    /// [`heartbeat::STALL_AFTER`].
    pub fn current_exe() -> io::Result<Self> {
        Ok(Self {
            program: std::env::current_exe()?,
            args: vec![super::MONITOR_FLAG.into()],
            stall_after: heartbeat::STALL_AFTER,
        })
    }
}

/// Signal handlers borrow this slot without locking; reconnects reclaim old connections.
static CONNECTION: slot::Slot<minidumper::Client> = slot::Slot::new();
/// Where crash reports go: [`CONNECTING`], [`CLAIMED`], [`CONNECTED`] or [`IN_PROCESS`].
static ROUTE: AtomicU8 = AtomicU8::new(CONNECTING);
/// No connection yet: a crash now claims the in-process report.
const CONNECTING: u8 = 0;
/// The connection thread is introducing this client; a crash waits for it.
const CLAIMED: u8 = 1;
/// The monitor reports crashes.
const CONNECTED: u8 = 2;
/// A crash was written in-process, so the monitor must never hear of this client as one that
/// can still crash.
const IN_PROCESS: u8 = 3;
/// How long a crash waits for a connection being introduced or a dump receipt, in monotonic
/// milliseconds; counting pauses instead let coarse or overshooting sleeps stretch it.
const INTRODUCTION_WAIT_MS: u64 = 2_000;
/// Messages at most this long are framed on the stack, as signal handlers need.
const STACK_MESSAGE_BYTES: usize = 64;

/// What a reconnection repeats: the socket, the token and the serialized hello.
struct Session {
    socket: PathBuf,
    token: Token,
    hello: Vec<u8>,
}

static SESSION: OnceLock<Session> = OnceLock::new();
/// The monitor's process id once it started.
static PID: OnceLock<u32> = OnceLock::new();
/// Retains the Windows monitor's identity even after the child registry reaps it.
#[cfg(windows)]
static PROCESS: OnceLock<children::Spawned> = OnceLock::new();
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
/// process until the connection is up. `restart` is how the monitor starts this client again.
pub(super) fn start(command: &MonitorCommand, crash_dir: &Path, hello: &Hello, restart: Restart) {
    if let Err(error) = try_start(command, crash_dir, hello, restart) {
        tracing::warn!("crash monitor unavailable, reports stay in-process: {error}");
        settle(Progress::Unavailable);
    }
}

fn try_start(
    command: &MonitorCommand,
    crash_dir: &Path,
    hello: &Hello,
    restart: Restart,
) -> io::Result<()> {
    let mut token: Token = [0; TOKEN_BYTES];
    getrandom::fill(&mut token).map_err(|error| io::Error::other(error.to_string()))?;
    let pid = std::process::id();
    let nonce = nonce();
    let socket = socket_path(pid, nonce)?;
    let hang = match heartbeat::watch(crash_dir, pid, nonce) {
        Ok(heartbeat) => Some(HangWatch {
            heartbeat,
            stall_after_ms: u64::try_from(command.stall_after.as_millis()).unwrap_or(u64::MAX),
        }),
        Err(error) => {
            tracing::warn!("hang detection unavailable: {error}");
            None
        }
    };
    let mut launch = serde_json::to_vec(&Launch {
        token,
        restart,
        hang: hang.clone(),
        staging: hello.staging.clone(),
    })?;
    launch.push(b'\n');
    let hello = serde_json::to_vec(hello)?;
    let mut monitor = Command::new(&command.program);
    monitor
        .args(&command.args)
        .arg(&socket)
        .arg(pid.to_string())
        .arg(crash_dir)
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::null());
    detach_from_terminal(&mut monitor);
    let spawned = match children::spawn_outliving(&mut monitor) {
        Ok(spawned) => spawned,
        Err(error) => {
            if let Some(hang) = hang {
                let _ = std::fs::remove_file(hang.heartbeat);
            }
            return Err(error);
        }
    };
    let _ = PID.set(spawned.id());
    #[cfg(windows)]
    let spawned = PROCESS.get_or_init(|| spawned);
    let mut stdin = spawned
        .take_stdin()
        .ok_or_else(|| io::Error::other("no stdin"))?;
    stdin.write_all(&launch)?;
    let _ = LIFELINE.set(Mutex::new(stdin));
    let stdout = spawned
        .take_stdout()
        .ok_or_else(|| io::Error::other("no stdout"))?;
    let _ = SESSION.set(Session {
        socket,
        token,
        hello,
    });
    std::thread::Builder::new()
        .name("crash-monitor".into())
        .spawn(move || connect(stdout))?;
    Ok(())
}

/// Waits for the monitor's ready line, connects and introduces this client, then drains the
/// monitor's stdout until it exits so its writes never fail.
fn connect(stdout: ChildStdout) {
    let mut lines = BufReader::new(stdout).lines();
    let ready = lines
        .by_ref()
        .any(|line| line.is_ok_and(|line| line.trim_end() == protocol::READY));
    let connected = if ready {
        introduce()
    } else {
        Err("it exited before listening".to_owned())
    };
    match connected {
        Ok(()) => {
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

/// Connects and says hello, unless a crash already took the in-process report: then the
/// monitor never hears of this client and so reports nothing of its own.
fn introduce() -> Result<(), String> {
    let client = open_connection().map_err(|error| error.to_string())?;
    if ROUTE
        .compare_exchange(CONNECTING, CLAIMED, Ordering::AcqRel, Ordering::Acquire)
        .is_err()
    {
        return Err("a crash was already reported in-process".to_owned());
    }
    if let Some(pid) = PID.get() {
        super::handler::allow_dumps_by(*pid);
    }
    if !say_hello(&client) {
        ROUTE.store(IN_PROCESS, Ordering::Release);
        return Err("the hello was not delivered".to_owned());
    }
    if !CONNECTION.publish(client) {
        return Err("previous crash connection is still in use".to_owned());
    }
    ROUTE.store(CONNECTED, Ordering::Release);
    Ok(())
}

/// Connects again after a dump, which ends the server's side of a connection, so later faults
/// and hangs still reach the monitor; false when that failed.
pub(super) fn reconnect() -> bool {
    let Ok(client) = open_connection() else {
        return false;
    };
    if let Some(pid) = PID.get() {
        super::handler::allow_dumps_by(*pid);
    }
    if !say_hello(&client) {
        return false;
    }
    CONNECTION.publish(client)
}

/// Opens a replacement connection outside native crash handlers.
fn open_connection() -> Result<minidumper::Client, minidumper::Error> {
    let session = SESSION
        .get()
        .ok_or(minidumper::Error::ProtocolError("no crash monitor session"))?;
    minidumper::Client::with_name(protocol::socket_name(&session.socket))
}

/// Reintroduces this session on a newly opened connection.
fn say_hello(client: &minidumper::Client) -> bool {
    SESSION
        .get()
        .is_some_and(|session| send_on(client, protocol::HELLO, &session.hello))
}

fn settle(progress: Progress) {
    *PROGRESS.lock().unwrap_or_else(PoisonError::into_inner) = progress;
    SETTLED.notify_all();
}

/// The connection, once up; lock-free, so crash handlers may call it.
pub(super) fn connection() -> Option<slot::Guard<'static, minidumper::Client>> {
    CONNECTION.read()
}

/// The connection a crash reports through, or `None` when the crash must write its folder
/// in-process. Lock-free and allocation-free, so crash handlers may call it; while the
/// connection is being introduced it waits for it.
pub(super) fn route() -> Option<slot::Guard<'static, minidumper::Client>> {
    if let Some(client) = connection() {
        return Some(client);
    }
    match ROUTE.compare_exchange(CONNECTING, IN_PROCESS, Ordering::AcqRel, Ordering::Acquire) {
        Ok(_) | Err(IN_PROCESS) => return None,
        Err(_) => {}
    }
    let mut client = None;
    wait_bounded(
        || {
            client = connection();
            client.is_some() || ROUTE.load(Ordering::Acquire) == IN_PROCESS
        },
        monotonic_ms,
        introduction_pause,
    );
    client
}

/// Polls `ready` between pauses until it holds or [`INTRODUCTION_WAIT_MS`] pass on `now_ms`;
/// lock- and allocation-free when its arguments are.
fn wait_bounded(
    mut ready: impl FnMut() -> bool,
    mut now_ms: impl FnMut() -> u64,
    mut pause: impl FnMut(),
) -> bool {
    let deadline = now_ms().saturating_add(INTRODUCTION_WAIT_MS);
    loop {
        if ready() {
            return true;
        }
        if now_ms() >= deadline {
            return false;
        }
        pause();
    }
}

/// Monotonic milliseconds, readable from signal handlers; `u64::MAX` ends a wait at once
/// when the clock is unavailable.
#[cfg(unix)]
fn monotonic_ms() -> u64 {
    let mut now = libc::timespec {
        tv_sec: 0,
        tv_nsec: 0,
    };
    // SAFETY: clock_gettime is async-signal-safe and only writes the supplied timespec.
    if unsafe { libc::clock_gettime(libc::CLOCK_MONOTONIC, &mut now) } != 0 {
        return u64::MAX;
    }
    (now.tv_sec as u64)
        .saturating_mul(1000)
        .saturating_add(now.tv_nsec as u64 / 1_000_000)
}

/// Monotonic milliseconds since boot.
#[cfg(not(unix))]
fn monotonic_ms() -> u64 {
    // SAFETY: GetTickCount64 has no preconditions.
    unsafe { windows_sys::Win32::System::SystemInformation::GetTickCount64() }
}

/// Pauses a bounded introduction wait without entering Rust's thread runtime on Unix.
fn introduction_pause() {
    #[cfg(unix)]
    {
        let pause = libc::timespec {
            tv_sec: 0,
            tv_nsec: 1_000_000,
        };
        // SAFETY: nanosleep reads a valid timespec and requires no allocation or locks.
        unsafe {
            libc::nanosleep(&pause, std::ptr::null_mut());
        }
    }
    #[cfg(not(unix))]
    std::thread::sleep(Duration::from_millis(1));
}

/// Announces a macOS dump and bounds its receipt wait using the existing connection budget.
#[cfg(target_os = "macos")]
pub(super) fn announce_dump(client: &minidumper::Client) -> bool {
    static REQUEST: std::sync::atomic::AtomicU64 = std::sync::atomic::AtomicU64::new(1);
    let Some(region) = heartbeat::watched() else {
        return false;
    };
    let request = REQUEST.fetch_add(1, Ordering::Relaxed);
    send_on(client, protocol::DUMP_REQUEST, &request.to_le_bytes())
        && wait_for_dump_receipt(region, request, monotonic_ms, introduction_pause)
}

/// Waits without locks or socket reads; an unresponsive monitor consumes only the wait budget.
#[cfg(any(target_os = "macos", test))]
fn wait_for_dump_receipt(
    region: &heartbeat::Region,
    request: u64,
    now_ms: impl FnMut() -> u64,
    pause: impl FnMut(),
) -> bool {
    wait_bounded(|| region.dump_acknowledged(request), now_ms, pause)
}

/// Sends one message; false when there is no connection or the send failed.
pub(super) fn send(kind: u32, payload: &[u8]) -> bool {
    connection().is_some_and(|client| send_on(&client, kind, payload))
}

/// Sends `payload` behind the launch token on `client`. Payloads of at most
/// [`STACK_MESSAGE_BYTES`] allocate nothing, so signal handlers may send them.
pub(super) fn send_on(client: &minidumper::Client, kind: u32, payload: &[u8]) -> bool {
    let Some(session) = SESSION.get() else {
        return false;
    };
    let length = TOKEN_BYTES + payload.len();
    if payload.len() <= STACK_MESSAGE_BYTES {
        let mut message = [0_u8; TOKEN_BYTES + STACK_MESSAGE_BYTES];
        message[..TOKEN_BYTES].copy_from_slice(&session.token);
        message[TOKEN_BYTES..length].copy_from_slice(payload);
        client.send_message(kind, &message[..length]).is_ok()
    } else {
        let mut message = Vec::with_capacity(length);
        message.extend_from_slice(&session.token);
        message.extend_from_slice(payload);
        client.send_message(kind, message).is_ok()
    }
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

/// Duplicates the retained Windows monitor handle, including when the monitor already exited.
#[cfg(windows)]
pub(crate) fn process_handle() -> io::Result<Option<std::os::windows::io::OwnedHandle>> {
    PROCESS
        .get()
        .map(children::Spawned::process_handle)
        .transpose()
}

/// The socket the monitor listens on, once started.
pub fn socket() -> Option<&'static Path> {
    SESSION.get().map(|session| session.socket.as_path())
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

/// A per-launch value that keeps socket names from colliding with a stale socket.
fn nonce() -> u64 {
    let mut bytes = [0_u8; 8];
    if getrandom::fill(&mut bytes).is_err() {
        return u64::from(std::process::id());
    }
    u64::from_le_bytes(bytes)
}

fn socket_file(pid: u32, nonce: u64) -> String {
    format!("crash-{pid}-{nonce:016x}.sock")
}

/// A fresh socket path for a client with `pid` in this user's temporary directory, which
/// Windows keeps private to the user.
#[cfg(windows)]
fn socket_path(pid: u32, nonce: u64) -> io::Result<PathBuf> {
    Ok(std::env::temp_dir().join(socket_file(pid, nonce)))
}

/// A fresh socket path for a client with `pid`, in the first directory that only this user can
/// enter and that keeps the path within `sun_path`: `$XDG_RUNTIME_DIR/cinnabar`, else
/// `cinnabar-<uid>` in the temporary directory or `/tmp`.
#[cfg(unix)]
fn socket_path(pid: u32, nonce: u64) -> io::Result<PathBuf> {
    // SAFETY: geteuid has no preconditions.
    let user = format!("cinnabar-{}", unsafe { libc::geteuid() });
    let runtime = std::env::var_os("XDG_RUNTIME_DIR")
        .filter(|dir| !dir.is_empty())
        .map(|dir| PathBuf::from(dir).join("cinnabar"));
    let candidates = runtime.into_iter().chain([
        std::env::temp_dir().join(&user),
        Path::new("/tmp").join(&user),
    ]);
    let file = socket_file(pid, nonce);
    let mut last = io::Error::other("no private directory for the crash monitor socket");
    for dir in candidates {
        let path = dir.join(&file);
        if path.as_os_str().len() >= unix::sun_path_capacity() {
            last = io::Error::other(format!("{} is too long for a socket", path.display()));
            continue;
        }
        match unix::private_dir(&dir) {
            Ok(()) => return Ok(path),
            Err(error) => last = error,
        }
    }
    Err(last)
}

#[cfg(unix)]
mod unix {
    use std::{io, path::Path};

    /// Bytes in `sockaddr_un::sun_path`, its terminating NUL included.
    pub(super) fn sun_path_capacity() -> usize {
        // SAFETY: `sockaddr_un` is plain data, for which all zeroes is valid.
        let address: libc::sockaddr_un = unsafe { std::mem::zeroed() };
        address.sun_path.len()
    }

    /// Creates `dir` for this user only, or accepts an existing one that is: a real directory
    /// this user owns, with no group or other access.
    pub(super) fn private_dir(dir: &Path) -> io::Result<()> {
        use std::os::unix::fs::{DirBuilderExt, MetadataExt};

        match std::fs::DirBuilder::new().mode(0o700).create(dir) {
            Ok(()) => {}
            Err(error) if error.kind() == io::ErrorKind::AlreadyExists => {}
            Err(error) => return Err(error),
        }
        let metadata = std::fs::symlink_metadata(dir)?;
        // SAFETY: geteuid has no preconditions.
        let user = unsafe { libc::geteuid() };
        if !metadata.is_dir() || metadata.uid() != user || metadata.mode() & 0o077 != 0 {
            return Err(io::Error::other(format!(
                "{} is not private to this user",
                dir.display()
            )));
        }
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A monitor that never acknowledges holds the caller only for the wait budget, however
    /// long each pause overshoots.
    #[test]
    fn nonresponsive_dump_monitor_exhausts_the_existing_wait_budget() {
        let directory = tempfile::tempdir().unwrap();
        let heartbeat = heartbeat::Heartbeat::create(&directory.path().join("heartbeat")).unwrap();
        for pause_ms in [1, 7, 500] {
            let now = std::cell::Cell::new(0);
            let mut pauses = 0;
            assert!(!wait_for_dump_receipt(
                heartbeat.region(),
                1,
                || now.get(),
                || {
                    pauses += 1;
                    now.set(now.get() + pause_ms);
                }
            ));
            assert_eq!(pauses, INTRODUCTION_WAIT_MS.div_ceil(pause_ms));
        }
    }

    /// A late receipt for an older dump cannot release a new request's ordering barrier.
    #[test]
    fn dump_receipt_matches_the_current_request() {
        let directory = tempfile::tempdir().unwrap();
        let path = directory.path().join("heartbeat");
        let client = heartbeat::Heartbeat::create(&path).unwrap();
        let monitor = heartbeat::Heartbeat::open(&path).unwrap();
        monitor.region().acknowledge_dump(1);
        let mut pauses = 0;
        assert!(wait_for_dump_receipt(
            client.region(),
            2,
            || 0,
            || {
                pauses += 1;
                monitor.region().acknowledge_dump(2);
            }
        ));
        assert_eq!(pauses, 1);
    }

    #[test]
    fn socket_paths_are_unique_per_launch() {
        let first = socket_path(1, 1).unwrap();
        assert_ne!(first, socket_path(1, 2).unwrap());
        assert!(
            first
                .file_name()
                .unwrap()
                .to_string_lossy()
                .starts_with("crash-1-")
        );
    }

    #[cfg(unix)]
    #[test]
    fn socket_directories_must_be_private_to_this_user() {
        use std::os::unix::fs::PermissionsExt;

        let root = tempfile::tempdir().unwrap();
        let created = root.path().join("created");
        unix::private_dir(&created).unwrap();
        let mode = std::fs::metadata(&created).unwrap().permissions().mode();
        assert_eq!(mode & 0o777, 0o700);
        let shared = root.path().join("shared");
        std::fs::create_dir(&shared).unwrap();
        std::fs::set_permissions(&shared, std::fs::Permissions::from_mode(0o755)).unwrap();
        assert!(unix::private_dir(&shared).is_err());
        let link = root.path().join("link");
        std::os::unix::fs::symlink(&created, &link).unwrap();
        assert!(unix::private_dir(&link).is_err());
    }

    #[cfg(unix)]
    #[test]
    fn socket_paths_fit_in_sun_path() {
        let path = socket_path(u32::MAX, u64::MAX).unwrap();
        assert!(path.as_os_str().len() < unix::sun_path_capacity());
    }
}
