//! Original-resolution virtual images, decoded once into bounded immutable texture tiles.

use std::{
    collections::BTreeSet,
    sync::{
        Arc, OnceLock,
        atomic::{AtomicBool, Ordering},
    },
};

use crossbeam_channel::{Receiver, Sender, TrySendError};
use image::Limits;
use json_ui::UvRect;
use render_model::{
    MAX_UI_NATIVE_SHEET_BYTES, MAX_UI_NATIVE_SHEET_PAGES, MAX_UI_NATIVE_SHEET_SIDE,
    MAX_UI_TEXTURE_SIDE, UiTexturePage,
};

use super::Source;

#[cfg(test)]
mod tests;

/// Neighbor texels overlap tile borders so bilinear filtering crosses them unchanged.
const INTERIOR: u32 = MAX_UI_TEXTURE_SIDE - 2;

/// Whether the complete source fits one tile without an internal sampling boundary.
pub(super) fn fits_single_tile(size: [u32; 2]) -> bool {
    size.into_iter().all(|side| side <= INTERIOR)
}

#[derive(Clone, Copy)]
struct TileSpec {
    origin: [u32; 2],
    content: [u32; 4],
    size: [u32; 2],
}

struct Plan {
    size: [u32; 2],
    tiles: Vec<TileSpec>,
    bytes: usize,
}

impl Plan {
    /// Checks every decoded and tiled allocation before scheduling an image.
    fn new(size: [u32; 2]) -> Option<Self> {
        let [width, height] = size;
        if width == 0
            || height == 0
            || width > MAX_UI_NATIVE_SHEET_SIDE
            || height > MAX_UI_NATIVE_SHEET_SIDE
        {
            return None;
        }
        let decoded = u64::from(width) * u64::from(height) * 4;
        if decoded > MAX_UI_NATIVE_SHEET_BYTES as u64 {
            return None;
        }
        let mut tiles = Vec::new();
        let mut bytes = 0usize;
        for y in (0..height).step_by(INTERIOR as usize) {
            for x in (0..width).step_by(INTERIOR as usize) {
                let right = (x + INTERIOR).min(width);
                let bottom = (y + INTERIOR).min(height);
                let origin = [x.saturating_sub(1), y.saturating_sub(1)];
                let size = [
                    (right + 1).min(width) - origin[0],
                    (bottom + 1).min(height) - origin[1],
                ];
                bytes = bytes.checked_add(size[0] as usize * size[1] as usize * 4)?;
                if bytes > MAX_UI_NATIVE_SHEET_BYTES || tiles.len() >= MAX_UI_NATIVE_SHEET_PAGES {
                    return None;
                }
                tiles.push(TileSpec {
                    origin,
                    content: [x, y, right, bottom],
                    size,
                });
            }
        }
        Some(Self { size, tiles, bytes })
    }
}

struct Tile {
    spec: TileSpec,
    page: UiTexturePage,
}

pub(super) struct Sheet {
    size: [u32; 2],
    tiles: Vec<Tile>,
    bytes: usize,
}

/// A UV region expressed against one tile and its matching destination fragment.
pub struct Quad {
    pub page: usize,
    pub uv: [f32; 4],
    pub dest: [f32; 4],
    /// The complete authored source bounds in this page's texels, before page clipping.
    pub repeat_source: [f32; 4],
}

/// Two endpoints plus each possible source and tile edge bound the sampling work.
const MAX_AXIS_CUTS: usize = MAX_UI_NATIVE_SHEET_SIDE.div_ceil(INTERIOR) as usize + 3;

#[derive(Clone, Copy, Default)]
struct Axis {
    dest: [f32; 2],
    sample: [f32; 2],
    middle: f32,
}

#[derive(Default)]
struct Segments {
    values: [Axis; MAX_AXIS_CUTS - 1],
    len: usize,
}

/// Splits sampling at tile and source edges, retaining mirroring and clamp-to-edge spans.
fn axis(start: f32, end: f32, extent: u32) -> Option<Segments> {
    if !start.is_finite() || !end.is_finite() {
        return None;
    }
    let limit = extent as f32;
    let mut pieces = Segments::default();
    if start == end {
        let sample = start.clamp(0.0, limit);
        pieces.values[0] = Axis {
            dest: [0.0, 1.0],
            sample: [sample; 2],
            middle: sample,
        };
        pieces.len = 1;
        return Some(pieces);
    }
    let lower = start.min(end);
    let upper = start.max(end);
    let mut cuts = [0.0; MAX_AXIS_CUTS];
    cuts[..2].copy_from_slice(&[lower, upper]);
    let mut len = 2;
    for edge in (0..extent)
        .step_by(INTERIOR as usize)
        .chain(std::iter::once(extent))
    {
        let edge = edge as f32;
        if edge > lower && edge < upper {
            *cuts.get_mut(len)? = edge;
            len += 1;
        }
    }
    cuts[..len].sort_unstable_by(f32::total_cmp);
    for (index, edges) in cuts[..len].windows(2).enumerate() {
        let mut a = edges[0];
        let mut b = edges[1];
        if end < start {
            std::mem::swap(&mut a, &mut b);
        }
        pieces.values[index] = Axis {
            dest: [(a - start) / (end - start), (b - start) / (end - start)],
            sample: [a.clamp(0.0, limit), b.clamp(0.0, limit)],
            middle: ((a + b) * 0.5).clamp(0.0, limit),
        };
    }
    pieces.len = len - 1;
    Some(pieces)
}

/// Samples resident pages lazily with bounded stack storage for both axes.
pub struct Quads<'a> {
    sheet: &'a Sheet,
    xs: Segments,
    ys: Segments,
    x: usize,
    y: usize,
    dest: [f32; 4],
    first: usize,
    source: [f32; 4],
}

impl Quads<'_> {
    /// Rebases tile references into the presentation's logical texture catalog.
    pub fn rebased(mut self, first: usize) -> Self {
        self.first += first;
        self
    }
}

impl Iterator for Quads<'_> {
    type Item = Quad;

    fn next(&mut self) -> Option<Self::Item> {
        if self.xs.len == 0 {
            return None;
        }
        while self.y < self.ys.len {
            let x = self.xs.values[self.x];
            let y = self.ys.values[self.y];
            self.x += 1;
            if self.x == self.xs.len {
                self.x = 0;
                self.y += 1;
            }
            let Some((index, tile)) = self.sheet.tiles.iter().enumerate().find(|(_, tile)| {
                let [left, top, right, bottom] = tile.spec.content;
                x.middle >= left as f32
                    && (x.middle < right as f32 || right == self.sheet.size[0])
                    && y.middle >= top as f32
                    && (y.middle < bottom as f32 || bottom == self.sheet.size[1])
            }) else {
                continue;
            };
            let [left, top] = tile.spec.origin.map(|side| side as f32);
            return Some(Quad {
                page: self.first + index,
                uv: [
                    x.sample[0] - left,
                    y.sample[0] - top,
                    x.sample[1] - left,
                    y.sample[1] - top,
                ],
                dest: [
                    self.dest[0] + x.dest[0] * (self.dest[2] - self.dest[0]),
                    self.dest[1] + y.dest[0] * (self.dest[3] - self.dest[1]),
                    self.dest[0] + x.dest[1] * (self.dest[2] - self.dest[0]),
                    self.dest[1] + y.dest[1] * (self.dest[3] - self.dest[1]),
                ],
                repeat_source: [
                    self.source[0] - left,
                    self.source[1] - top,
                    self.source[2] - left,
                    self.source[3] - top,
                ],
            });
        }
        None
    }
}

impl Sheet {
    /// Maps the authored source rectangle without resampling or changing its animation time.
    pub(super) fn quads(&self, uv: UvRect, dest: [f32; 4], first: usize) -> Quads<'_> {
        let [width, height] = self.size.map(|side| side as f32);
        Quads {
            sheet: self,
            xs: axis(uv.u0 * width, uv.u1 * width, self.size[0]).unwrap_or_default(),
            ys: axis(uv.v0 * height, uv.v1 * height, self.size[1]).unwrap_or_default(),
            x: 0,
            y: 0,
            dest,
            first,
            source: [uv.u0 * width, uv.v0 * height, uv.u1 * width, uv.v1 * height],
        }
    }
}

type Completed = (String, Option<Sheet>);

struct Job {
    key: String,
    bytes: Arc<[u8]>,
    plan: Plan,
    cancelled: Arc<AtomicBool>,
    done: Sender<Completed>,
}

impl super::jobs::ImageJob for Job {
    /// Decodes one bounded sheet and publishes only while its pack still owns it.
    fn run(self) {
        if self.cancelled.load(Ordering::Acquire) {
            return;
        }
        let sheet = decode(&self.bytes, &self.plan, &self.cancelled);
        if !self.cancelled.load(Ordering::Acquire) {
            let _ = self.done.try_send((self.key, sheet));
        }
    }
}

/// One process-wide decode and one queued input bound memory even during rapid pack reloads.
fn worker() -> Option<&'static super::jobs::Worker<Job>> {
    static WORKER: OnceLock<Option<super::jobs::Worker<Job>>> = OnceLock::new();
    WORKER
        .get_or_init(|| super::jobs::Worker::new("ui-native-sheets"))
        .as_ref()
}

/// Drains one browser decode without blocking on an empty queue.
#[cfg(target_arch = "wasm32")]
pub(crate) fn run_one() -> bool {
    worker().is_some_and(super::jobs::Worker::run_one)
}

/// Decodes on the worker or inline, releasing the temporary image once texels are tiled.
fn decode(bytes: &[u8], plan: &Plan, cancelled: &AtomicBool) -> Option<Sheet> {
    (bytes.len() as u64 <= resource_pack::MAX_FILE_BYTES).then_some(())?;
    let mut reader = super::reader(bytes)?;
    let mut limits = Limits::default();
    limits.max_image_width = Some(MAX_UI_NATIVE_SHEET_SIDE);
    limits.max_image_height = Some(MAX_UI_NATIVE_SHEET_SIDE);
    limits.max_alloc = Some(MAX_UI_NATIVE_SHEET_BYTES as u64);
    reader.limits(limits);
    let image = reader.decode().ok()?.into_rgba8();
    (image.dimensions() == (plan.size[0], plan.size[1])).then_some(())?;
    let mut tiles = Vec::with_capacity(plan.tiles.len());
    for spec in &plan.tiles {
        if cancelled.load(Ordering::Acquire) {
            return None;
        }
        let pixels = image::imageops::crop_imm(
            &image,
            spec.origin[0],
            spec.origin[1],
            spec.size[0],
            spec.size[1],
        )
        .to_image()
        .into_raw();
        tiles.push(Tile {
            spec: *spec,
            page: UiTexturePage::owned(spec.size, pixels.into())
                .ok()?
                .with_repeat_content([
                    spec.content[0] - spec.origin[0],
                    spec.content[1] - spec.origin[1],
                    spec.content[2] - spec.origin[0],
                    spec.content[3] - spec.origin[1],
                ])
                .ok()?,
        });
    }
    Some(Sheet {
        size: plan.size,
        tiles,
        bytes: plan.bytes,
    })
}

pub(super) struct NativeSheets {
    loaded: Vec<(String, Sheet)>,
    failed: BTreeSet<String>,
    pending: Option<String>,
    retry: Option<Job>,
    sender: Sender<Completed>,
    receiver: Receiver<Completed>,
    cancelled: Arc<AtomicBool>,
    pages: Vec<UiTexturePage>,
    dirty: bool,
}

impl Default for NativeSheets {
    fn default() -> Self {
        let (sender, receiver) = crossbeam_channel::bounded(1);
        Self {
            loaded: Vec::new(),
            failed: BTreeSet::new(),
            pending: None,
            retry: None,
            sender,
            receiver,
            cancelled: Arc::new(AtomicBool::new(false)),
            pages: Vec::new(),
            dirty: false,
        }
    }
}

impl Drop for NativeSheets {
    fn drop(&mut self) {
        self.cancelled.store(true, Ordering::Release);
    }
}

impl NativeSheets {
    /// Whether a sheet, failure or pending job still depends on this URL's generation.
    pub(super) fn contains(&self, key: &str) -> bool {
        self.loaded.iter().any(|(name, _)| name == key)
            || self.failed.contains(key)
            || self.pending.as_deref() == Some(key)
            || self.retry.as_ref().is_some_and(|job| job.key == key)
    }

    /// Retires one URL before painting; unrelated sheets keep their immutable pixels.
    pub(super) fn remove(&mut self, key: &str) {
        if self.pending.as_deref() == Some(key)
            || self.retry.as_ref().is_some_and(|job| job.key == key)
        {
            self.cancelled.store(true, Ordering::Release);
            (self.sender, self.receiver) = crossbeam_channel::bounded(1);
            self.cancelled = Arc::new(AtomicBool::new(false));
            self.pending = None;
            self.retry = None;
        }
        self.failed.remove(key);
        let count = self.loaded.len();
        self.loaded.retain(|(name, _)| name != key);
        if self.loaded.len() != count {
            self.pages = self
                .loaded
                .iter()
                .flat_map(|(_, sheet)| sheet.tiles.iter().map(|tile| tile.page.clone()))
                .collect();
            self.dirty = true;
        }
    }

    /// Publishes completed immutable pages; pack replacement drops the whole namespace.
    pub(super) fn collect(&mut self) -> bool {
        let mut completed = false;
        while let Ok((key, sheet)) = self.receiver.try_recv() {
            completed = true;
            self.pending = None;
            self.publish(key, sheet);
        }
        if let Some(job) = self.retry.take() {
            if let Some(worker) = worker() {
                self.submit(worker.sender(), job);
            } else {
                self.fail(job.key);
                completed = true;
            }
        }
        completed
    }

    /// Requests one original image while keeping decoded bytes and page count within the pack
    /// budget; without `workers` it decodes on this thread.
    pub(super) fn request(&mut self, key: &str, source: &Source, workers: bool) {
        if self.loaded.iter().any(|(name, _)| name == key)
            || self.failed.contains(key)
            || self.pending.is_some()
            || self.retry.is_some()
        {
            return;
        }
        let Some(plan) = Plan::new(source.size) else {
            self.fail(key.to_owned());
            return;
        };
        let bytes: usize = self.loaded.iter().map(|(_, sheet)| sheet.bytes).sum();
        if self.pages.len() + plan.tiles.len() > MAX_UI_NATIVE_SHEET_PAGES
            || bytes + plan.bytes > MAX_UI_NATIVE_SHEET_BYTES
        {
            self.fail(key.to_owned());
            return;
        }
        if !workers {
            let sheet = decode(&source.bytes, &plan, &self.cancelled);
            self.publish(key.to_owned(), sheet);
            return;
        }
        let Some(worker) = worker() else {
            self.fail(key.to_owned());
            return;
        };
        let job = Job {
            key: key.to_owned(),
            bytes: Arc::clone(&source.bytes),
            plan,
            cancelled: Arc::clone(&self.cancelled),
            done: self.sender.clone(),
        };
        self.submit(worker.sender(), job);
    }

    /// Appends a decoded sheet's pages after those already published, or records its failure.
    fn publish(&mut self, key: String, sheet: Option<Sheet>) {
        if let Some(sheet) = sheet {
            self.pages
                .extend(sheet.tiles.iter().map(|tile| tile.page.clone()));
            self.loaded.push((key, sheet));
            self.dirty = true;
        } else {
            self.fail(key);
        }
    }

    /// Retains one bounded request when another pack occupies the shared decode queue.
    fn submit(&mut self, worker: &Sender<Job>, job: Job) {
        let key = job.key.clone();
        match worker.try_send(job) {
            Ok(()) => self.pending = Some(key),
            Err(TrySendError::Full(job)) => self.retry = Some(job),
            Err(TrySendError::Disconnected(_)) => self.fail(key),
        }
    }

    /// Counts an unusable or over-budget image once while retaining a bounded failure set.
    fn fail(&mut self, key: String) {
        if self.failed.len() >= super::MAX_FAILED {
            self.failed.pop_first();
        }
        tracing::warn!(
            texture = key,
            "skipping unusable or over-budget original UI sheet"
        );
        self.failed.insert(key);
    }

    /// A terminal failure permits the ordinary sprite lookup to reveal valid lower-layer pixels.
    pub(super) fn failed(&self, key: &str) -> bool {
        self.failed.contains(key)
    }

    /// The loaded sheet and its first reserved logical page, independent of ordinary atlas residency.
    pub(super) fn get(&self, key: &str) -> Option<(&Sheet, usize)> {
        let mut first = 0;
        for (name, sheet) in &self.loaded {
            if name == key {
                return Some((sheet, first));
            }
            first += sheet.tiles.len();
        }
        None
    }

    /// Original coordinates for consumers whose image fits one sprite page.
    pub(super) fn sprite(&self, key: &str) -> Option<(usize, [f32; 4])> {
        let (sheet, first) = self.get(key)?;
        (sheet.tiles.len() == 1).then_some((
            first,
            [0.0, 0.0, sheet.size[0] as f32, sheet.size[1] as f32],
        ))
    }

    /// Immutable original pages in the same order their sheet references address.
    pub(super) fn pages(&self) -> &[UiTexturePage] {
        &self.pages
    }

    /// Whether resource admission changed since the presentation last published pages.
    pub(super) fn take_dirty(&mut self) -> bool {
        std::mem::take(&mut self.dirty)
    }
}
