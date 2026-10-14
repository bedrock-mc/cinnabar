//! Completion requests for removal, after frame detachment and off-frame settings retirement.

use super::{
    Message, after_retirement,
    selection::{Receipt, SELECTION_CHANGED, same_path},
};
use crossbeam_channel::{Receiver, Sender, bounded};
use std::{
    path::{Path, PathBuf},
    sync::{
        Arc, Mutex,
        atomic::{AtomicU64, Ordering},
    },
};

pub(in crate::modding) struct Client {
    messages: Sender<Message>,
    latest: Arc<AtomicU64>,
    companions: Arc<Mutex<Vec<PathBuf>>>,
}

/// Keeps the applied generation attached until the library finishes deleting files.
pub(in crate::modding) struct Permit {
    generation: u64,
    latest: Arc<AtomicU64>,
    companions: Arc<Mutex<Vec<PathBuf>>>,
}

impl Permit {
    /// Binds deletion to the watcher generation whose hosts have finished retiring.
    pub(in crate::modding) fn new(
        generation: u64,
        latest: Arc<AtomicU64>,
        companions: Arc<Mutex<Vec<PathBuf>>>,
    ) -> Self {
        Self {
            generation,
            latest,
            companions,
        }
    }

    /// Protects companions still owned by any surviving runtime settings writer.
    pub(in crate::modding) fn shared_companion(&self, path: &Path) -> bool {
        let owners = self.companions.lock().expect("mod settings owners").clone();
        owners.iter().any(|owner| same_path(owner, path))
    }
    /// Rejects supersession even when a later selection restores the same registration bytes.
    pub(in crate::modding) fn confirm(&self) -> Result<(), String> {
        if self.latest.load(Ordering::Acquire) == self.generation {
            Ok(())
        } else {
            Err(SELECTION_CHANGED.into())
        }
    }
}

impl Client {
    /// Connects the library worker to the registration worker's retirement acknowledgments.
    pub(super) fn new(
        messages: Sender<Message>,
        latest: Arc<AtomicU64>,
        companions: Arc<Mutex<Vec<PathBuf>>>,
    ) -> Self {
        Self {
            messages,
            latest,
            companions,
        }
    }

    /// Waits off-frame for this unchanged selection to apply and retired hosts to finish writing.
    pub(in crate::modding) fn wait(&self, selection: &Receipt) -> Result<Permit, String> {
        let (complete, completion) = bounded(1);
        self.messages
            .send(Message::AwaitRetirement {
                selection: selection.clone(),
                complete,
            })
            .map_err(|_| "The mod host is unavailable; the mod was not removed.".to_owned())?;
        let generation = completion
            .recv()
            .unwrap_or_else(|_| Err("The mod host stopped before removal finished.".to_owned()))?;
        Ok(Permit::new(
            generation,
            Arc::clone(&self.latest),
            self.companions.clone(),
        ))
    }
}

pub(super) type Completion = Sender<Result<u64, String>>;

/// A fully applied removal and the exact registration it must still own after draining saves.
pub(super) struct Fence {
    generation: u64,
    selection: Receipt,
    complete: Completion,
}

#[derive(Default)]
pub(super) struct Pending {
    applied: Option<(u64, String)>,
    waiting: Option<(Receipt, Completion)>,
}

impl Pending {
    /// Rejects a removal superseded by a different selection instead of deleting live files.
    pub(super) fn observe(&mut self, request_id: Option<&str>) {
        if self
            .waiting
            .as_ref()
            .is_some_and(|(waiting, _)| Some(waiting.request_id.as_str()) != request_id)
            && let Some((_, complete)) = self.waiting.take()
        {
            let _ = complete.send(Err(SELECTION_CHANGED.into()));
        }
    }

    /// Records a full frame transition, distinct from an early authority-revocation notice.
    pub(super) fn applied(&mut self, generation: u64, request_id: String, fences: &Sender<Fence>) {
        self.applied = Some((generation, request_id.clone()));
        if self
            .waiting
            .as_ref()
            .is_some_and(|(waiting, _)| waiting.request_id == request_id)
            && let Some((selection, complete)) = self.waiting.take()
        {
            let _ = fences.send(Fence {
                generation,
                selection,
                complete,
            });
        }
    }

    /// Queues a retirement fence immediately only when this exact selection already applied.
    pub(super) fn wait(
        &mut self,
        generation: u64,
        selection: Receipt,
        complete: Completion,
        fences: &Sender<Fence>,
    ) {
        if self
            .applied
            .as_ref()
            .is_some_and(|(applied, id)| *applied == generation && *id == selection.request_id)
        {
            let _ = fences.send(Fence {
                generation,
                selection,
                complete,
            });
        } else if let Some((_, previous)) = self.waiting.replace((selection, complete)) {
            let _ = previous.send(Err("Another removal replaced this request.".into()));
        }
    }
}

/// Drains retired hosts, then rejects superseded generations or changed on-disk registrations.
pub(super) fn finish<T>(retired: &Receiver<T>, fence: Fence, directory: &Path, latest: &AtomicU64) {
    after_retirement(retired, || {
        let result = if fence.generation == latest.load(Ordering::Acquire) {
            fence
                .selection
                .confirm(directory)
                .map(|()| fence.generation)
        } else {
            Err(SELECTION_CHANGED.into())
        };
        let _ = fence.complete.send(result);
    });
}

#[cfg(test)]
mod tests;
