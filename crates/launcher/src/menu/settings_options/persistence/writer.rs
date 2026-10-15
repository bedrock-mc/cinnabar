//! One disk owner batches edits without holding its mailbox during serialization or IO.

use super::super::SettingsOptions;
use std::{
    io,
    path::{Path, PathBuf},
    sync::{Arc, Condvar, Mutex},
    thread::{self, JoinHandle},
    time::{Duration, Instant},
};

/// Maximum batching delay from the first pending edit; later edits do not extend it.
const SAVE_DELAY: Duration = Duration::from_millis(100);
/// Failed writes retain the newest snapshot and wait before retrying.
pub const RETRY_DELAY: Duration = Duration::from_secs(1);

#[derive(Debug)]
struct Request {
    path: PathBuf,
    settings: Arc<SettingsOptions>,
}

#[derive(Debug, Default)]
struct State {
    pending: Option<Request>,
    due: Option<Instant>,
    completed: Option<Result<(), String>>,
    stopping: bool,
}

impl State {
    /// Replaces queued edits without moving an existing save or retry deadline.
    fn submit(&mut self, request: Request, now: Instant) {
        self.pending = Some(request);
        self.due.get_or_insert(now + SAVE_DELAY);
    }

    /// Takes the latest eligible snapshot, bypassing the batching delay at shutdown.
    fn take_due(&mut self, now: Instant) -> Option<Request> {
        if !self.stopping && self.due.is_none_or(|due| now < due) {
            return None;
        }
        self.due = None;
        self.pending.take()
    }

    /// Publishes the latest result and retains failed work beneath any newer edits.
    fn complete(&mut self, request: Request, result: io::Result<()>, now: Instant) {
        if result.is_err() && !self.stopping {
            self.pending.get_or_insert(request);
            self.due = Some(now + RETRY_DELAY);
        }
        self.completed = Some(result.map_err(|error| error.to_string()));
    }
}

#[derive(Debug, Default)]
struct Mailbox {
    state: Mutex<State>,
    wake: Condvar,
}

/// Bounded latest-value mailbox with one worker; dropping it drains pending settings.
#[derive(Debug)]
pub struct Writer {
    mailbox: Arc<Mailbox>,
    worker: Option<JoinHandle<()>>,
}

impl Writer {
    /// Starts the settings disk owner, keeping the existing atomic file replacement.
    pub fn new() -> io::Result<Self> {
        Self::with_saver(|settings, path| settings.save(path))
    }

    /// Starts a disk owner with a supplied save operation, also used by storage tests.
    fn with_saver(
        mut save: impl FnMut(&SettingsOptions, &Path) -> io::Result<()> + Send + 'static,
    ) -> io::Result<Self> {
        let mailbox = Arc::new(Mailbox::default());
        let shared = Arc::clone(&mailbox);
        let worker = thread::Builder::new()
            .name("settings-writer".into())
            .spawn(move || {
                loop {
                    let mut state = shared.state.lock().unwrap_or_else(|p| p.into_inner());
                    let request = loop {
                        let now = Instant::now();
                        if let Some(request) = state.take_due(now) {
                            break request;
                        }
                        if state.stopping {
                            return;
                        }
                        state = if let Some(due) = state.due {
                            shared
                                .wake
                                .wait_timeout(state, due.saturating_duration_since(now))
                                .unwrap_or_else(|p| p.into_inner())
                                .0
                        } else {
                            shared.wake.wait(state).unwrap_or_else(|p| p.into_inner())
                        };
                    };
                    drop(state);
                    let result = save(&request.settings, &request.path);
                    let mut state = shared.state.lock().unwrap_or_else(|p| p.into_inner());
                    state.complete(request, result, Instant::now());
                }
            })?;
        Ok(Self {
            mailbox,
            worker: Some(worker),
        })
    }

    /// Hands off a snapshot without waiting for serialization or filesystem work.
    pub fn submit(&mut self, path: PathBuf, settings: Arc<SettingsOptions>) -> io::Result<()> {
        if self.worker.as_ref().is_none_or(JoinHandle::is_finished) {
            return Err(io::Error::other("settings writer stopped"));
        }
        self.mailbox
            .state
            .lock()
            .unwrap_or_else(|p| p.into_inner())
            .submit(Request { path, settings }, Instant::now());
        self.mailbox.wake.notify_one();
        Ok(())
    }

    /// Takes the newest completion without waiting for storage; failed work remains queued.
    pub fn poll(&mut self) -> Option<Result<(), String>> {
        self.mailbox
            .state
            .lock()
            .unwrap_or_else(|p| p.into_inner())
            .completed
            .take()
    }

    /// Stops accepting work and waits for the latest snapshot, without a retry or batching delay.
    pub fn finish(&mut self) -> io::Result<()> {
        self.mailbox
            .state
            .lock()
            .unwrap_or_else(|p| p.into_inner())
            .stopping = true;
        self.mailbox.wake.notify_one();
        if let Some(worker) = self.worker.take() {
            worker
                .join()
                .map_err(|_| io::Error::other("settings writer panicked"))?;
        }
        self.poll().unwrap_or(Ok(())).map_err(io::Error::other)
    }
}

impl Drop for Writer {
    /// Flushes accepted settings before releasing their disk owner.
    fn drop(&mut self) {
        let _ = self.finish();
    }
}

#[cfg(test)]
mod tests;
