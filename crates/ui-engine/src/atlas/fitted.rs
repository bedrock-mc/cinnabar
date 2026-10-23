//! Bounded fitted-image jobs whose inputs and completions follow their atlas lifetime.

use super::Source;
use crossbeam_channel::{Receiver, Sender, TrySendError};
use std::sync::{
    Arc, OnceLock,
    atomic::{AtomicBool, Ordering},
};

type Completed = (String, Option<Vec<u8>>);

pub(super) struct Job {
    key: String,
    bytes: Arc<[u8]>,
    size: [u32; 2],
    cancelled: Arc<AtomicBool>,
    done: Sender<Completed>,
}

impl super::jobs::ImageJob for Job {
    /// Skips retired inputs and never blocks a shared worker while publishing pixels.
    fn run(self) {
        self.run_with(super::decode);
    }
}

impl Job {
    /// Checks lifetime before decode and again before publishing its owned result.
    fn run_with(self, decode: impl FnOnce(&[u8], [u32; 2]) -> Option<Vec<u8>>) {
        if self.cancelled.load(Ordering::Acquire) {
            return;
        }
        let pixels = decode(&self.bytes, self.size);
        if !self.cancelled.load(Ordering::Acquire) {
            let _ = self.done.try_send((self.key, pixels));
        }
    }
}

/// Shares one executing and one queued fitted decode across atlases.
fn worker() -> Option<&'static super::jobs::Worker<Job>> {
    static WORKER: OnceLock<Option<super::jobs::Worker<Job>>> = OnceLock::new();
    WORKER
        .get_or_init(|| super::jobs::Worker::new("ui-fitted-images"))
        .as_ref()
}

/// Runs one queued browser decode after the host yields.
#[cfg(target_arch = "wasm32")]
pub(crate) fn run_one() -> bool {
    worker().is_some_and(super::jobs::Worker::run_one)
}

/// One outstanding input or completion per atlas; dropping the owner cancels its queued work.
pub(super) struct FittedDecodes {
    pending: Option<String>,
    retry: Option<Job>,
    sender: Sender<Completed>,
    receiver: Receiver<Completed>,
    cancelled: Arc<AtomicBool>,
}

impl Default for FittedDecodes {
    fn default() -> Self {
        let (sender, receiver) = crossbeam_channel::bounded(1);
        Self {
            pending: None,
            retry: None,
            sender,
            receiver,
            cancelled: Arc::new(AtomicBool::new(false)),
        }
    }
}

impl Drop for FittedDecodes {
    fn drop(&mut self) {
        self.cancelled.store(true, Ordering::Release);
    }
}

impl FittedDecodes {
    /// Cancels this URL's input or completion without disturbing another URL's reservation.
    pub(super) fn remove(&mut self, key: &str) {
        if self.pending() == Some(key) {
            *self = Self::default();
        }
    }

    /// Keeps the reservation until its result is consumed, including time spent waiting to submit.
    pub(super) fn pending(&self) -> Option<&str> {
        self.pending.as_deref()
    }

    /// Admits one source without decoding inline or retaining later misses on backpressure.
    pub(super) fn request(&mut self, key: &str, source: &Source) {
        self.request_to(key, source, worker().map(super::jobs::Worker::sender));
    }

    /// Applies the same reservation policy to a supplied bounded worker queue.
    pub(super) fn request_to(&mut self, key: &str, source: &Source, sender: Option<&Sender<Job>>) {
        if self.pending.is_some() {
            return;
        }
        self.pending = Some(key.to_owned());
        let job = Job {
            key: key.to_owned(),
            bytes: Arc::clone(&source.bytes),
            size: source.packed,
            cancelled: Arc::clone(&self.cancelled),
            done: self.sender.clone(),
        };
        self.submit(job, sender);
    }

    /// Retries a retained input and releases its reservation only on terminal consumption.
    pub(super) fn collect(&mut self) -> Option<Completed> {
        if let Some(job) = self.retry.take() {
            self.submit(job, worker().map(super::jobs::Worker::sender));
        }
        let result = self.receiver.try_recv().ok()?;
        self.pending = None;
        Some(result)
    }

    /// Retains exactly one queued input or reports unavailable workers without blocking.
    fn submit(&mut self, job: Job, sender: Option<&Sender<Job>>) {
        let Some(sender) = sender else {
            let _ = job.done.try_send((job.key, None));
            return;
        };
        match sender.try_send(job) {
            Ok(()) => {}
            Err(TrySendError::Full(job)) => self.retry = Some(job),
            Err(TrySendError::Disconnected(job)) => {
                let _ = job.done.try_send((job.key, None));
            }
        }
    }
}

#[cfg(test)]
mod tests;
