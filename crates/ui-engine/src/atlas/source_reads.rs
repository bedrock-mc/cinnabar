//! Bounded first-use image reads too large for presentation-thread work.

use super::Source;
use crate::sources::LocalPack;
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
    input: Input,
    limit: u64,
    cancelled: Arc<AtomicBool>,
    done: Sender<Completed>,
}

enum Input {
    Archive(resource_pack::LayeredPackView, String),
    Local(Arc<dyn LocalPack>, String),
}

impl Job {
    /// Reads the image unless its owner has gone, and publishes the result to that owner.
    fn run(self) {
        if self.cancelled.load(Ordering::Acquire) {
            return;
        }
        let source = match &self.input {
            Input::Archive(view, path) => super::read_source(view, &self.key, path, self.limit),
            Input::Local(root, relative) => {
                super::local_sources::read(root.as_ref(), relative, self.limit, false).0
            }
        };
        if !self.cancelled.load(Ordering::Acquire) {
            let _ = self.done.try_send((self.key, source));
        }
    }
}

/// One image read and one queued input bound file IO, inflation and decryption process-wide.
fn worker() -> Option<&'static Sender<Job>> {
    static WORKER: OnceLock<Option<Sender<Job>>> = OnceLock::new();
    WORKER
        .get_or_init(|| {
            let (sender, receiver) = crossbeam_channel::bounded::<Job>(1);
            std::thread::Builder::new()
                .name("ui-image-sources".into())
                .spawn(move || {
                    while let Ok(job) = receiver.recv() {
                        job.run();
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
    pub(super) fn request(
        &self,
        key: &str,
        path: &str,
        view: &resource_pack::LayeredPackView,
        limit: u64,
    ) {
        self.request_input(key, Input::Archive(view.clone(), path.to_owned()), limit);
    }

    /// Queues the installed image's extension search, retaining fallback order on the worker.
    pub(super) fn request_local(
        &self,
        key: &str,
        root: &Arc<dyn LocalPack>,
        relative: &str,
        limit: u64,
    ) {
        self.request_input(
            key,
            Input::Local(Arc::clone(root), relative.to_owned()),
            limit,
        );
    }

    /// Holds one outstanding input, including requests awaiting queue capacity.
    fn request_input(&self, key: &str, input: Input, limit: u64) {
        let mut state = self.state.borrow_mut();
        if state.pending.is_some() || state.retry.is_some() {
            return;
        }
        let job = Job {
            key: key.to_owned(),
            input,
            limit,
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
        Self::submit_to(state, job, worker());
    }

    /// Applies queue backpressure without losing the pack's requested key. Without a worker
    /// thread, as in a browser, the read runs on the caller's frame and stays pending until
    /// collected, as a queued read does.
    fn submit_to(state: &mut State, job: Job, worker: Option<&Sender<Job>>) {
        let Some(worker) = worker else {
            state.pending = Some(job.key.clone());
            job.run();
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

#[cfg(test)]
mod tests {
    use super::*;

    /// An installed pack with no files.
    struct NoFiles;

    impl LocalPack for NoFiles {
        fn file_size(&self, _: &str) -> Option<u64> {
            None
        }

        fn read(&self, _: &str, _: u64) -> Option<Vec<u8>> {
            None
        }
    }

    /// Creates an unread local input for deterministic queue-lifecycle checks.
    fn job(reads: &SourceReads, key: &str) -> Job {
        Job {
            key: key.into(),
            input: Input::Local(Arc::new(NoFiles), "textures/fixture".into()),
            limit: 1,
            cancelled: Arc::clone(&reads.cancelled),
            done: reads.state.borrow().sender.clone(),
        }
    }

    #[test]
    fn full_source_queue_retains_retry_and_drop_cancels_the_owned_namespace() {
        let reads = SourceReads::default();
        let (sender, receiver) = crossbeam_channel::bounded(1);
        sender.try_send(job(&reads, "occupied")).ok().unwrap();
        let pending = job(&reads, "wanted");
        SourceReads::submit_to(&mut reads.state.borrow_mut(), pending, Some(&sender));
        assert!(reads.state.borrow().receiver.try_recv().is_err());
        receiver.try_recv().unwrap();
        let pending = reads.state.borrow_mut().retry.take().unwrap();
        SourceReads::submit_to(&mut reads.state.borrow_mut(), pending, Some(&sender));
        let accepted = receiver.try_recv().unwrap();
        assert_eq!(accepted.key, "wanted");
        drop(reads);
        assert!(accepted.cancelled.load(Ordering::Acquire));
        let replacement = SourceReads::default();
        assert!(replacement.collect().is_none());
        assert!(!replacement.cancelled.load(Ordering::Acquire));
    }

    #[test]
    fn stopped_source_worker_publishes_terminal_failure() {
        let reads = SourceReads::default();
        let (sender, receiver) = crossbeam_channel::bounded(1);
        drop(receiver);
        let pending = job(&reads, "failed");
        SourceReads::submit_to(&mut reads.state.borrow_mut(), pending, Some(&sender));
        let (key, source) = reads.collect().unwrap();
        assert_eq!(key, "failed");
        assert!(source.is_none());
        assert!(reads.collect().is_none());
    }

    #[test]
    fn inline_source_read_stays_pending_until_collected() {
        let reads = SourceReads::default();
        let first = job(&reads, "first");
        SourceReads::submit_to(&mut reads.state.borrow_mut(), first, None);
        assert_eq!(reads.state.borrow().pending.as_deref(), Some("first"));
        let files: Arc<dyn LocalPack> = Arc::new(NoFiles);
        reads.request_local("second", &files, "textures/fixture", 1);
        let (key, source) = reads.collect().unwrap();
        assert_eq!(key, "first");
        assert!(source.is_none());
        assert!(reads.state.borrow().pending.is_none());
        assert!(
            reads.collect().is_none(),
            "the sibling waited instead of reading"
        );
    }
}
