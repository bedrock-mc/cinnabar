//! Bounded appearance-worker preparation of cape pixels for visible player draws.
mod preparation;
#[cfg(test)]
mod tests;

use preparation::WorkerCache;
use render_api::{CapeImage, SkinRgba8};
use std::{
    collections::HashMap,
    sync::{
        Arc, Mutex,
        atomic::{AtomicBool, Ordering},
        mpsc,
    },
};

// A visible population and its pending replacements can coexist.
const MAX_SOURCES: usize = render_model::MAX_RENDERED_PLAYERS * 2;
const SOURCE_BYTES: usize = protocol::MAX_PLAYER_LIST_SKIN_BYTES * 2;
type SourceId = (usize, u32, u32);

#[derive(Debug)]
struct Entry {
    source: CapeImage,
    outcome: Option<Option<SkinRgba8>>,
    seen: u64,
}

#[derive(Debug)]
struct Completed {
    cache: WorkerCache,
    sources: Vec<(CapeImage, Option<SkinRgba8>)>,
}

/// Retains source identities on the caller and prepares content on the appearance worker pool.
#[derive(Debug)]
pub struct CapePreparationQueue {
    entries: HashMap<SourceId, Entry>,
    queued: Vec<CapeImage>,
    cache: Option<WorkerCache>,
    sender: mpsc::Sender<Completed>,
    receiver: Mutex<mpsc::Receiver<Completed>>,
    cancelled: Arc<AtomicBool>,
    frame: u64,
    source_bytes: usize,
}

impl Default for CapePreparationQueue {
    /// Gives each presentation owner its own cache and completion channel.
    fn default() -> Self {
        let (sender, receiver) = mpsc::channel();
        Self {
            entries: HashMap::new(),
            queued: Vec::new(),
            cache: Some(WorkerCache::default()),
            sender,
            receiver: Mutex::new(receiver),
            cancelled: Arc::new(AtomicBool::new(false)),
            frame: 0,
            source_bytes: 0,
        }
    }
}

impl Drop for CapePreparationQueue {
    /// Cancels future sources and retires retained pixels on a worker.
    fn drop(&mut self) {
        self.cancelled.store(true, Ordering::Relaxed);
        let entries = std::mem::take(&mut self.entries);
        let queued = std::mem::take(&mut self.queued);
        let cache = self.cache.take();
        if !entries.is_empty()
            || !queued.is_empty()
            || cache.as_ref().is_none_or(|cache| !cache.is_empty())
        {
            let (_, idle) = mpsc::channel();
            let receiver = std::mem::replace(&mut self.receiver, Mutex::new(idle));
            rayon::spawn(move || drop((entries, queued, cache, receiver)));
        }
    }
}

impl CapePreparationQueue {
    /// Receives one bounded completion without waiting and retires sources absent from recent draws.
    pub fn begin_frame(&mut self) {
        self.frame = self.frame.wrapping_add(1);
        if self.cache.is_none()
            && let Ok(completed) = self
                .receiver
                .get_mut()
                .unwrap_or_else(std::sync::PoisonError::into_inner)
                .try_recv()
        {
            self.complete(completed);
        }
        let oldest = self.frame.saturating_sub(2);
        let mut retired = Vec::new();
        self.entries.retain(|_, entry| {
            if entry.seen >= oldest || entry.outcome.is_none() {
                return true;
            }
            self.source_bytes -= entry.source.rgba8.len();
            retired.push((entry.source.clone(), entry.outcome.take()));
            false
        });
        if !retired.is_empty() {
            rayon::spawn(move || drop(retired));
        }
    }

    /// Returns ready pixels or admits a bounded cold source without hashing or resampling inline.
    pub fn request(&mut self, source: &CapeImage) -> Option<SkinRgba8> {
        let id = source_id(source);
        if let Some(entry) = self.entries.get_mut(&id) {
            entry.seen = self.frame;
            return entry.outcome.clone().flatten();
        }
        if self.queued.len() >= super::skin::queue::MAX_SKIN_PREPARATIONS_PER_PASS
            || self.entries.len() >= MAX_SOURCES
            || self.source_bytes.saturating_add(source.rgba8.len()) > SOURCE_BYTES
        {
            return None;
        }
        self.source_bytes += source.rgba8.len();
        self.entries.insert(
            id,
            Entry {
                source: source.clone(),
                outcome: None,
                seen: self.frame,
            },
        );
        self.queued.push(source.clone());
        None
    }

    /// Moves a batch and its content cache to Rayon; preparation holds no mutex.
    pub fn submit(&mut self) {
        if self.queued.is_empty() || self.cache.is_none() {
            return;
        }
        let mut cache = self.cache.take().expect("idle cape worker owns the cache");
        let queued = std::mem::take(&mut self.queued);
        let cancelled = Arc::clone(&self.cancelled);
        let sender = self.sender.clone();
        rayon::spawn(move || {
            let _span =
                tracing::info_span!("actor.cape_prepare_batch", sources = queued.len()).entered();
            let mut sources = Vec::with_capacity(queued.len());
            for source in queued {
                if cancelled.load(Ordering::Relaxed) {
                    return;
                }
                let pixels = cache.prepare(&source);
                sources.push((source, pixels));
            }
            let _ = sender.send(Completed { cache, sources });
        });
    }

    /// Publishes only retained source identities; actor and animation state stay with presentation.
    fn complete(&mut self, completed: Completed) {
        self.cache = Some(completed.cache);
        for (source, outcome) in completed.sources {
            if let Some(entry) = self.entries.get_mut(&source_id(&source)) {
                entry.outcome = Some(outcome);
            }
        }
    }

    /// Tests await their own submitted work without making wall-clock assertions.
    #[cfg(any(test, feature = "test-support"))]
    pub fn finish_fixture_batch(&mut self) {
        if self.cache.is_none() {
            let completed = self.receiver.get_mut().unwrap().recv().unwrap();
            self.complete(completed);
        }
    }
}

/// Retaining the source prevents allocation reuse; dimensions distinguish shared raster views.
fn source_id(source: &CapeImage) -> SourceId {
    (source.rgba8.as_ptr() as usize, source.width, source.height)
}
