//! Messages between a client and its crash monitor, over the `minidumper` connection.
//!
//! The client starts the monitor with one [`Launch`] line on its stdin, which only the client
//! holds: the token every message starts with, and how to start the client again. The monitor
//! prints [`READY`] on stdout once it listens; the client then connects, sends [`HELLO`] and its
//! current [`STATE`], and later [`STATE`] on every change. Every dump request follows a
//! [`DUMP_REQUEST`], and a [`PANIC`] before that when a panic caused it. The server drops a
//! connection once it has dumped it, so a client that survives its panic connects again and
//! repeats its [`HELLO`]. A client exiting on purpose sends [`EXIT`], after [`STARTUP_FAILURE`] or
//! [`RUNTIME_FAILURE`] when `main` failed.
//!
//! `minidumper` does not say which connection a message came from, so the token is what tells
//! the client's messages from anyone else's.

use std::{collections::BTreeMap, ffi::OsString, path::PathBuf};

use crash_report::report::{Identity, Report, Thread};
use serde::{Deserialize, Serialize};

/// The line the monitor prints once its server listens.
pub const READY: &str = "CINNABAR_CRASH_MONITOR_READY";

/// [`Hello`], after each connection.
pub const HELLO: u32 = 1;
/// [`State`], whenever it changes.
pub const STATE: u32 = 2;
/// [`Panic`], just before the panicking thread's dump request.
pub const PANIC: u32 = 3;
/// The exit code a client is about to exit with, as little-endian `i32`; the exit is deliberate
/// and needs no report.
pub const EXIT: u32 = 4;
/// UTF-8 text of the error `main` failed with before the game started.
pub const STARTUP_FAILURE: u32 = 5;
/// Before every dump request: a [`DumpContext`] when [`Hello::full_dump`] is set on Windows,
/// a little-endian `u64` receipt identity on macOS, else empty. The monitor keeps only dumps
/// announced this way.
pub const DUMP_REQUEST: u32 = 6;
/// UTF-8 text of the error `main` failed with after the game started.
pub const RUNTIME_FAILURE: u32 = 7;

/// The [`EXIT`] code the `atexit` hook announces: `main` returned or something called `exit`
/// without announcing a code of its own, as the runtime does after a panic on the main thread.
pub const AT_EXIT: i32 = i32::MIN;
/// The runtime status of an unhandled Rust panic, also used when teardown is cut short.
pub const PANIC_EXIT_CODE: u8 = 101;
/// The [`EXIT`] code of a client that wrote its crash folder itself before the monitor
/// connected, so the monitor must not add another.
pub const SELF_REPORTED: i32 = i32::MIN + 1;

/// Bytes of the per-launch secret every message starts with.
pub const TOKEN_BYTES: usize = 16;
/// The per-launch secret every message starts with.
pub type Token = [u8; TOKEN_BYTES];

/// The payload of a `message` that starts with `token`; `None` for anyone else's.
pub fn open<'a>(token: &Token, message: &'a [u8]) -> Option<&'a [u8]> {
    message.strip_prefix(token.as_slice())
}

/// What the client tells the monitor on its stdin before anything else, as one JSON line.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct Launch {
    pub token: Token,
    /// Relaunches the client as the player started it.
    pub restart: Restart,
    /// The client's main-loop progress, when it could share it.
    pub hang: Option<HangWatch>,
    /// Staging protected by the monitor before it announces readiness, through report writes.
    #[serde(default)]
    pub staging: Option<PathBuf>,
}

/// Where the client publishes main-loop progress and how long a stall must last to be a hang;
/// see [`super::heartbeat`].
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct HangWatch {
    /// The heartbeat file, which the monitor removes once the client is gone.
    pub heartbeat: PathBuf,
    /// Milliseconds without progress, outside loading, that make a hang.
    pub stall_after_ms: u64,
}

/// Who the client is and where its logs and crash staging directory are.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct Hello {
    pub identity: Identity,
    pub client_log: PathBuf,
    pub core_log: PathBuf,
    /// Dumps should hold all process memory.
    pub full_dump: bool,
    /// The crash staging directory whose rings and samples reports attach; `None` without one.
    pub staging: Option<PathBuf>,
}

/// How to start the client again; the monitor runs its own executable, the client's.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct Restart {
    /// The client's arguments, without the program.
    pub args: Vec<OsString>,
    /// Variables for this launch only, removed from the restarted client's environment.
    pub env_remove: Vec<String>,
}

/// Why `main` failed.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum Stage {
    /// Before the game showed its first frames.
    Startup,
    /// While the game was running.
    Runtime,
}

impl Stage {
    /// The message that carries a failure at this stage.
    pub const fn message(self) -> u32 {
        match self {
            Self::Startup => STARTUP_FAILURE,
            Self::Runtime => RUNTIME_FAILURE,
        }
    }
}

/// What the client was doing; see [`super::state`].
#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
pub struct State {
    /// The renderer attempt, as `report.json` records it.
    pub renderer: Option<serde_json::Value>,
    pub gpu: Option<String>,
    pub driver: Option<String>,
    pub game_state: BTreeMap<String, String>,
}

impl State {
    /// Records this state in `report`.
    pub fn apply_to(&self, report: &mut Report) {
        report.renderer.clone_from(&self.renderer);
        report.gpu.clone_from(&self.gpu);
        report.driver.clone_from(&self.driver);
        report.game_state.clone_from(&self.game_state);
    }
}

/// The panic a dump request belongs to.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct Panic {
    pub message: String,
    pub thread: Thread,
    pub backtrace: String,
    pub uptime_ms: u64,
}

/// The faulting context of a Windows dump request, so the monitor can write the full-memory
/// dump `minidumper` does not offer.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct DumpContext {
    /// The address of the client's `EXCEPTION_POINTERS`.
    pub exception_pointers: u64,
    pub process_id: u32,
    pub thread_id: u32,
    pub exception_code: i32,
}

impl DumpContext {
    pub const BYTES: usize = 20;

    pub fn to_bytes(self) -> [u8; Self::BYTES] {
        let mut bytes = [0; Self::BYTES];
        bytes[..8].copy_from_slice(&self.exception_pointers.to_le_bytes());
        bytes[8..12].copy_from_slice(&self.process_id.to_le_bytes());
        bytes[12..16].copy_from_slice(&self.thread_id.to_le_bytes());
        bytes[16..].copy_from_slice(&self.exception_code.to_le_bytes());
        bytes
    }

    pub fn from_bytes(bytes: &[u8]) -> Option<Self> {
        let bytes: &[u8; Self::BYTES] = bytes.try_into().ok()?;
        Some(Self {
            exception_pointers: u64::from_le_bytes(bytes[..8].try_into().ok()?),
            process_id: u32::from_le_bytes(bytes[8..12].try_into().ok()?),
            thread_id: u32::from_le_bytes(bytes[12..16].try_into().ok()?),
            exception_code: i32::from_le_bytes(bytes[16..].try_into().ok()?),
        })
    }
}

/// The `minidumper` name of the socket file at `path`, which the monitor's command line names.
/// iOS runs no monitor, so it has no socket to name.
#[cfg(not(target_os = "ios"))]
pub fn socket_name(path: &std::path::Path) -> minidumper::SocketName<'_> {
    minidumper::SocketName::Path(path)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn dump_context_round_trips() {
        let context = DumpContext {
            exception_pointers: 0x0123_4567_89ab_cdef,
            process_id: 7,
            thread_id: 9,
            exception_code: 0xC000_0005_u32 as i32,
        };
        assert_eq!(DumpContext::from_bytes(&context.to_bytes()), Some(context));
        assert_eq!(DumpContext::from_bytes(&[0; 3]), None);
    }

    #[test]
    fn only_messages_that_start_with_the_token_open() {
        let token = [7; TOKEN_BYTES];
        let mut message = token.to_vec();
        message.extend_from_slice(b"payload");
        assert_eq!(open(&token, &message), Some(b"payload".as_slice()));
        assert_eq!(open(&token, &token), Some([].as_slice()));
        assert_eq!(open(&[8; TOKEN_BYTES], &message), None);
        assert_eq!(open(&token, b"payload"), None);
    }
}
