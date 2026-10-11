//! Lifecycle of the spawned `bedrock-core` child process.
//!
//! The guard owns the tracked child; closing its piped stdin is the graceful
//! cancellation path bedrock-core itself consumes.

use std::{
    fs,
    path::{Path, PathBuf},
    process::{Command, Stdio},
    time::{Duration, Instant},
};

use anyhow::{Context, Result, bail};
use {
    crate::lifecycle::children::{self, Spawned, StopOutcome},
    launcher::install_layout::InstallLayout,
};

/// Bounds the graceful-stop wait before SIGTERM, then SIGKILL, fire.
///
/// With that escalation the stop stays inside the post-`AppExit` shutdown
/// watchdog envelope (2 s), so a wedged core cannot turn orderly teardown
/// into a watchdog `process::exit` that skips the stop.
const CORE_GRACEFUL_STOP_DEADLINE: Duration = children::EXIT_GRACE;
use bridge::{
    CORE_START_TIMEOUT, bridge_endpoint_exists, session_endpoint_path as bridge_endpoint_path,
};

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum CoreStopOutcome {
    NotRunning,
    ExitedAfterGracefulClose,
    TerminatedAfterGracefulTimeout,
    KilledAfterGracefulTimeout,
    /// Survived SIGKILL's bounded wait; the exit sweep retries it.
    Unreaped,
}

#[derive(Debug, Default)]
pub struct CoreProcessGuard {
    child: Option<Spawned>,
}

impl CoreProcessGuard {
    /// Stops (and reaps) the current core before adopting `child`.
    pub fn replace(&mut self, child: impl Into<Spawned>) {
        self.stop();
        self.child = Some(child.into());
    }

    /// Stops the core gracefully: close its piped stdin (the cancellation
    /// path bedrock-core itself consumes), wait bounded for exit, then
    /// escalate to SIGTERM and SIGKILL, so endpoint leases and the pack cache
    /// still see an orderly shutdown whenever the core honors stdin EOF.
    pub fn stop(&mut self) -> CoreStopOutcome {
        self.stop_with_deadline(CORE_GRACEFUL_STOP_DEADLINE)
    }

    pub fn stop_with_deadline(&mut self, deadline: Duration) -> CoreStopOutcome {
        let Some(child) = self.child.take() else {
            return CoreStopOutcome::NotRunning;
        };
        match child.stop(deadline) {
            StopOutcome::Exited => CoreStopOutcome::ExitedAfterGracefulClose,
            StopOutcome::Terminated => CoreStopOutcome::TerminatedAfterGracefulTimeout,
            StopOutcome::Killed => CoreStopOutcome::KilledAfterGracefulTimeout,
            StopOutcome::Unreaped => CoreStopOutcome::Unreaped,
        }
    }

    /// Returns the tracked child PID for process-lifetime fixtures.
    #[cfg(any(test, feature = "test-support"))]
    pub fn id(&self) -> Option<u32> {
        self.child.as_ref().map(Spawned::id)
    }

    /// Whether the child has already exited on its own.
    pub fn exited(&mut self) -> bool {
        self.child
            .as_ref()
            .is_some_and(|child| matches!(child.try_wait(), Ok(Some(_))))
    }

    /// [`Self::stop`] on a reaper thread, running `then` once the core is gone,
    /// so the frame never waits out the graceful deadline. The child stays
    /// tracked, so the exit sweep still ends it if the process exits first.
    pub fn stop_detached(&mut self, then: impl FnOnce() + Send + 'static) {
        let Some(child) = self.child.take() else {
            then();
            return;
        };
        type Job = (Spawned, Box<dyn FnOnce() + Send>);
        let (job, next) = crossbeam_channel::bounded::<Job>(1);
        let spawned = std::thread::Builder::new()
            .name("bedrock-core-reaper".to_owned())
            .spawn(move || {
                if let Ok((child, then)) = next.recv() {
                    child.stop(CORE_GRACEFUL_STOP_DEADLINE);
                    then();
                }
            });
        if let Err(error) = spawned {
            tracing::warn!("core reaper unavailable, stopping inline: {error}");
            child.stop(CORE_GRACEFUL_STOP_DEADLINE);
            then();
            return;
        }
        let _ = job.send((child, Box::new(then)));
    }
}

impl Drop for CoreProcessGuard {
    fn drop(&mut self) {
        self.stop();
    }
}

/// Stops a just-spawned core completely before running setup rollback such
/// as releasing its session-directory ownership.
pub fn stop_core_then<T>(
    guard: &mut CoreProcessGuard,
    release: impl FnOnce(CoreStopOutcome) -> T,
) -> T {
    let outcome = guard.stop();
    release(outcome)
}

/// `ask_server_trust` is set when a menu polls this core for its server trust question; the
/// menu-less `--address` session has nobody to ask.
pub fn spawn_core_for_address(
    layout: &InstallLayout,
    socket_dir: &Path,
    address: &str,
    auth_cache: Option<&Path>,
    enable_upstream_client_cache: bool,
    ask_server_trust: bool,
) -> Result<Spawned> {
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
    if ask_server_trust {
        command
            .arg("-server-trust-file")
            .arg(layout.server_trust_file());
    }
    children::spawn(&mut command)
        .with_context(|| format!("spawn {} for {address}", executable.display()))
}

/// Builds a direct game core command with the install's cache and identity paths.
pub fn core_command_for_address(
    layout: &InstallLayout,
    executable: &Path,
    socket_dir: &Path,
    address: &str,
    auth_cache: Option<&Path>,
    enable_upstream_client_cache: bool,
) -> Command {
    // The fallback core needs the same default-port normalization as `connect.v1`.
    let address = match crate::launcher_core::target_for(address) {
        bridge::ConnectTarget::RakNet(address) => address,
        _ => address.to_owned(),
    };
    let mut command = Command::new(executable);
    command
        .arg("-control-status")
        .arg("-socket-dir")
        .arg(socket_dir)
        .arg("-upstream")
        .arg(address)
        .arg("-resource-pack-cache-dir")
        .arg(layout.resource_pack_cache_dir())
        .arg("-device-file")
        .arg(layout.device_profile_file())
        .stdin(Stdio::piped())
        .stdout(Stdio::null())
        .stderr(
            crate::lifecycle::core_health::open_core_log(layout)
                .map_or_else(Stdio::null, Stdio::from),
        );
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
pub fn clear_stale_bridge_endpoint(socket_dir: &Path) -> Result<()> {
    let endpoint = bridge_endpoint_path(socket_dir);
    match fs::remove_file(&endpoint) {
        Ok(()) => Ok(()),
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => Ok(()),
        Err(error) => Err(error)
            .with_context(|| format!("remove stale bridge endpoint {}", endpoint.display())),
    }
}

/// Blocks until the core publishes its endpoint; only for paths off the frame.
pub fn wait_for_core(socket_dir: &Path) -> Result<()> {
    let deadline = Instant::now() + CORE_START_TIMEOUT;
    while Instant::now() < deadline {
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

pub fn core_executable(layout: &InstallLayout) -> Option<PathBuf> {
    layout
        .core_executable
        .is_file()
        .then(|| layout.core_executable.clone())
}

pub fn auth_cache_path(layout: &InstallLayout) -> Option<PathBuf> {
    Some(layout.auth_cache()).filter(|path| path.is_file())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn fallback_core_normalizes_the_same_raknet_address_as_the_launcher() {
        let layout = launcher::test_support::scratch("fallback-target");
        for address in ["example.test", "example.test:19133", "::1", "realm_id/42"] {
            let command = core_command_for_address(
                &layout,
                &layout.core_executable,
                &layout.runtime_root,
                address,
                None,
                false,
            );
            let args: Vec<_> = command.get_args().collect();
            let upstream = args.windows(2).find(|pair| pair[0] == "-upstream").unwrap()[1];
            let expected = match crate::launcher_core::target_for(address) {
                bridge::ConnectTarget::RakNet(address) => address,
                _ => address.to_owned(),
            };
            assert_eq!(upstream, std::ffi::OsStr::new(&expected));
        }
    }

    #[test]
    fn direct_core_enables_private_control_with_or_without_optional_join_settings() {
        let layout = launcher::test_support::scratch("direct-core-control");
        for configured in [false, true] {
            let cache = layout.auth_cache();
            let command = core_command_for_address(
                &layout,
                &layout.core_executable,
                &layout.runtime_root,
                "example.test",
                configured.then_some(cache.as_path()),
                configured,
            );
            assert!(
                command
                    .get_args()
                    .any(|argument| argument == "-control-status")
            );
            assert!(
                command
                    .get_args()
                    .all(|argument| argument != "-xbox-presence")
            );
        }
    }
}

#[cfg(test)]
#[path = "core_process/lifecycle_tests.rs"]
mod lifecycle_tests;
