//! One coalescing disk writer over a bounded shared sample snapshot.

use super::FailureSamples;
use crossbeam_channel::{Receiver, Sender, bounded};
use std::{
    io,
    path::PathBuf,
    sync::{
        Arc, Mutex, PoisonError,
        atomic::{AtomicU64, Ordering},
    },
    time::{Duration, Instant},
};

/// Minimum interval between disk snapshots under a continuous rejection flood.
const COALESCE: Duration = Duration::from_millis(100);

/// A one-slot wakeup queue; sample bodies stay in the bounded shared store.
pub(crate) struct SampleWriter {
    wake: Sender<Option<Sender<()>>>,
}

impl SampleWriter {
    /// Starts persistence without holding the sample mutex during any filesystem operation.
    pub(crate) fn start(
        dir: PathBuf,
        samples: Arc<Mutex<FailureSamples>>,
        missed: Arc<AtomicU64>,
    ) -> io::Result<Self> {
        let (wake, received) = bounded(1);
        std::thread::Builder::new()
            .name("packet-samples".into())
            .spawn(move || run(dir, samples, missed, received))?;
        Ok(Self { wake })
    }

    /// Coalesces another change without waiting for the writer or reserving another snapshot.
    pub(crate) fn changed(&self) {
        let _ = self.wake.try_send(None);
    }

    /// Waits for a persisted snapshot in tests without relying on filesystem timing.
    #[cfg(any(test, feature = "test-support"))]
    pub(crate) fn flush(&self) {
        let (done, finished) = bounded(1);
        self.wake
            .send_timeout(Some(done), Duration::from_secs(10))
            .unwrap();
        finished.recv_timeout(Duration::from_secs(10)).unwrap();
    }
}

/// Coalesces wakeups, copies under the mutex, and performs all disk work after releasing it.
fn run(
    dir: PathBuf,
    samples: Arc<Mutex<FailureSamples>>,
    missed: Arc<AtomicU64>,
    received: Receiver<Option<Sender<()>>>,
) {
    let mut persisted = None;
    while let Ok(mut barrier) = received.recv() {
        let deadline = Instant::now() + COALESCE;
        let mut disconnected = false;
        while barrier.is_none() {
            match received.recv_timeout(deadline.saturating_duration_since(Instant::now())) {
                Ok(next) => barrier = next,
                Err(crossbeam_channel::RecvTimeoutError::Timeout) => break,
                Err(crossbeam_channel::RecvTimeoutError::Disconnected) => {
                    disconnected = true;
                    break;
                }
            }
            if Instant::now() >= deadline {
                break;
            }
        }
        let snapshot = samples
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
            .clone();
        let missed = missed.load(Ordering::Relaxed);
        match snapshot.persist_since(&dir, persisted, missed) {
            Ok(()) => persisted = Some((snapshot.session, snapshot.samples.len())),
            Err(error) => {
                tracing::warn!(dir = %dir.display(), %error, "could not save rejected packet samples")
            }
        }
        if let Some(done) = barrier {
            let _ = done.send(());
        }
        if disconnected {
            break;
        }
    }
}
