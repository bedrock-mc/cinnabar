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
    fn outline(self) -> Option<Arc<SpriteMask>> {
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
                )
            }
            Self::Carrier { page, bounds } => outline(&page.rgba8, page.width, bounds),
        })
    }
}

/// Derives a source-local outline from a validated RGBA rectangle without copying its pixels.
fn outline(pixels: &[u8], stride: u32, [x, y, width, height]: [u32; 4]) -> Arc<SpriteMask> {
    Arc::new(SpriteMask::outline([width, height], |px, py| {
        pixels[((y + py) as usize * stride as usize + (x + px) as usize) * 4 + 3]
    }))
}

type Completed = (String, Option<Arc<SpriteMask>>);

struct Job {
    key: String,
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
        let outline = self.input.outline();
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
    ready: BTreeMap<String, Option<Arc<SpriteMask>>>,
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
    pub(super) fn needs(&self, key: &str) -> bool {
        !self.pending && !self.ready.contains_key(key)
    }

    /// Reads completed coverage without decoding, hashing or sampling any pixels.
    pub(super) fn get(&self, key: &str) -> Option<Arc<SpriteMask>> {
        self.ready.get(key).and_then(Clone::clone)
    }

    /// Schedules one source; explicitly inline hosts prepare it here, before painting.
    pub(super) fn request(&mut self, key: &str, input: Input, workers: bool) {
        if !self.needs(key) {
            return;
        }
        if !workers {
            self.ready.insert(key.to_owned(), input.outline());
            return;
        }
        self.pending = true;
        let job = Job {
            key: key.to_owned(),
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
        let Ok((key, mask)) = self.receiver.try_recv() else {
            return false;
        };
        self.ready.insert(key, mask);
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
