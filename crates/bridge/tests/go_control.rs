use std::io::Read;
use std::path::{Path, PathBuf};
use std::process::{Child, ChildStdin, Command, Stdio};
use std::thread::JoinHandle;
use std::time::{Duration, Instant};

use anyhow::{Context as _, Result, bail};
use bridge::{Lifecycle, PackApplication, PackOffer};

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
    let mut child = ChildGuard::spawn(&executable, &socket_dir)?;
    let endpoint = bridge::control_endpoint_path(&socket_dir);
    wait_for_publication(&mut child, &endpoint).await?;

    let status = tokio::time::timeout(IO_TIMEOUT, bridge::read_status(&socket_dir))
        .await
        .context("timed out reading Status v1")??;
    assert_eq!(status.schema_version, 1);
    assert_eq!(status.lifecycle, Lifecycle::Running);
    assert_eq!(status.pack_admission.attempt_id, 0);
    assert_eq!(status.pack_admission.offer, PackOffer::None);
    assert_eq!(
        status.pack_admission.application,
        PackApplication::Unavailable
    );

    child.terminate();
    wait_for_cleanup(&endpoint).await?;
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

async fn wait_for_publication(child: &mut ChildGuard, endpoint: &Path) -> Result<()> {
    let deadline = Instant::now() + START_TIMEOUT;
    loop {
        if endpoint.exists() {
            return Ok(());
        }
        if let Some(status) = child
            .child
            .try_wait()
            .context("poll bedrock-core process")?
        {
            let logs = child.collect_logs();
            bail!("bedrock-core exited before publishing endpoint ({status})\n{logs}");
        }
        if Instant::now() >= deadline {
            child.terminate();
            let logs = child.collect_logs();
            bail!("timed out waiting for {}\n{logs}", endpoint.display());
        }
        tokio::time::sleep(POLL_INTERVAL).await;
    }
}

async fn wait_for_cleanup(endpoint: &Path) -> Result<()> {
    let deadline = Instant::now() + Duration::from_secs(2);
    while endpoint.exists() && Instant::now() < deadline {
        tokio::time::sleep(POLL_INTERVAL).await;
    }
    if endpoint.exists() {
        bail!(
            "control endpoint remained after exit: {}",
            endpoint.display()
        );
    }
    Ok(())
}

struct ChildGuard {
    child: Child,
    stdin: Option<ChildStdin>,
    stdout: Option<JoinHandle<String>>,
    stderr: Option<JoinHandle<String>>,
}

impl ChildGuard {
    fn spawn(executable: &Path, socket_dir: &Path) -> Result<Self> {
        let mut child = Command::new(executable)
            .arg("-socket-dir")
            .arg(socket_dir)
            .arg("-upstream")
            .arg("127.0.0.1:19132")
            .arg("-control-status")
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::piped())
            .spawn()
            .context("spawn bedrock-core")?;
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
        for _ in 0..100 {
            if self.child.try_wait().ok().flatten().is_some() {
                return;
            }
            std::thread::sleep(POLL_INTERVAL);
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
