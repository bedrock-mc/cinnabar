//! Lifecycle of the spawned `bedrock-core` child process.
//!
//! The guard owns both the child handle and the piped stdin that
//! bedrock-core consumes as its graceful cancellation path.

use std::{
    fs,
    path::{Path, PathBuf},
    process::{Child, ChildStdin, Command, Stdio},
    time::{Duration, Instant},
};

use anyhow::{Context, Result, bail};
use bevy::prelude::Resource;

use crate::{
    install_layout::InstallLayout,
    runtime::endpoint::{bridge_endpoint_exists, bridge_endpoint_path},
};

/// Bounds the graceful-stop wait before the kill fallback fires.
///
/// The deadline must stay inside the post-`AppExit` shutdown watchdog
/// envelope (2 s) so a wedged core cannot turn orderly teardown into a
/// watchdog `process::exit` that would orphan the child. Explicit stop
/// callers run before the watchdog arms; the `Drop` path accepts roughly
/// half a second of teardown slack before that race, and the OS closing
/// the pipe still delivers stdin EOF to the core even then.
const CORE_GRACEFUL_STOP_DEADLINE: Duration = Duration::from_millis(1_500);
const CORE_STOP_POLL_INTERVAL: Duration = Duration::from_millis(20);

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) enum CoreStopOutcome {
    NotRunning,
    ExitedAfterGracefulClose,
    KilledAfterGracefulTimeout,
}

#[derive(Debug, Resource, Default)]
pub(crate) struct CoreProcessGuard {
    child: Option<Child>,
    stdin: Option<ChildStdin>,
}

impl CoreProcessGuard {
    pub(crate) fn replace(&mut self, mut child: Child) {
        self.stop();
        self.stdin = child.stdin.take();
        self.child = Some(child);
    }

    /// Stops the core gracefully: close its piped stdin (the cancellation
    /// path bedrock-core itself consumes), wait bounded for exit, then kill
    /// only as a fallback so endpoint leases and the pack cache still see an
    /// orderly shutdown whenever the core honors stdin EOF.
    pub(crate) fn stop(&mut self) -> CoreStopOutcome {
        self.stop_with_deadline(CORE_GRACEFUL_STOP_DEADLINE)
    }

    pub(crate) fn stop_with_deadline(&mut self, deadline: Duration) -> CoreStopOutcome {
        let Some(mut child) = self.child.take() else {
            self.stdin = None;
            return CoreStopOutcome::NotRunning;
        };
        drop(self.stdin.take());
        let started = Instant::now();
        while started.elapsed() < deadline {
            if let Ok(Some(_)) = child.try_wait() {
                return CoreStopOutcome::ExitedAfterGracefulClose;
            }
            std::thread::sleep(CORE_STOP_POLL_INTERVAL);
        }
        let _ = child.kill();
        let _ = child.wait();
        CoreStopOutcome::KilledAfterGracefulTimeout
    }
}

impl Drop for CoreProcessGuard {
    fn drop(&mut self) {
        self.stop();
    }
}

/// Stops a just-spawned core completely before running setup rollback such
/// as releasing its session-directory ownership.
pub(crate) fn stop_core_then<T>(
    guard: &mut CoreProcessGuard,
    release: impl FnOnce(CoreStopOutcome) -> T,
) -> T {
    let outcome = guard.stop();
    release(outcome)
}

pub(crate) fn spawn_core_for_address(
    layout: &InstallLayout,
    socket_dir: &Path,
    address: &str,
    auth_cache: Option<&Path>,
    enable_upstream_client_cache: bool,
) -> Result<Child> {
    let executable = core_executable(layout).ok_or_else(|| {
        anyhow::anyhow!(
            "bedrock-core executable was not found at {}",
            layout.core_executable.display()
        )
    })?;
    clear_stale_bridge_endpoint(socket_dir)?;
    let mut command = core_command_for_address(
        layout,
        &executable,
        socket_dir,
        address,
        auth_cache,
        enable_upstream_client_cache,
    );
    let child = command
        .spawn()
        .with_context(|| format!("spawn {} for {address}", executable.display()))?;
    Ok(child)
}

pub(super) fn core_command_for_address(
    layout: &InstallLayout,
    executable: &Path,
    socket_dir: &Path,
    address: &str,
    auth_cache: Option<&Path>,
    enable_upstream_client_cache: bool,
) -> Command {
    let mut command = Command::new(executable);
    command
        .arg("-socket-dir")
        .arg(socket_dir)
        .arg("-upstream")
        .arg(address)
        .arg("-resource-pack-cache-dir")
        .arg(layout.resource_pack_cache_dir())
        .stdin(Stdio::piped())
        .stdout(Stdio::null())
        .stderr(Stdio::null());
    // Passed only when the spawning session provably owns the verified blob
    // cache whose resolver advertises cache support downstream: the core's
    // upstream advertisement must never lead the downstream one.
    if enable_upstream_client_cache {
        command.arg("-upstream-client-cache");
    }
    if let Some(auth_cache) = auth_cache {
        command.arg("-auth-cache").arg(auth_cache);
    }
    command
}

/// Drops any endpoint publication left behind by an earlier core.
///
/// [`wait_for_core`] can only observe that the endpoint exists, so a stale
/// publication would satisfy it immediately and the client would dial a socket
/// nothing is listening on. Clearing it first means the wait observes the newly
/// spawned core's own bind.
pub(crate) fn clear_stale_bridge_endpoint(socket_dir: &Path) -> Result<()> {
    let endpoint = bridge_endpoint_path(socket_dir);
    match fs::remove_file(&endpoint) {
        Ok(()) => Ok(()),
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => Ok(()),
        Err(error) => Err(error)
            .with_context(|| format!("remove stale bridge endpoint {}", endpoint.display())),
    }
}

pub(crate) fn wait_for_core(socket_dir: &Path) -> Result<()> {
    for _ in 0..100 {
        if bridge_endpoint_exists(socket_dir) {
            return Ok(());
        }
        std::thread::sleep(Duration::from_millis(50));
    }
    bail!(
        "bedrock-core did not publish its endpoint at {}",
        socket_dir.display()
    )
}

pub(super) fn core_executable(layout: &InstallLayout) -> Option<PathBuf> {
    layout
        .core_executable
        .is_file()
        .then(|| layout.core_executable.clone())
}

pub(super) fn auth_cache_path(layout: &InstallLayout) -> Option<PathBuf> {
    Some(layout.auth_cache()).filter(|path| path.is_file())
}
