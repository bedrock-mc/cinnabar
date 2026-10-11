//! Bounded first-use archive reads for images too large for presentation-thread work.

use super::Source;
use crossbeam_channel::{Receiver, Sender, TrySendError};
use std::{
    cell::RefCell,
    sync::{
        Arc, OnceLock,
        atomic::{AtomicBool, Ordering},
    },
};

type Completed = (String, Option<Source>);

struct Job {
    key: String,
    path: String,
    view: resource_pack::LayeredPackView,
    cancelled: Arc<AtomicBool>,
    done: Sender<Completed>,
}

/// One archive read and one queued input bound inflation and decryption work process-wide.
fn worker() -> Option<&'static Sender<Job>> {
    static WORKER: OnceLock<Option<Sender<Job>>> = OnceLock::new();
    WORKER
        .get_or_init(|| {
            let (sender, receiver) = crossbeam_channel::bounded::<Job>(1);
            std::thread::Builder::new()
                .name("ui-image-sources".into())
                .spawn(move || {
                    while let Ok(job) = receiver.recv() {
                        if job.cancelled.load(Ordering::Acquire) {
                            continue;
                        }
                        let source = super::read_source(&job.view, &job.key, &job.path);
                        if !job.cancelled.load(Ordering::Acquire) {
                            let _ = job.done.try_send((job.key, source));
                        }
                    }
                })
                .ok()?;
            Some(sender)
        })
        .as_ref()
}

struct State {
    pending: Option<String>,
    retry: Option<Job>,
    receiver: Receiver<Completed>,
    sender: Sender<Completed>,
}

/// A pack owns one outstanding read; dropping it cancels publication from that namespace.
pub(super) struct SourceReads {
    state: RefCell<State>,
    cancelled: Arc<AtomicBool>,
}

impl Default for SourceReads {
    fn default() -> Self {
        let (sender, receiver) = crossbeam_channel::bounded(1);
        Self {
            state: RefCell::new(State {
                pending: None,
                retry: None,
                receiver,
                sender,
            }),
            cancelled: Arc::new(AtomicBool::new(false)),
        }
    }
}

impl Drop for SourceReads {
    fn drop(&mut self) {
        self.cancelled.store(true, Ordering::Release);
    }
}

impl SourceReads {
    /// Starts one large entry without reading or decompressing it on the requesting thread.
    pub(super) fn request(&self, key: &str, path: &str, view: &resource_pack::LayeredPackView) {
        let mut state = self.state.borrow_mut();
        if state.pending.is_some() || state.retry.is_some() {
            return;
        }
        let job = Job {
            key: key.to_owned(),
            path: path.to_owned(),
            view: view.clone(),
            cancelled: Arc::clone(&self.cancelled),
            done: state.sender.clone(),
        };
        Self::submit(&mut state, job);
    }

    /// Delivers readiness once and retries a request deferred by another pack's queue occupancy.
    pub(super) fn collect(&self) -> Option<Completed> {
        let mut state = self.state.borrow_mut();
        if let Some(job) = state.retry.take() {
            Self::submit(&mut state, job);
        }
        let result = state.receiver.try_recv().ok()?;
        state.pending = None;
        Some(result)
    }

    /// Retains the request on backpressure and publishes a terminal failure if the worker stops.
    fn submit(state: &mut State, job: Job) {
        let Some(worker) = worker() else {
            let _ = job.done.try_send((job.key, None));
            return;
        };
        let key = job.key.clone();
        match worker.try_send(job) {
            Ok(()) => state.pending = Some(key),
            Err(TrySendError::Full(job)) => state.retry = Some(job),
            Err(TrySendError::Disconnected(job)) => {
                let _ = job.done.try_send((job.key, None));
            }
        }
    }
}
