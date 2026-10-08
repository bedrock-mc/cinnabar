//! Bounded process protocol. OS-restricted production launch deliberately fails closed.

#[cfg(feature = "execution")]
use crate::server::BundleHost;
use anyhow::{Result, bail, ensure};
use serde::{Deserialize, Serialize};
use server_experience::{
    crypto,
    policy::*,
    runtime::{Capabilities, Principal, Transaction},
    screen::{self, GuiSize},
};
use std::{
    io::{Read, Write},
    path::Path,
    sync::{Mutex, mpsc},
    time::{Duration, Instant},
};

mod supervisor;

const MAX_STARTUP_IPC: usize = MAX_COMPONENT_BYTES * 2 + MAX_HOST_OUTPUT;
// Decimal byte encoding needs up to four bytes per payload byte, plus bounded metadata.
const MAX_DISPATCH_IPC: usize = MAX_MESSAGE_BYTES * 4 + MAX_HOST_OUTPUT;
/// A reply: one callback's output, or a failure, with room for the reply's own framing.
const MAX_REPLY_IPC: usize = MAX_HOST_OUTPUT + 2 * MAX_FAILURE_REASON_BYTES;
const HELPER_DEADLINE: Duration = Duration::from_secs(10);

/// The helper IPC protocol. A client and its helper ship together; a helper of another version
/// refuses the start. 2 added `protocol` to the start and replies that are a failure instead of
/// the helper exiting.
const HELPER_PROTOCOL: u32 = 2;
/// UTF-8 bytes of a failure's reason, its error chain and guest backtrace; the rest is cut off
/// at a char boundary.
pub const MAX_FAILURE_REASON_BYTES: usize = 2048;
/// Bytes of one helper stderr line that reach the client's log; the rest of the line is dropped.
pub const MAX_LOG_LINE_BYTES: usize = 512;
/// Helper stderr lines per second that reach the client's log; the others are dropped and
/// counted.
pub const MAX_LOG_LINES_PER_SECOND: usize = 32;

/// Locates the helper beside the profile-selected client executable.
pub fn developer_executable(client: &Path) -> std::path::PathBuf {
    client.with_file_name(if cfg!(windows) {
        "mod-host.exe"
    } else {
        "mod-host"
    })
}

/// Locates the media decoder helper beside the client executable.
pub fn media_executable(client: &Path) -> std::path::PathBuf {
    client.with_file_name(if cfg!(windows) {
        "cinnabar-media-helper.exe"
    } else {
        "cinnabar-media-helper"
    })
}

/// Checks the developer launcher before offering component execution.
pub fn developer_runtime_available(client: &Path) -> bool {
    let Ok(metadata) = std::fs::metadata(developer_executable(client)) else {
        return false;
    };
    if !metadata.is_file() {
        return false;
    }
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        metadata.permissions().mode() & 0o111 != 0
    }
    #[cfg(not(unix))]
    {
        true
    }
}

#[derive(Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
struct Start {
    protocol: u32,
    owner: Principal,
    capabilities: Capabilities,
    epoch: u64,
    component: String,
}

/// What one callback delivers to the guest.
#[derive(Clone, Debug, Deserialize, Serialize, PartialEq)]
#[serde(tag = "kind", rename_all = "snake_case", deny_unknown_fields)]
pub enum Event {
    /// A validated channel record, to `dispatch`.
    Message { channel: String, record: Vec<u8> },
    /// A declared action fired from the focused modal, to `action`, with its collection row.
    Action { id: String, index: Option<u32> },
    /// The world epoch changed, to `epoch`; the guest and its state live on.
    Epoch,
    /// A secondary press fired a declared action from the focused modal, to `secondary-action`,
    /// with its collection row.
    SecondaryAction { id: String, index: Option<u32> },
    /// The open modal was first drawn at, or resized to, `size`, to `modal-resized`.
    Resized { size: GuiSize },
    /// The text of the modal's edit box `control`, a declared action, changed while the modal had
    /// focus, to `text-changed`.
    Text { control: String, text: String },
}

impl Event {
    /// Bounds an event before it crosses the process boundary or enters the guest.
    pub fn check(&self) -> Result<()> {
        let valid = match self {
            Event::Message { channel, record } => {
                server_experience::manifest::identifier(channel)
                    && record.len() <= MAX_MESSAGE_BYTES
            }
            Event::Action { id, .. } | Event::SecondaryAction { id, .. } => {
                server_experience::manifest::identifier(id)
            }
            Event::Epoch => true,
            Event::Resized { size } => size.valid(),
            Event::Text { control, text } => {
                server_experience::manifest::identifier(control) && screen::edit_text(text)
            }
        };
        ensure!(valid, "helper event too large or malformed");
        Ok(())
    }
}

impl Event {
    /// The guest export that receives this event.
    pub fn callback(&self) -> &'static str {
        match self {
            Event::Message { .. } => "dispatch",
            Event::Action { .. } => "action",
            Event::Epoch => "epoch",
            Event::SecondaryAction { .. } => "secondary-action",
            Event::Resized { .. } => "modal-resized",
            Event::Text { .. } => "text-changed",
        }
    }
}

/// The helper's answer to its start and to each dispatch.
#[derive(Debug, Deserialize, Serialize)]
#[serde(rename_all = "snake_case", deny_unknown_fields)]
pub enum Reply {
    /// The callback returned; its output, to publish whole, and the fuel it consumed of its
    /// `CALLBACK_FUEL`.
    Committed { transaction: Transaction, fuel: u64 },
    /// The callback failed and published nothing. After a failed dispatch the helper runs on, on
    /// a fresh instance of the guest; after a failed start it exits.
    Failed(CallFailure),
}

/// Why one callback, or the start, failed.
#[derive(Clone, Copy, Debug, Deserialize, Serialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum FailureKind {
    /// The callback ran out of fuel.
    Fuel,
    /// The guest panicked, which a Rust guest's `unreachable` trap is.
    Panic,
    /// Any other trap.
    Trap,
    /// The host refused the callback or something it did, such as an undeclared action or too
    /// much output.
    Refused,
    /// The component could not be compiled, linked or initialized.
    Startup,
}

/// A failed callback: its bundle, the guest export it called, the kind of failure and a bounded
/// reason with no control characters but line breaks.
#[derive(Clone, Debug, Deserialize, Serialize, PartialEq)]
#[serde(deny_unknown_fields)]
pub struct CallFailure {
    pub bundle: String,
    pub callback: String,
    pub kind: FailureKind,
    pub reason: String,
    /// The fuel the callback consumed before it failed, of its `CALLBACK_FUEL`; none for a
    /// failed start.
    pub fuel: Option<u64>,
}

impl CallFailure {
    /// The failure of `bundle`'s `callback` with `error`: its error chain, which carries the
    /// guest backtrace of a trap.
    #[cfg(feature = "execution")]
    pub(crate) fn of(bundle: &str, callback: &str, error: &anyhow::Error) -> Self {
        use wasmtime::Trap;
        let kind = match error.downcast_ref::<Trap>() {
            Some(Trap::OutOfFuel) => FailureKind::Fuel,
            Some(Trap::UnreachableCodeReached) => FailureKind::Panic,
            Some(_) => FailureKind::Trap,
            None => FailureKind::Refused,
        };
        Self::new(bundle, callback, kind, error)
    }

    #[cfg(feature = "execution")]
    /// A start of `bundle` that failed with `error`.
    fn startup(bundle: &str, error: &anyhow::Error) -> Self {
        Self::new(bundle, "init", FailureKind::Startup, error)
    }

    #[cfg(feature = "execution")]
    fn new(bundle: &str, callback: &str, kind: FailureKind, error: &anyhow::Error) -> Self {
        let mut reason: String = format!("{error:#}")
            .chars()
            .filter(|c| !c.is_control() || *c == '\n')
            .collect();
        reason.truncate(reason.floor_char_boundary(MAX_FAILURE_REASON_BYTES));
        Self {
            bundle: bundle.to_owned(),
            callback: callback.to_owned(),
            kind,
            reason,
            fuel: None,
        }
    }
}

#[derive(Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub struct Dispatch {
    pub event: Event,
    /// The world epoch the callback's output must carry.
    pub epoch: u64,
    /// The bundle's open modal's size, which `ui.modal-size` returns; none while it is closed.
    pub gui: Option<GuiSize>,
}

pub struct Helper {
    process: supervisor::Process,
    requests: mpsc::SyncSender<Dispatch>,
    responses: Mutex<mpsc::Receiver<Result<Reply>>>,
    /// The helper's stderr lines, cut and rate-limited.
    log: Mutex<mpsc::Receiver<String>>,
    pending_since: Option<Instant>,
    quarantined: bool,
}

impl Helper {
    /// Refuses production remote code until platform restrictions are implemented and verified.
    pub fn spawn_restricted(
        _executable: &Path,
        _bytes: &[u8],
        _owner: Principal,
        _capabilities: Capabilities,
        _epoch: u64,
    ) -> Result<Self> {
        bail!("restricted server helpers are unavailable on this build")
    }

    /// Returns a pending developer helper; its supervisor prepares and launches the process.
    pub fn spawn_developer(
        executable: &Path,
        bytes: Vec<u8>,
        owner: Principal,
        capabilities: Capabilities,
        epoch: u64,
    ) -> Result<Self> {
        ensure!(
            std::env::var(DEVELOPER_ENV).as_deref() == Ok("1"),
            "developer helper disabled"
        );
        ensure!(bytes.len() <= MAX_COMPONENT_BYTES, "component too large");
        Self::spawn_pending(executable, move || {
            Ok(Start {
                protocol: HELPER_PROTOCOL,
                owner,
                capabilities,
                epoch,
                component: crypto::hex(&bytes),
            })
        })
    }

    /// Transfers startup work to supervision and begins the deadline before returning.
    fn spawn_pending(
        executable: &Path,
        prepare: impl FnOnce() -> Result<Start> + Send + 'static,
    ) -> Result<Self> {
        let pending_since = Some(Instant::now());
        let (requests, receiver) = mpsc::sync_channel(1);
        let (sender, responses) = mpsc::sync_channel(1);
        let (lines, log) = mpsc::sync_channel(4 * MAX_LOG_LINES_PER_SECOND);
        let process =
            supervisor::Process::start(executable.to_owned(), prepare, receiver, sender, lines)?;
        Ok(Self {
            process,
            requests,
            responses: Mutex::new(responses),
            log: Mutex::new(log),
            pending_since,
            quarantined: false,
        })
    }

    /// Sends one callback without ever waiting for the child from the render thread.
    pub fn dispatch(&mut self, request: Dispatch) -> Result<()> {
        ensure!(
            !self.quarantined && self.pending_since.is_none(),
            "helper busy or quarantined"
        );
        request.event.check()?;
        serialize_frame(&request, MAX_DISPATCH_IPC)?;
        self.requests.try_send(request)?;
        self.pending_since = Some(Instant::now());
        Ok(())
    }

    /// Polls completed output and kills a stalled compiler or guest after its deadline. A failed
    /// callback is an `Ok` failure reply and the helper runs on; an `Err` means the helper is
    /// gone.
    pub fn poll(&mut self) -> Option<Result<Reply>> {
        if self.quarantined {
            return None;
        }
        if self
            .pending_since
            .is_some_and(|since| since.elapsed() >= HELPER_DEADLINE)
        {
            self.kill();
            return Some(Err(anyhow::anyhow!("helper deadline exceeded")));
        }
        let response = self.responses.lock().ok()?.try_recv();
        match response {
            Ok(result) => {
                self.pending_since = None;
                if result.is_err() {
                    self.kill();
                }
                Some(result)
            }
            Err(mpsc::TryRecvError::Empty) => None,
            Err(mpsc::TryRecvError::Disconnected) => {
                self.kill();
                Some(Err(anyhow::anyhow!("helper exited")))
            }
        }
    }

    /// The helper's stderr lines since the last call, for the client's log.
    pub fn drain_log(&mut self) -> Vec<String> {
        self.log
            .get_mut()
            .map(|log| log.try_iter().collect())
            .unwrap_or_default()
    }

    /// Revokes this process immediately; no automatic restart is allowed.
    fn kill(&mut self) {
        self.quarantined = true;
        self.process.cancel();
    }
}

impl Drop for Helper {
    /// Ends guest execution before deferring process reaping off the main thread.
    fn drop(&mut self) {
        self.kill();
        self.process.reap();
    }
}

/// Runs only as the private helper entry point; there are no inherited game handles.
#[cfg(feature = "execution")]
pub fn serve_developer() -> Result<()> {
    ensure!(
        std::env::var(DEVELOPER_ENV).as_deref() == Ok("1"),
        "developer helper disabled"
    );
    let mut input = std::io::stdin().lock();
    let mut output = std::io::stdout().lock();
    let startup: Start = read_frame(&mut input, MAX_STARTUP_IPC)?;
    let bundle = startup.owner.bundle.clone();
    let started = (|| {
        ensure!(
            startup.protocol == HELPER_PROTOCOL,
            "the client speaks helper protocol {}, this helper {HELPER_PROTOCOL}",
            startup.protocol
        );
        ensure!(
            startup.component.len() <= MAX_COMPONENT_BYTES * 2,
            "component too large"
        );
        BundleHost::instantiate(
            &crypto::unhex(&startup.component)?,
            startup.owner,
            startup.capabilities,
            startup.epoch,
        )
    })();
    let mut host = match started {
        Ok(host) => host,
        Err(error) => {
            let failure = CallFailure::startup(&bundle, &error);
            write_frame(&mut output, &Reply::Failed(failure), MAX_REPLY_IPC)?;
            return Err(error);
        }
    };
    let init = Reply::Committed {
        transaction: host.take_transaction(),
        fuel: host.last_fuel_used(),
    };
    answer(&mut output, init, &bundle, "init")?;
    loop {
        let request: Dispatch = read_frame(&mut input, MAX_DISPATCH_IPC)?;
        let callback = request.event.callback();
        host.set_modal_size(request.gui);
        let reply = match host.dispatch(&request.event, request.epoch) {
            Ok(transaction) => Reply::Committed {
                transaction,
                fuel: host.last_fuel_used(),
            },
            Err(error) => Reply::Failed(CallFailure {
                fuel: Some(host.last_fuel_used()),
                ..CallFailure::of(&bundle, callback, &error)
            }),
        };
        answer(&mut output, reply, &bundle, callback)?;
    }
}

#[cfg(feature = "execution")]
/// Writes `reply`, or a failure in its place when it does not fit a reply frame.
fn answer(output: &mut impl Write, reply: Reply, bundle: &str, callback: &str) -> Result<()> {
    let bytes = match serialize_frame(&reply, MAX_REPLY_IPC) {
        Ok(bytes) => bytes,
        Err(error) => {
            let failure = CallFailure::new(bundle, callback, FailureKind::Refused, &error);
            serialize_frame(&Reply::Failed(failure), MAX_REPLY_IPC)?
        }
    };
    output.write_all(&(bytes.len() as u32).to_le_bytes())?;
    output.write_all(&bytes)?;
    output.flush()?;
    Ok(())
}

/// Checks an IPC length before allocating or deserializing its payload.
pub(crate) fn read_frame<T: serde::de::DeserializeOwned>(
    reader: &mut impl Read,
    limit: usize,
) -> Result<T> {
    let mut header = [0; 4];
    reader.read_exact(&mut header)?;
    let len = u32::from_le_bytes(header) as usize;
    ensure!(len <= limit, "IPC frame too large");
    let mut bytes = vec![0; len];
    reader.read_exact(&mut bytes)?;
    Ok(serde_json::from_slice(&bytes)?)
}

/// Sends one length-delimited transaction without ambient handles or paths.
pub(crate) fn write_frame(
    writer: &mut impl Write,
    value: &impl Serialize,
    limit: usize,
) -> Result<()> {
    let bytes = serialize_frame(value, limit)?;
    writer.write_all(&(bytes.len() as u32).to_le_bytes())?;
    writer.write_all(&bytes)?;
    writer.flush()?;
    Ok(())
}

/// Stops serialization as soon as another byte would exceed the frame budget.
fn serialize_frame(value: &impl Serialize, limit: usize) -> Result<Vec<u8>> {
    let mut writer = BoundedBytes {
        bytes: Vec::new(),
        limit,
    };
    serde_json::to_writer(&mut writer, value)?;
    Ok(writer.bytes)
}

struct BoundedBytes {
    bytes: Vec<u8>,
    limit: usize,
}
impl Write for BoundedBytes {
    fn write(&mut self, bytes: &[u8]) -> std::io::Result<usize> {
        if bytes.len() > self.limit.saturating_sub(self.bytes.len()) {
            return Err(std::io::Error::other("IPC frame too large"));
        }
        self.bytes.extend_from_slice(bytes);
        Ok(bytes.len())
    }
    fn flush(&mut self) -> std::io::Result<()> {
        Ok(())
    }
}

#[cfg(test)]
mod tests;

#[cfg(test)]
mod availability_tests {
    use super::*;

    #[test]
    fn developer_launch_requires_an_executable_sibling() {
        let directory = tempfile::tempdir().unwrap();
        let client = directory.path().join("bedrock-client");
        let helper = developer_executable(&client);
        assert_eq!(helper.parent(), client.parent());
        assert!(!developer_runtime_available(&client));
        std::fs::write(&helper, []).unwrap();
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            std::fs::set_permissions(&helper, std::fs::Permissions::from_mode(0o644)).unwrap();
            assert!(!developer_runtime_available(&client));
            std::fs::set_permissions(&helper, std::fs::Permissions::from_mode(0o755)).unwrap();
        }
        assert!(developer_runtime_available(&client));
    }
}
