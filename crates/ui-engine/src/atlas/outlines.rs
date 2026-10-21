//! CPU source outlines prepared independently of GPU texture admission.

use std::{
    cell::RefCell,
    collections::VecDeque,
    sync::{
        Arc, OnceLock,
        atomic::{AtomicBool, Ordering},
    },
};

use crossbeam_channel::{Receiver, Sender, TrySendError};
use render_api::sprite_mask::SpriteMask;

use super::jobs::ImageJob;

pub(super) enum Input {
    Encoded(super::Source),
    Carrier {
        page: assets::UiAtlasPage,
        bounds: [u32; 4],
    },
}

impl Input {
    /// Reads source dimensions without decoding its pixels.
    fn size(&self) -> [u32; 2] {
        match self {
            Self::Encoded(source) => source.size,
            Self::Carrier { bounds, .. } => [bounds[2], bounds[3]],
        }
    }

    /// Derives alpha coverage on the image worker, retaining no decoded source pixels.
    fn outline(self, size: [u32; 2]) -> Option<Arc<SpriteMask>> {
        Some(match self {
            Self::Encoded(source) => {
                let mut reader = super::reader(&source.bytes)?;
                let mut limits = image::Limits::default();
                limits.max_image_width = Some(super::MAX_SOURCE_SIDE);
                limits.max_image_height = Some(super::MAX_SOURCE_SIDE);
                limits.max_alloc = Some(render_model::MAX_UI_NATIVE_SHEET_BYTES as u64);
                reader.limits(limits);
                let image = reader.decode().ok()?.into_rgba8();
                (image.dimensions() == (source.size[0], source.size[1])).then_some(())?;
                outline(
                    image.as_raw(),
                    source.size[0],
                    [0, 0, source.size[0], source.size[1]],
                    size,
                )
            }
            Self::Carrier { page, bounds } => outline(&page.rgba8, page.width, bounds, size),
        })
    }
}

/// Derives a source-local outline from a validated RGBA rectangle without copying its pixels.
fn outline(
    pixels: &[u8],
    stride: u32,
    [x, y, width, height]: [u32; 4],
    size: [u32; 2],
) -> Arc<SpriteMask> {
    Arc::new(SpriteMask::outline([width, height], size, |px, py| {
        pixels[((y + py) as usize * stride as usize + (x + px) as usize) * 4 + 3]
    }))
}

type Key = (String, [u32; 2]);
type Completed = (Key, [u32; 2], Option<Arc<SpriteMask>>);
const MAX_READY: usize = 8;

struct Ready {
    key: Key,
    source_size: [u32; 2],
    mask: Option<Arc<SpriteMask>>,
}

impl Ready {
    /// Matches requests that produce the same clamped source grid.
    fn matches(&self, key: &str, size: [u32; 2]) -> bool {
        self.key.0 == key && self.key.1 == SpriteMask::grid_size(self.source_size, size)
    }
}

struct Job {
    key: Key,
    input: Input,
    source_size: [u32; 2],
    cancelled: Arc<AtomicBool>,
    done: Sender<Completed>,
}

impl ImageJob for Job {
    /// Prepares source coverage off the paint path and discards retired pack results.
    fn run(self) {
        if self.cancelled.load(Ordering::Acquire) {
            return;
        }
        let outline = self.input.outline(self.key.1);
        if !self.cancelled.load(Ordering::Acquire) {
            let _ = self.done.try_send((self.key, self.source_size, outline));
        }
    }
}

/// Shares one running decode and one queued source across all atlases.
fn worker() -> Option<&'static super::jobs::Worker<Job>> {
    static WORKER: OnceLock<Option<super::jobs::Worker<Job>>> = OnceLock::new();
    WORKER
        .get_or_init(|| super::jobs::Worker::new("ui-source-outlines"))
        .as_ref()
}

/// Runs one browser outline job after the host yields.
#[cfg(target_arch = "wasm32")]
pub(crate) fn run_one() -> bool {
    worker().is_some_and(super::jobs::Worker::run_one)
}

/// Keeps ready masks and one pending input for the atlas's source lifetime.
pub(super) struct Outlines {
    ready: RefCell<VecDeque<Ready>>,
    dirty: bool,
    pending: bool,
    retry: Option<Job>,
    sender: Sender<Completed>,
    receiver: Receiver<Completed>,
    cancelled: Arc<AtomicBool>,
}

impl Default for Outlines {
    fn default() -> Self {
        let (sender, receiver) = crossbeam_channel::bounded(1);
        Self {
            ready: RefCell::default(),
            dirty: false,
            pending: false,
            retry: None,
            sender,
            receiver,
            cancelled: Arc::new(AtomicBool::new(false)),
        }
    }
}

impl Drop for Outlines {
    fn drop(&mut self) {
        self.cancelled.store(true, Ordering::Release);
    }
}

impl Outlines {
    /// Avoids resolving inputs already cached or while this atlas has outstanding work.
    pub(super) fn needs(&self, key: &str, size: [u32; 2]) -> bool {
        !self.pending
            && !self
                .ready
                .borrow()
                .iter()
                .any(|entry| entry.matches(key, size))
    }

    /// Reads completed coverage and marks its actual grid as recently used.
    pub(super) fn get(&self, key: &str, size: [u32; 2]) -> Option<Arc<SpriteMask>> {
        let mut ready = self.ready.borrow_mut();
        let index = ready.iter().position(|entry| entry.matches(key, size))?;
        let entry = ready.remove(index)?;
        let mask = entry.mask.clone();
        ready.push_back(entry);
        mask
    }

    /// Retains a small LRU of actual grids, releasing old resolutions after GUI scale changes.
    fn insert(&mut self, (key, source_size, mask): Completed) {
        let ready = self.ready.get_mut();
        if ready.len() == MAX_READY {
            ready.pop_front();
        }
        ready.push_back(Ready {
            key,
            source_size,
            mask,
        });
    }

    /// Schedules one draw-sized source; explicitly inline hosts also prepare it outside paint.
    pub(super) fn request(&mut self, key: &str, size: [u32; 2], input: Input, workers: bool) {
        if !self.needs(key, size) {
            return;
        }
        let source_size = input.size();
        let size = SpriteMask::grid_size(source_size, size);
        if !workers {
            self.insert(((key.to_owned(), size), source_size, input.outline(size)));
            self.dirty = true;
            return;
        }
        self.pending = true;
        let job = Job {
            key: (key.to_owned(), size),
            input,
            source_size,
            cancelled: self.cancelled.clone(),
            done: self.sender.clone(),
        };
        self.submit(job);
    }

    /// Publishes worker results before retained frames decide whether they need repainting.
    pub(super) fn collect(&mut self) -> bool {
        if let Some(job) = self.retry.take() {
            self.submit(job);
        }
        let dirty = std::mem::take(&mut self.dirty);
        let Ok(completed) = self.receiver.try_recv() else {
            return dirty;
        };
        self.insert(completed);
        self.pending = false;
        true
    }

    /// Retains one input on backpressure without running image work on the caller.
    fn submit(&mut self, job: Job) {
        let Some(worker) = worker() else {
            let _ = job.done.try_send((job.key, job.source_size, None));
            return;
        };
        match worker.sender().try_send(job) {
            Ok(()) => {}
            Err(TrySendError::Full(job)) => self.retry = Some(job),
            Err(TrySendError::Disconnected(job)) => {
                let _ = job.done.try_send((job.key, job.source_size, None));
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn outline_cache_reuses_clamped_grids_and_bounds_scale_history() {
        let page = assets::UiAtlasPage {
            width: 72,
            height: 72,
            rgba8: vec![255; 72 * 72 * 4].into(),
        };
        let input = || Input::Carrier {
            page: page.clone(),
            bounds: [0, 0, 72, 72],
        };
        let mut cache = Outlines::default();
        cache.request("icon", [16; 2], input(), false);
        let hot = cache.get("icon", [16; 2]).unwrap();
        cache.request("icon", [17; 2], input(), false);
        let cold = Arc::downgrade(&cache.get("icon", [17; 2]).unwrap());
        let mut history = Vec::new();
        for scale in 18..=72 {
            cache.request("icon", [scale; 2], input(), false);
            history.push(Arc::downgrade(&cache.get("icon", [scale; 2]).unwrap()));
            assert!(Arc::ptr_eq(&hot, &cache.get("icon", [16; 2]).unwrap()));
        }
        assert!(
            cold.upgrade().is_none(),
            "old GUI scales must release their buffers"
        );
        assert!(
            history
                .iter()
                .filter(|mask| mask.upgrade().is_some())
                .count()
                < MAX_READY
        );
        let full = cache.get("icon", [72; 2]).unwrap();
        for scale in (73..=2048).rev() {
            cache.request("icon", [scale; 2], input(), false);
            assert!(Arc::ptr_eq(&full, &cache.get("icon", [scale; 2]).unwrap()));
        }
        assert!(Arc::ptr_eq(&hot, &cache.get("icon", [1; 2]).unwrap()));
        cache.request("icon", [17; 2], input(), false);
        assert!(
            cache.get("icon", [17; 2]).is_some(),
            "an evicted scale can be prepared again"
        );
    }
}
