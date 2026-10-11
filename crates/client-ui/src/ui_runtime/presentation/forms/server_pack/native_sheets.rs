//! Original-resolution virtual images, decoded once into bounded immutable texture tiles.

use std::{
    collections::BTreeSet,
    sync::{
        Arc, OnceLock,
        atomic::{AtomicBool, Ordering},
    },
};

use crossbeam_channel::{Receiver, Sender};
use image::Limits;
use json_ui::UvRect;
use render_model::{
    MAX_UI_NATIVE_SHEET_BYTES, MAX_UI_NATIVE_SHEET_PAGES, MAX_UI_NATIVE_SHEET_SIDE,
    MAX_UI_TEXTURE_SIDE, UiTexturePage,
};

use super::Source;

/// Neighbor texels overlap tile borders so bilinear filtering crosses them unchanged.
const INTERIOR: u32 = MAX_UI_TEXTURE_SIDE - 2;

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
pub(in crate::ui_runtime::presentation::forms) struct Quad {
    pub page: usize,
    pub uv: [f32; 4],
    pub dest: [f32; 4],
}

#[derive(Clone, Copy)]
struct Axis {
    dest: [f32; 2],
    sample: [f32; 2],
    middle: f32,
}

/// Splits sampling at tile and source edges, retaining mirroring and clamp-to-edge spans.
fn axis(start: f32, end: f32, extent: u32) -> Option<Vec<Axis>> {
    if !start.is_finite() || !end.is_finite() {
        return None;
    }
    let limit = extent as f32;
    if start == end {
        let sample = start.clamp(0.0, limit);
        return Some(vec![Axis {
            dest: [0.0, 1.0],
            sample: [sample; 2],
            middle: sample,
        }]);
    }
    let mut cuts = vec![start.min(end), start.max(end)];
    for edge in (0..extent)
        .step_by(INTERIOR as usize)
        .chain(std::iter::once(extent))
    {
        let edge = edge as f32;
        if edge > cuts[0] && edge < cuts[1] {
            cuts.push(edge);
        }
    }
    cuts.sort_by(f32::total_cmp);
    let pieces = cuts
        .windows(2)
        .map(|edges| {
            let mut a = edges[0];
            let mut b = edges[1];
            if end < start {
                std::mem::swap(&mut a, &mut b);
            }
            Axis {
                dest: [(a - start) / (end - start), (b - start) / (end - start)],
                sample: [a.clamp(0.0, limit), b.clamp(0.0, limit)],
                middle: ((a + b) * 0.5).clamp(0.0, limit),
            }
        })
        .collect();
    Some(pieces)
}

impl Sheet {
    /// Maps the authored source rectangle without resampling or changing its animation time.
    pub(super) fn quads(&self, uv: UvRect, dest: [f32; 4], first: usize) -> Vec<Quad> {
        let [width, height] = self.size.map(|side| side as f32);
        let Some(xs) = axis(uv.u0 * width, uv.u1 * width, self.size[0]) else {
            return Vec::new();
        };
        let Some(ys) = axis(uv.v0 * height, uv.v1 * height, self.size[1]) else {
            return Vec::new();
        };
        let mut quads = Vec::new();
        for y in ys {
            for x in &xs {
                let Some((index, tile)) = self.tiles.iter().enumerate().find(|(_, tile)| {
                    let [left, top, right, bottom] = tile.spec.content;
                    x.middle >= left as f32
                        && (x.middle < right as f32 || right == self.size[0])
                        && y.middle >= top as f32
                        && (y.middle < bottom as f32 || bottom == self.size[1])
                }) else {
                    continue;
                };
                let [left, top] = tile.spec.origin.map(|side| side as f32);
                quads.push(Quad {
                    page: first + index,
                    uv: [
                        x.sample[0] - left,
                        y.sample[0] - top,
                        x.sample[1] - left,
                        y.sample[1] - top,
                    ],
                    dest: [
                        dest[0] + x.dest[0] * (dest[2] - dest[0]),
                        dest[1] + y.dest[0] * (dest[3] - dest[1]),
                        dest[0] + x.dest[1] * (dest[2] - dest[0]),
                        dest[1] + y.dest[1] * (dest[3] - dest[1]),
                    ],
                });
            }
        }
        quads
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

/// One process-wide decode and one queued input bound memory even during rapid pack reloads.
fn worker() -> Option<&'static Sender<Job>> {
    static WORKER: OnceLock<Option<Sender<Job>>> = OnceLock::new();
    WORKER
        .get_or_init(|| {
            let (sender, receiver) = crossbeam_channel::bounded::<Job>(1);
            std::thread::Builder::new()
                .name("ui-native-sheets".into())
                .spawn(move || {
                    while let Ok(job) = receiver.recv() {
                        if job.cancelled.load(Ordering::Acquire) {
                            continue;
                        }
                        let sheet = decode(&job.bytes, &job.plan, &job.cancelled);
                        if !job.cancelled.load(Ordering::Acquire) {
                            let _ = job.done.try_send((job.key, sheet));
                        }
                    }
                })
                .ok()?;
            Some(sender)
        })
        .as_ref()
}

/// Decodes on the worker, releasing its temporary image after copying original texels into tiles.
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
            page: UiTexturePage::owned(spec.size, pixels.into()).ok()?,
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
    /// Publishes completed immutable pages; pack replacement drops the whole namespace.
    pub(super) fn collect(&mut self) {
        while let Ok((key, sheet)) = self.receiver.try_recv() {
            self.pending = None;
            if let Some(sheet) = sheet {
                self.pages
                    .extend(sheet.tiles.iter().map(|tile| tile.page.clone()));
                self.loaded.push((key, sheet));
                self.dirty = true;
            } else {
                self.fail(key);
            }
        }
    }

    /// Requests one original image while keeping decoded bytes and page count within the pack budget.
    pub(super) fn request(&mut self, key: &str, source: &Source) {
        if self.loaded.iter().any(|(name, _)| name == key)
            || self.failed.contains(key)
            || self.pending.is_some()
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
        if worker.try_send(job).is_ok() {
            self.pending = Some(key.to_owned());
        }
    }

    /// Counts an unusable or over-budget image once while retaining a bounded failure set.
    fn fail(&mut self, key: String) {
        if self.failed.len() >= super::MAX_FAILED {
            self.failed.pop_first();
        }
        bevy::log::warn!(
            texture = key,
            "skipping unusable or over-budget original UI sheet"
        );
        self.failed.insert(key);
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

    /// Immutable original pages in the same order their sheet references address.
    pub(super) fn pages(&self) -> &[UiTexturePage] {
        &self.pages
    }

    /// Whether resource admission changed since the presentation last published pages.
    pub(super) fn take_dirty(&mut self) -> bool {
        std::mem::take(&mut self.dirty)
    }
}
