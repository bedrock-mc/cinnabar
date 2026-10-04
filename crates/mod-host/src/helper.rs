//! Bounded process protocol. OS-restricted production launch deliberately fails closed.

#[cfg(feature = "execution")]
use crate::server::BundleHost;
use anyhow::{Result, bail, ensure};
use serde::{Deserialize, Serialize};
use server_experience::{
    crypto,
    policy::*,
    runtime::{Capabilities, Principal, Transaction},
};
use std::{
    collections::BTreeSet,
    io::{Read, Write},
    path::Path,
    sync::{Mutex, mpsc},
    time::{Duration, Instant},
};

mod supervisor;

const MAX_STARTUP_IPC: usize = MAX_COMPONENT_BYTES * 2 + MAX_HOST_OUTPUT;
// Decimal byte encoding needs up to four bytes per payload byte, plus bounded metadata.
const MAX_DISPATCH_IPC: usize = MAX_PAYLOAD_BYTES * 4 + MAX_HOST_OUTPUT;
const HELPER_DEADLINE: Duration = Duration::from_secs(10);

/// Locates the helper beside the profile-selected client executable.
pub fn developer_executable(client: &Path) -> std::path::PathBuf {
    client.with_file_name(if cfg!(windows) {
        "mod-host.exe"
    } else {
        "mod-host"
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
    owner: Principal,
    capabilities: Capabilities,
    epoch: u64,
    component: String,
}

#[derive(Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub struct Dispatch {
    pub channel: String,
    pub record: Vec<u8>,
    pub actions: BTreeSet<String>,
    pub epoch: u64,
}

pub struct Helper {
    process: supervisor::Process,
    requests: mpsc::SyncSender<Dispatch>,
    responses: Mutex<mpsc::Receiver<Result<Transaction>>>,
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
        let process = supervisor::Process::start(executable.to_owned(), prepare, receiver, sender)?;
        Ok(Self {
            process,
            requests,
            responses: Mutex::new(responses),
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
        validate_dispatch(&request)?;
        serialize_frame(&request, MAX_DISPATCH_IPC)?;
        self.requests.try_send(request)?;
        self.pending_since = Some(Instant::now());
        Ok(())
    }

    /// Polls completed output and kills a stalled compiler or guest after its deadline.
    pub fn poll(&mut self) -> Option<Result<Transaction>> {
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
    ensure!(
        startup.component.len() <= MAX_COMPONENT_BYTES * 2,
        "component too large"
    );
    let mut host = BundleHost::instantiate(
        &crypto::unhex(&startup.component)?,
        startup.owner,
        startup.capabilities,
        startup.epoch,
    )?;
    write_frame(&mut output, &host.take_transaction(), MAX_HOST_OUTPUT)?;
    loop {
        let request: Dispatch = read_frame(&mut input, MAX_DISPATCH_IPC)?;
        validate_dispatch(&request)?;
        let result = host.dispatch(
            &request.channel,
            &request.record,
            request.actions,
            request.epoch,
        )?;
        write_frame(&mut output, &result, MAX_HOST_OUTPUT)?;
    }
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

/// Checks callback metadata before serialization or queuing can copy it.
fn validate_dispatch(request: &Dispatch) -> Result<()> {
    ensure!(
        request.record.len() <= MAX_PAYLOAD_BYTES
            && server_experience::manifest::identifier(&request.channel)
            && request.actions.len() <= MAX_ACTIONS
            && request
                .actions
                .iter()
                .all(|action| server_experience::manifest::identifier(action)),
        "helper event too large or malformed"
    );
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
