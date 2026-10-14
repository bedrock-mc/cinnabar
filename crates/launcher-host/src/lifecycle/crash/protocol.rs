//! Messages between a client and its crash monitor, over the `minidumper` connection.
//!
//! The monitor prints [`READY`] on stdout once it listens; the client then connects, sends
//! [`HELLO`] and its current [`STATE`], and later [`STATE`] on every change. Before a dump
//! request the client sends [`PANIC`] when a panic caused it. A client exiting on purpose
//! sends [`EXIT`], after [`STARTUP_FAILURE`] when `main` failed before the game started.

use std::{collections::BTreeMap, ffi::OsString, path::PathBuf};

use crash_report::report::{Identity, Report, Thread};
use serde::{Deserialize, Serialize};

/// The line the monitor prints once its server listens.
pub const READY: &str = "CINNABAR_CRASH_MONITOR_READY";

/// [`Hello`], once after connecting.
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
/// [`DumpContext`], before a native dump request when [`Hello::full_dump`] is set.
pub const DUMP_CONTEXT: u32 = 6;

/// Who the client is, how to start it again and where its logs are.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct Hello {
    pub identity: Identity,
    pub client_log: PathBuf,
    pub core_log: PathBuf,
    /// Relaunches the client as the player started it.
    pub restart: Restart,
    /// Dumps should hold all process memory.
    pub full_dump: bool,
}

/// A command line that starts the client again.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct Restart {
    pub program: PathBuf,
    pub args: Vec<OsString>,
    /// Variables for this launch only, removed from the restarted client's environment.
    pub env_remove: Vec<String>,
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

/// The socket name for an IPC name passed on the monitor's command line: an abstract socket on
/// Linux and Android, a socket file elsewhere.
pub fn socket_name(name: &str) -> minidumper::SocketName<'_> {
    #[cfg(any(target_os = "linux", target_os = "android"))]
    {
        minidumper::SocketName::Abstract(name)
    }
    #[cfg(not(any(target_os = "linux", target_os = "android")))]
    {
        minidumper::SocketName::Path(std::path::Path::new(name))
    }
}

/// A fresh IPC name for a client with `pid`, unique per launch.
pub fn ipc_name(pid: u32, nonce: u64) -> String {
    let name = format!("cinnabar-crash-{pid}-{nonce:016x}");
    #[cfg(any(target_os = "linux", target_os = "android"))]
    {
        name
    }
    #[cfg(not(any(target_os = "linux", target_os = "android")))]
    {
        std::env::temp_dir()
            .join(format!("{name}.sock"))
            .to_string_lossy()
            .into_owned()
    }
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
    fn ipc_names_are_unique_per_launch() {
        assert_ne!(ipc_name(1, 1), ipc_name(1, 2));
        assert!(ipc_name(1, 1).contains("cinnabar-crash-1-"));
    }
}
