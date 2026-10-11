//! Parent side of the media decoder process: it alone fetches and verifies signed chunks,
//! the child only decodes, and a watchdog kills a child past its memory ceiling.

use super::{
    ceiling::{HELPER_MEMORY_BYTES, footprint_bytes},
    descriptor::Descriptor,
    ipc::{self, Reply, Request, Start},
    ranges::{ChunkSource, HttpsChunks, load_verified},
    service::output::Output,
};
use anyhow::{Result, ensure};
use std::{
    collections::BTreeSet,
    io::{Read, Write},
    path::Path,
    process::{Child, Command, Stdio},
    sync::{
        Arc, Mutex,
        atomic::{AtomicBool, AtomicU64, Ordering},
        mpsc,
    },
    time::Duration,
};

/// Helper subcommand that serves one decode over stdio.
pub const HELPER_COMMAND: &str = "media-helper";
const OUTPUT_DEPTH: usize = 2;
static DECODER_ACTIVE: AtomicBool = AtomicBool::new(false);

pub(super) struct DecoderLease(());

impl DecoderLease {
    /// Claims the process-wide decoder slot; None while an earlier decoder still holds it.
    pub(super) fn acquire() -> Option<Self> {
        DECODER_ACTIVE
            .compare_exchange(false, true, Ordering::AcqRel, Ordering::Acquire)
            .is_ok()
            .then(|| Self(()))
    }

    /// Whether a new decoder could start now.
    pub(super) fn free() -> bool {
        !DECODER_ACTIVE.load(Ordering::Acquire)
    }
}

impl Drop for DecoderLease {
    /// Holds the process-wide decoder slot until the IPC thread really exits.
    fn drop(&mut self) {
        DECODER_ACTIVE.store(false, Ordering::Release);
    }
}

type SharedChild = Arc<Mutex<Option<Child>>>;

pub struct Worker {
    cancelled: Arc<AtomicBool>,
    output: Mutex<mpsc::Receiver<Result<Output>>>,
    child: SharedChild,
}

impl Worker {
    /// True when a helper that can enforce the memory ceiling is installed.
    pub fn available(executable: &Path) -> bool {
        if cfg!(windows) {
            return false;
        }
        let Ok(metadata) = std::fs::metadata(executable) else {
            return false;
        };
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            metadata.is_file() && metadata.permissions().mode() & 0o111 != 0
        }
        #[cfg(not(unix))]
        {
            metadata.is_file()
        }
    }

    /// Starts one decode from `start_us` in a fresh helper; only under the developer switch.
    pub fn start(
        executable: &Path,
        descriptor: Descriptor,
        origins: BTreeSet<String>,
        generation: u64,
        data_budget: Arc<AtomicU64>,
        start_us: u64,
    ) -> Result<Self> {
        ensure!(
            std::env::var(crate::policy::DEVELOPER_ENV).as_deref() == Ok("1"),
            "native media requires a restricted production helper"
        );
        ensure!(
            Self::available(executable),
            "media decoding requires a memory-limited helper"
        );
        let cancelled = Arc::new(AtomicBool::new(false));
        let source = HttpsChunks::new(
            descriptor.clone(),
            origins,
            Arc::clone(&cancelled),
            data_budget,
        )?;
        let mut command = Command::new(executable);
        command
            .arg(HELPER_COMMAND)
            .env_clear()
            .env(crate::policy::DEVELOPER_ENV, "1");
        Self::spawn(command, descriptor, source, generation, start_us, cancelled)
    }

    /// Launches `command` with private stdio and serves its chunk requests from `source`.
    pub(crate) fn spawn<S: ChunkSource + Send + 'static>(
        mut command: Command,
        descriptor: Descriptor,
        source: S,
        generation: u64,
        start_us: u64,
        cancelled: Arc<AtomicBool>,
    ) -> Result<Self> {
        let lease = DecoderLease::acquire()
            .ok_or_else(|| anyhow::anyhow!("media decoder already active"))?;
        let mut child = command
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::null())
            .spawn()?;
        let (Some(input), Some(output)) = (child.stdin.take(), child.stdout.take()) else {
            let _ = child.kill();
            let _ = child.wait();
            anyhow::bail!("missing media helper pipes");
        };
        let pid = child.id();
        let child: SharedChild = Arc::new(Mutex::new(Some(child)));
        let over_ceiling = Arc::new(AtomicBool::new(false));
        let (sender, receiver) = mpsc::sync_channel(OUTPUT_DEPTH);
        {
            let child = Arc::clone(&child);
            let over_ceiling = Arc::clone(&over_ceiling);
            std::thread::Builder::new()
                .name("experience-media-ipc".into())
                .spawn(move || {
                    let _lease = lease;
                    let start = Start {
                        descriptor: descriptor.clone(),
                        start_us,
                    };
                    let result = serve_child(
                        input,
                        output,
                        &descriptor,
                        source,
                        start,
                        generation,
                        &sender,
                    );
                    if let Err(error) = result {
                        let error = if over_ceiling.load(Ordering::Acquire) {
                            anyhow::anyhow!("media helper exceeded its memory ceiling")
                        } else {
                            error
                        };
                        let _ = sender.send(Err(error));
                    }
                    kill(&child);
                })?;
        }
        {
            let child = Arc::clone(&child);
            let cancelled = Arc::clone(&cancelled);
            std::thread::Builder::new()
                .name("experience-media-watchdog".into())
                .spawn(move || watchdog(pid, &child, &cancelled, &over_ceiling))?;
        }
        Ok(Self {
            cancelled,
            output: Mutex::new(receiver),
            child,
        })
    }

    /// Reads only completed output; frame validation is repeated by the consumer.
    pub fn poll(&self) -> Option<Result<Output>> {
        self.output.lock().ok()?.try_recv().ok()
    }
}

impl Drop for Worker {
    /// Cancels fetches, kills the helper and reaps it off the calling thread.
    fn drop(&mut self) {
        self.cancelled.store(true, Ordering::Release);
        let child = self.child.lock().ok().and_then(|mut child| child.take());
        if let Some(mut child) = child {
            let _ = child.kill();
            let _ = std::thread::Builder::new()
                .name("experience-media-reap".into())
                .spawn(move || {
                    let _ = child.wait();
                });
        }
    }
}

/// Runs one decode session; returns once the helper ends, fails or the consumer leaves.
pub(crate) fn serve_child(
    mut input: impl Write,
    mut output: impl Read,
    descriptor: &Descriptor,
    mut source: impl ChunkSource,
    start: Start,
    generation: u64,
    sender: &mpsc::SyncSender<Result<Output>>,
) -> Result<()> {
    ipc::write_request(&mut input, &Request::Start(Box::new(start)))
        .map_err(|error| error.context("media helper exited"))?;
    let mut ended = false;
    loop {
        let reply = match ipc::read_reply(&mut output, generation) {
            Ok(reply) => reply,
            Err(_) if ended => return Ok(()),
            Err(error) => return Err(error.context("media helper exited")),
        };
        match reply {
            Reply::Need(index) => {
                let bytes = load_verified(descriptor, &mut source, index as usize)?;
                ipc::write_request(&mut input, &Request::Chunk { index, bytes })
                    .map_err(|error| error.context("media helper exited"))?;
            }
            Reply::Output(output) => {
                ended |= matches!(output, Output::End);
                if sender.send(Ok(output)).is_err() {
                    return Ok(());
                }
            }
            Reply::Error(text) => anyhow::bail!("media helper failed: {text}"),
        }
    }
}

fn kill(child: &SharedChild) {
    if let Ok(mut child) = child.lock()
        && let Some(child) = child.as_mut()
    {
        let _ = child.kill();
    }
}

/// Kills the helper once its footprint passes the process ceiling.
fn watchdog(pid: u32, child: &SharedChild, cancelled: &AtomicBool, over: &AtomicBool) {
    while !cancelled.load(Ordering::Acquire) {
        let alive = child.lock().ok().is_some_and(|mut child| {
            child
                .as_mut()
                .is_some_and(|c| matches!(c.try_wait(), Ok(None)))
        });
        if !alive {
            return;
        }
        if footprint_bytes(pid).is_some_and(|bytes| bytes > HELPER_MEMORY_BYTES) {
            over.store(true, Ordering::Release);
            kill(child);
            return;
        }
        std::thread::sleep(Duration::from_millis(50));
    }
}

#[cfg(test)]
pub(crate) mod tests {
    use super::*;
    use crate::media::ranges::tests::descriptor_for;

    /// Held by every test that claims the process-wide decoder slot, until it is free again.
    pub(crate) static DECODER_SLOT: std::sync::Mutex<()> = std::sync::Mutex::new(());

    #[test]
    fn a_refused_acquisition_leaves_the_owner_holding_the_slot() {
        let _slot = DECODER_SLOT.lock();
        let owner = DecoderLease::acquire().unwrap();
        assert!(DecoderLease::acquire().is_none());
        assert!(
            !DecoderLease::free(),
            "a refused claim released the owner's slot"
        );
        drop(owner);
        assert!(DecoderLease::free());
    }

    #[cfg(unix)]
    #[test]
    fn a_helper_that_dies_reports_failure_and_frees_the_decoder_slot() {
        use crate::media::ranges::tests::MemoryChunks;

        let _slot = DECODER_SLOT.lock();
        let bytes = vec![0u8; 1000];
        let mut command = Command::new("/bin/sh");
        command.args(["-c", "head -c 4 >/dev/null; exit 3"]);
        let worker = Worker::spawn(
            command,
            descriptor_for(&bytes),
            MemoryChunks {
                bytes,
                corrupt: None,
                loads: 0,
            },
            1,
            0,
            Arc::new(AtomicBool::new(false)),
        )
        .unwrap();
        let deadline = std::time::Instant::now() + Duration::from_secs(10);
        let mut result = None;
        test_time::eventually_within(
            deadline.saturating_duration_since(std::time::Instant::now()),
            "a helper failure",
            || {
                result = worker.poll();
                result.is_some()
            },
        );
        let error = result.unwrap().unwrap_err();
        assert!(error.to_string().contains("media helper exited"), "{error}");
        drop(worker);
        test_time::eventually_within(
            deadline.saturating_duration_since(std::time::Instant::now()),
            "the decoder slot to be released",
            || !DECODER_ACTIVE.load(Ordering::Acquire),
        );
    }

    #[test]
    fn production_builds_without_the_switch_never_spawn_a_helper() {
        if std::env::var(crate::policy::DEVELOPER_ENV).as_deref() == Ok("1") {
            return;
        }
        let bytes = vec![0u8; 1000];
        let error = Worker::start(
            Path::new("/bin/sh"),
            descriptor_for(&bytes),
            BTreeSet::from(["https://example.com".to_owned()]),
            1,
            Arc::new(AtomicU64::new(u64::MAX)),
            0,
        )
        .err()
        .unwrap();
        assert!(error.to_string().contains("restricted production helper"));
        assert!(!Worker::available(Path::new("/nonexistent/mod-host")));
    }
}
