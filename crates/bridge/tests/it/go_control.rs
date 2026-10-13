use std::io::Read;
use std::path::{Path, PathBuf};
use std::process::{Child, ChildStdin, Command, Stdio};
use std::thread::JoinHandle;
use std::time::{Duration, Instant};

use anyhow::{Context as _, Result, bail};
use bridge::{Lifecycle, PackAcquisition, PackApplication, PackDownstreamOutcome, PackOffer};

const START_TIMEOUT: Duration = Duration::from_secs(10);
const IO_TIMEOUT: Duration = Duration::from_secs(10);
const POLL_INTERVAL: Duration = Duration::from_millis(20);

#[tokio::test]
async fn go_status_endpoint_returns_the_strict_initialized_snapshot() -> Result<()> {
    let bridge_dir = PathBuf::from(env!("CARGO_MANIFEST_DIR"));
    let core_dir = bridge_dir
        .join("../../core")
        .canonicalize()
        .context("locate Go core module")?;
    let temp = tempfile::tempdir().context("create integration-test directory")?;
    let socket_dir = temp.path().join("socket");
    let executable = temp.path().join(if cfg!(windows) {
        "bedrock-core.exe"
    } else {
        "bedrock-core"
    });

    build_core(&core_dir, &executable)?;
    let child = ChildGuard::spawn(&executable, &socket_dir)?;
    let endpoint = bridge::control_endpoint_path(&socket_dir);
    let (mut child, status) = wait_for_initialization(child, socket_dir).await?;
    assert_eq!(status.schema_version, 1);
    assert_eq!(status.lifecycle, Lifecycle::Running);
    assert_eq!(status.pack_admission.attempt_id, 0);
    assert_eq!(status.pack_admission.offer, PackOffer::None);
    assert_eq!(
        status.pack_admission.application,
        PackApplication::Unavailable
    );

    child.terminate();
    wait_for_cleanup(endpoint).await?;
    Ok(())
}

#[tokio::test]
async fn live_go_status_compatibility_outcome_decodes_as_schema_v1() -> Result<()> {
    let bridge_dir = PathBuf::from(env!("CARGO_MANIFEST_DIR"));
    let core_dir = bridge_dir
        .join("../../core")
        .canonicalize()
        .context("locate Go core module")?;
    let temp = tempfile::tempdir().context("create integration-test directory")?;
    let socket_dir = temp.path().join("socket");
    let executable = temp.path().join(if cfg!(windows) {
        "control.test.exe"
    } else {
        "control.test"
    });

    build_control_test(&core_dir, &executable)?;
    let child = ChildGuard::spawn_status_helper(&executable, &socket_dir)?;
    let endpoint = bridge::control_endpoint_path(&socket_dir);
    let (mut child, status) = wait_for_initialization(child, socket_dir).await?;
    assert_eq!(status.schema_version, 1);
    assert_eq!(status.lifecycle, Lifecycle::Running);
    assert_eq!(status.pack_admission.attempt_id, 17);
    assert_eq!(status.pack_admission.offer, PackOffer::Required);
    assert_eq!(status.pack_admission.acquisition, PackAcquisition::Ignored);
    assert_eq!(
        status.pack_admission.downstream_outcome,
        PackDownstreamOutcome::StrippedIgnored
    );
    assert_eq!(
        status.pack_admission.application,
        PackApplication::Unavailable
    );

    child.terminate();
    wait_for_cleanup(endpoint).await?;
    Ok(())
}

fn build_core(core_dir: &Path, executable: &Path) -> Result<()> {
    let output = Command::new("go")
        .current_dir(core_dir)
        .arg("build")
        .arg("-o")
        .arg(executable)
        .arg("./cmd/bedrock-core")
        .output()
        .context("run go build for bedrock-core")?;
    if !output.status.success() {
        bail!(
            "go build bedrock-core failed with {}\nstdout:\n{}\nstderr:\n{}",
            output.status,
            String::from_utf8_lossy(&output.stdout),
            String::from_utf8_lossy(&output.stderr)
        );
    }
    Ok(())
}

fn build_control_test(core_dir: &Path, executable: &Path) -> Result<()> {
    let output = Command::new("go")
        .current_dir(core_dir)
        .arg("test")
        .arg("-c")
        .arg("-o")
        .arg(executable)
        .arg("./control")
        .output()
        .context("build Go control test helper")?;
    if !output.status.success() {
        bail!(
            "go test -c control failed with {}\nstdout:\n{}\nstderr:\n{}",
            output.status,
            String::from_utf8_lossy(&output.stdout),
            String::from_utf8_lossy(&output.stderr)
        );
    }
    Ok(())
}

/// Waits for a strictly decoded Running snapshot, not just the early control endpoint.
/// Starting remains pending; process exits, shutdown and invalid responses fail immediately.
async fn wait_for_initialization(
    mut child: ChildGuard,
    socket_dir: PathBuf,
) -> Result<(ChildGuard, bridge::StatusV1)> {
    let runtime = tokio::runtime::Handle::current();
    tokio::task::spawn_blocking(move || {
        let deadline = Instant::now() + START_TIMEOUT;
        let endpoint = bridge::control_endpoint_path(&socket_dir);
        let mut outcome = None;
        test_time::wait_until(START_TIMEOUT, || {
            let poll = (|| -> Result<Option<bridge::StatusV1>> {
                if let Some(status) = child
                    .child
                    .try_wait()
                    .context("poll bedrock-core process")?
                {
                    bail!("bedrock-core exited before initialization ({status})");
                }
                let remaining = deadline.saturating_duration_since(Instant::now());
                if !endpoint.exists() || remaining.is_zero() {
                    return Ok(None);
                }
                let status = runtime.block_on(async {
                    tokio::time::timeout(
                        remaining.min(IO_TIMEOUT),
                        bridge::read_status(&socket_dir),
                    )
                    .await
                    .context("timed out reading initialization Status v1")?
                    .context("read initialization Status v1")
                })?;
                match status.lifecycle {
                    Lifecycle::Starting => Ok(None),
                    Lifecycle::Running => Ok(Some(status)),
                    Lifecycle::Stopping => bail!("bedrock-core stopped before initialization"),
                }
            })();
            outcome = match poll {
                Ok(None) => None,
                Ok(Some(status)) => Some(Ok(status)),
                Err(error) => Some(Err(error)),
            };
            outcome.is_some()
        });
        match outcome {
            Some(Ok(status)) => Ok((child, status)),
            error => {
                child.terminate();
                let logs = child.collect_logs();
                match error {
                    Some(Err(error)) => Err(error.context(logs)),
                    _ => bail!(
                        "timed out waiting for initialized {}\n{logs}",
                        endpoint.display()
                    ),
                }
            }
        }
    })
    .await
    .context("join initialization worker")?
}

/// Waits for endpoint cleanup on a blocking worker without stalling the async runtime.
async fn wait_for_cleanup(endpoint: PathBuf) -> Result<()> {
    tokio::task::spawn_blocking(move || {
        if !test_time::wait_until(Duration::from_secs(2), || !endpoint.exists()) {
            bail!(
                "control endpoint remained after exit: {}",
                endpoint.display()
            );
        }
        Ok(())
    })
    .await
    .context("join endpoint cleanup worker")?
}

struct ChildGuard {
    child: Child,
    stdin: Option<ChildStdin>,
    stdout: Option<JoinHandle<String>>,
    stderr: Option<JoinHandle<String>>,
}

impl ChildGuard {
    fn spawn(executable: &Path, socket_dir: &Path) -> Result<Self> {
        let mut command = Command::new(executable);
        command
            .arg("-socket-dir")
            .arg(socket_dir)
            .arg("-upstream")
            .arg("127.0.0.1:19132")
            .arg("-control-status");
        Self::spawn_command(command, "spawn bedrock-core")
    }

    fn spawn_status_helper(executable: &Path, socket_dir: &Path) -> Result<Self> {
        let mut command = Command::new(executable);
        command
            .arg("-test.run=^TestBridgeCompatibilityStatusHelper$")
            .env("CINNABAR_BRIDGE_STATUS_HELPER", "1")
            .env("CINNABAR_BRIDGE_STATUS_SOCKET_DIR", socket_dir);
        Self::spawn_command(command, "spawn Go status compatibility helper")
    }

    fn spawn_command(mut command: Command, context: &'static str) -> Result<Self> {
        let mut child = command
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::piped())
            .spawn()
            .context(context)?;
        let stdin = child.stdin.take();
        let stdout = child.stdout.take().context("capture bedrock-core stdout")?;
        let stderr = child.stderr.take().context("capture bedrock-core stderr")?;
        Ok(Self {
            child,
            stdin,
            stdout: Some(read_log(stdout)),
            stderr: Some(read_log(stderr)),
        })
    }

    fn terminate(&mut self) {
        self.stdin.take();
        if test_time::wait_until(POLL_INTERVAL * 100, || {
            self.child.try_wait().ok().flatten().is_some()
        }) {
            return;
        }
        let _ = self.child.kill();
        let _ = self.child.wait();
    }

    fn collect_logs(&mut self) -> String {
        let stdout = join_log(self.stdout.take());
        let stderr = join_log(self.stderr.take());
        format!("stdout:\n{stdout}\nstderr:\n{stderr}")
    }
}

impl Drop for ChildGuard {
    fn drop(&mut self) {
        self.terminate();
        let _ = self.collect_logs();
    }
}

fn read_log(mut reader: impl Read + Send + 'static) -> JoinHandle<String> {
    std::thread::spawn(move || {
        let mut bytes = Vec::new();
        match reader.read_to_end(&mut bytes) {
            Ok(_) => String::from_utf8_lossy(&bytes).into_owned(),
            Err(error) => format!("<failed to read child log: {error}>"),
        }
    })
}

fn join_log(handle: Option<JoinHandle<String>>) -> String {
    handle
        .map(|handle| {
            handle
                .join()
                .unwrap_or_else(|_| "<child log reader panicked>".to_owned())
        })
        .unwrap_or_default()
}
