//! CPU source outlines prepared independently of GPU texture admission.

use std::{
    collections::BTreeMap,
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
type Completed = (Key, Option<Arc<SpriteMask>>);

struct Job {
    key: Key,
    input: Input,
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
            let _ = self.done.try_send((self.key, outline));
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
    ready: BTreeMap<String, BTreeMap<[u32; 2], Option<Arc<SpriteMask>>>>,
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
            ready: BTreeMap::new(),
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
            && self
                .ready
                .get(key)
                .is_none_or(|sizes| !sizes.contains_key(&size))
    }

    /// Reads completed coverage without decoding, hashing or sampling any pixels.
    pub(super) fn get(&self, key: &str, size: [u32; 2]) -> Option<Arc<SpriteMask>> {
        self.ready.get(key)?.get(&size).and_then(Clone::clone)
    }

    /// Schedules one draw-sized source; explicitly inline hosts also prepare it outside paint.
    pub(super) fn request(&mut self, key: &str, size: [u32; 2], input: Input, workers: bool) {
        if !self.needs(key, size) {
            return;
        }
        if !workers {
            self.ready
                .entry(key.to_owned())
                .or_default()
                .insert(size, input.outline(size));
            self.dirty = true;
            return;
        }
        self.pending = true;
        let job = Job {
            key: (key.to_owned(), size),
            input,
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
        let Ok(((key, size), mask)) = self.receiver.try_recv() else {
            return dirty;
        };
        self.ready.entry(key).or_default().insert(size, mask);
        self.pending = false;
        true
    }

    /// Retains one input on backpressure without running image work on the caller.
    fn submit(&mut self, job: Job) {
        let Some(worker) = worker() else {
            let _ = job.done.try_send((job.key, None));
            return;
        };
        match worker.sender().try_send(job) {
            Ok(()) => {}
            Err(TrySendError::Full(job)) => self.retry = Some(job),
            Err(TrySendError::Disconnected(job)) => {
                let _ = job.done.try_send((job.key, None));
            }
        }
    }
}
