use std::io::Read;
use std::path::{Path, PathBuf};
use std::process::{Child, Command, ExitStatus, Stdio};
use std::thread::JoinHandle;
use std::time::{Duration, Instant};

use anyhow::{Context as _, Result, bail};
use bytes::Bytes;
use futures::{SinkExt, StreamExt};

const START_TIMEOUT: Duration = Duration::from_secs(10);
const IO_TIMEOUT: Duration = Duration::from_secs(10);
const EXIT_TIMEOUT: Duration = Duration::from_secs(10);
const POLL_INTERVAL: Duration = Duration::from_millis(20);

#[tokio::test]
async fn go_frame_echo_round_trips_binary_payloads_and_cleans_up() -> Result<()> {
    let bridge_dir = PathBuf::from(env!("CARGO_MANIFEST_DIR"));
    let core_dir = bridge_dir
        .join("../../core")
        .canonicalize()
        .context("locate Go core module")?;
    let temp = tempfile::tempdir().context("create integration-test directory")?;
    let socket_dir = temp.path().join("socket");
    let executable = temp.path().join(if cfg!(windows) {
        "frame-echo.exe"
    } else {
        "frame-echo"
    });

    build_fixture(&core_dir, &executable)?;
    let mut child = ChildGuard::spawn(&executable, &socket_dir)?;
    let endpoint = socket_dir.join(if cfg!(windows) {
        "game.addr"
    } else {
        "game.sock"
    });
    wait_for_publication(&mut child, &endpoint).await?;

    let mut stream = tokio::time::timeout(IO_TIMEOUT, bridge::connect(&socket_dir))
        .await
        .context("timed out connecting to frame-echo")??;

    round_trip(
        &mut stream,
        Bytes::from_static(&[0x00, 0xfe, 0x00, 0x01, 0xff, 0x00]),
    )
    .await?;
    let large = Bytes::from(
        (0..1024 * 1024)
            .map(|index| (index % 251) as u8)
            .collect::<Vec<_>>(),
    );
    round_trip(&mut stream, large).await?;

    tokio::time::timeout(IO_TIMEOUT, stream.close())
        .await
        .context("timed out closing bridge stream")??;
    drop(stream);

    let status = wait_for_exit(&mut child).await?;
    let logs = child.collect_logs();
    if !status.success() {
        bail!("frame-echo exited with {status}\n{logs}");
    }
    wait_for_cleanup(&endpoint).await?;
    Ok(())
}

fn build_fixture(core_dir: &Path, executable: &Path) -> Result<()> {
    let output = Command::new("go")
        .current_dir(core_dir)
        .arg("build")
        .arg("-o")
        .arg(executable)
        .arg("./cmd/frame-echo")
        .output()
        .context("run go build for frame-echo")?;
    if !output.status.success() {
        bail!(
            "go build frame-echo failed with {}\nstdout:\n{}\nstderr:\n{}",
            output.status,
            String::from_utf8_lossy(&output.stdout),
            String::from_utf8_lossy(&output.stderr)
        );
    }
    Ok(())
}

async fn round_trip(stream: &mut bridge::FramedStream, payload: Bytes) -> Result<()> {
    tokio::time::timeout(IO_TIMEOUT, stream.send(payload.clone()))
        .await
        .context("timed out sending frame")??;
    let echoed = tokio::time::timeout(IO_TIMEOUT, stream.next())
        .await
        .context("timed out receiving echoed frame")?
        .context("frame-echo closed before replying")??;
    if echoed != payload {
        bail!(
            "echoed payload differs: got {} bytes, expected {}",
            echoed.len(),
            payload.len()
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
        if let Some(status) = child.try_wait().context("poll frame-echo process")? {
            let logs = child.collect_logs();
            bail!("frame-echo exited before publishing endpoint ({status})\n{logs}");
        }
        if Instant::now() >= deadline {
            child.terminate();
            let logs = child.collect_logs();
            bail!("timed out waiting for {}\n{logs}", endpoint.display());
        }
        tokio::time::sleep(POLL_INTERVAL).await;
    }
}

async fn wait_for_exit(child: &mut ChildGuard) -> Result<ExitStatus> {
    let deadline = Instant::now() + EXIT_TIMEOUT;
    loop {
        if let Some(status) = child.try_wait().context("poll frame-echo shutdown")? {
            return Ok(status);
        }
        if Instant::now() >= deadline {
            child.terminate();
            let logs = child.collect_logs();
            bail!("timed out waiting for frame-echo shutdown\n{logs}");
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
            "endpoint remained after frame-echo exit: {}",
            endpoint.display()
        );
    }
    Ok(())
}

struct ChildGuard {
    child: Child,
    stdout: Option<JoinHandle<String>>,
    stderr: Option<JoinHandle<String>>,
    reaped: bool,
}

impl ChildGuard {
    fn spawn(executable: &Path, socket_dir: &Path) -> Result<Self> {
        let mut child = Command::new(executable)
            .arg(socket_dir)
            .stdout(Stdio::piped())
            .stderr(Stdio::piped())
            .spawn()
            .context("spawn frame-echo")?;
        let stdout = child.stdout.take().context("capture frame-echo stdout")?;
        let stderr = child.stderr.take().context("capture frame-echo stderr")?;
        Ok(Self {
            child,
            stdout: Some(read_log(stdout)),
            stderr: Some(read_log(stderr)),
            reaped: false,
        })
    }

    fn try_wait(&mut self) -> std::io::Result<Option<ExitStatus>> {
        let status = self.child.try_wait()?;
        if status.is_some() {
            self.reaped = true;
        }
        Ok(status)
    }

    fn terminate(&mut self) {
        if self.reaped {
            return;
        }
        let _ = self.child.kill();
        let _ = self.child.wait();
        self.reaped = true;
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
