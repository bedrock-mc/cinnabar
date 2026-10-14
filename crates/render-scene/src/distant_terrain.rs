//! The frame's hand-off to the distant terrain renderer, an opt-in Cinnabar extension: the
//! settings, one [`DrawUpdate`] at a time, the colour tables, the resident chunk columns
//! and the frame time go to the render world; the renderer's counters come back.
//!
//! One [`DistantTerrainLink`] is shared by both worlds. The frame hands over the next update
//! only once the renderer committed the previous one ([`DistantTerrainLink::ready_for_update`]),
//! so the renderer applies updates whole and in order, at its own upload pace.

use std::{
    collections::VecDeque,
    sync::{Arc, Mutex, MutexGuard, PoisonError},
};

use bevy::prelude::Resource;
use lod::{
    colour::{BiomeTints, MaterialColours},
    select::shown::DrawUpdate,
};
use render_api::{DistantTerrainQuality, DistantTerrainTier};

/// Chunk columns the player may move from the resident mask's centre before the frame
/// centres a new mask on them.
pub const RESIDENT_RECENTRE_CHUNKS: i32 = 16;

/// Chunk columns per side of the resident column mask, a power of two so shaders wrap
/// columns with a mask: every column a render distance of
/// [`render_api::MAX_VIEW_RADIUS_CHUNKS`] keeps around a player less than
/// [`RESIDENT_RECENTRE_CHUNKS`] from the centre fits.
pub const RESIDENT_MASK_SIDE: u32 = (2
    * (render_api::MAX_VIEW_RADIUS_CHUNKS + RESIDENT_RECENTRE_CHUNKS) as u32)
    .next_power_of_two();
/// Words of the resident column mask, one bit per column.
pub const RESIDENT_MASK_WORDS: usize = (RESIDENT_MASK_SIDE * RESIDENT_MASK_SIDE / 32) as usize;

const _: () = assert!(RESIDENT_MASK_SIDE.is_power_of_two() && RESIDENT_MASK_SIDE >= 32);
const _: () = assert!(
    ResidentColumnMask::RADIUS > render_api::MAX_VIEW_RADIUS_CHUNKS + RESIDENT_RECENTRE_CHUNKS - 1,
    "a resident column of a player short of recentring lies in the window"
);

/// What the renderer draws, while distant terrain is on.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct DistantTerrainRenderSettings {
    pub quality: DistantTerrainQuality,
    pub distance_chunks: u32,
}

/// The session's colour tables for the store's materials and biomes.
#[derive(Debug, Clone)]
pub struct DistantTerrainColourTables {
    pub materials: Arc<MaterialColours>,
    /// `None` until the session's biome tints are known; faces then go untinted.
    pub biomes: Option<Arc<BiomeTints>>,
}

/// The chunk columns resident near the player, one bit each, addressed modulo
/// [`RESIDENT_MASK_SIDE`]: column `(x, z)` is bit `(z mod side) * side + (x mod side)`. Only
/// columns within half a side of `centre` on both axes are answered from the bits; the rest
/// read as not resident.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ResidentColumnMask {
    pub centre: [i32; 2],
    pub words: Box<[u32]>,
}

impl ResidentColumnMask {
    /// Columns farther than this from the centre on either axis read as not resident.
    pub const RADIUS: i32 = (RESIDENT_MASK_SIDE / 2) as i32;

    /// The mask of `columns` around `centre`; columns outside the window are left out.
    #[must_use]
    pub fn new(centre: [i32; 2], columns: impl IntoIterator<Item = [i32; 2]>) -> Self {
        let mut words = vec![0_u32; RESIDENT_MASK_WORDS].into_boxed_slice();
        for column in columns {
            if Self::in_window(centre, column) {
                let bit = Self::bit(column);
                words[bit / 32] |= 1 << (bit % 32);
            }
        }
        Self { centre, words }
    }

    /// The chunk column holding block position `x`, `z`.
    #[must_use]
    pub fn column_of(block: [f64; 2]) -> [i32; 2] {
        block.map(|value| (value / world::SUB_CHUNK_SIDE as f64).floor() as i32)
    }

    /// True when `column` is resident.
    #[must_use]
    pub fn contains(&self, column: [i32; 2]) -> bool {
        if !Self::in_window(self.centre, column) {
            return false;
        }
        let bit = Self::bit(column);
        self.words[bit / 32] >> (bit % 32) & 1 != 0
    }

    /// True when `column` lies within [`Self::RADIUS`] of `centre` on both axes.
    fn in_window(centre: [i32; 2], column: [i32; 2]) -> bool {
        (0..2).all(|axis| {
            i64::from(column[axis]).abs_diff(i64::from(centre[axis])) < Self::RADIUS as u64
        })
    }

    /// The bit index of `column`.
    fn bit(column: [i32; 2]) -> usize {
        let side = RESIDENT_MASK_SIDE as i32;
        (column[1].rem_euclid(side) * side + column[0].rem_euclid(side)) as usize
    }
}

/// Renderer passes timed on the GPU, in encoding order.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum DistantTerrainPass {
    /// Upload scatter and slot writes.
    Scatter,
    /// Last frame's visible clusters.
    PassA,
    /// The depth pyramid after the near terrain and pass A.
    Pyramid,
    /// Section and cluster culling and compaction.
    Cull,
    /// Newly visible clusters, then water far to near, in one render pass.
    PassBWater,
}

impl DistantTerrainPass {
    /// Every pass in encoding order.
    pub const ALL: [Self; 5] = [
        Self::Scatter,
        Self::PassA,
        Self::Pyramid,
        Self::Cull,
        Self::PassBWater,
    ];
    /// Number of passes.
    pub const COUNT: usize = Self::ALL.len();

    /// The name diagnostics print.
    #[must_use]
    pub const fn name(self) -> &'static str {
        match self {
            Self::Scatter => "scatter",
            Self::PassA => "pass_a",
            Self::Pyramid => "pyramid",
            Self::Cull => "cull",
            Self::PassBWater => "pass_b_water",
        }
    }
}

/// Bins of the renderer's screen-size histograms: bin 0 holds sizes below the histogram's
/// unit, bin `i` sizes from `unit · 2^(i−1)` up to twice that, and the last bin every larger
/// size ([`size_bin_floor`]).
pub const DISTANT_TERRAIN_SIZE_BINS: usize = 8;
/// Unit of the visible sections' histogram, binned by the square root of their screen
/// rectangle's area in pixels.
pub const SECTION_SIZE_UNIT_PX: f32 = 4.0;
/// Unit of the visible opaque clusters' histogram, binned by their screen rectangle's longer
/// side in pixels.
pub const CLUSTER_SIZE_UNIT_PX: f32 = 1.0;

/// The smallest size bin `bin` of a histogram with `unit` holds: 0 for the first bin.
#[must_use]
pub fn size_bin_floor(bin: usize, unit: f32) -> f32 {
    if bin == 0 {
        0.0
    } else {
        unit * (1_u32 << (bin - 1)) as f32
    }
}

/// The renderer's counters, published once a frame.
#[derive(Debug, Clone, Copy, Default, PartialEq)]
pub struct DistantTerrainRenderStats {
    /// The device can draw distant terrain; `None` until the renderer looked.
    pub supported: Option<bool>,
    /// Pipelines are compiled and the pool is allocated.
    pub ready: bool,
    /// The device's caps; `None` until the renderer looked. A low-end adapter of a desktop
    /// build caps the drawing distance below the build's tier.
    pub tier: Option<DistantTerrainTier>,
    /// Sections are tested against a depth pyramid of the view; false while its depth target
    /// cannot be sampled, when only the frustum culls.
    pub hiz: bool,
    /// Bytes of the geometry pool, and those in use.
    pub pool_bytes: u64,
    pub pool_used_bytes: u64,
    /// Sections and cluster slots drawn.
    pub sections: u32,
    pub clusters: u32,
    /// Bytes uploaded last frame and the current per-frame budget.
    pub uploaded_bytes: u64,
    pub upload_budget: u64,
    /// Updates committed so far.
    pub committed_updates: u64,
    /// Times the renderer dropped everything it drew for a new pool or target; the frame
    /// starts selection again from nothing drawn whenever this moves on.
    pub resets_requested: u64,
    /// Read back from the GPU a few frames late: visible opaque clusters, those drawn in
    /// the second pass, and visible translucent clusters.
    pub visible_clusters: u32,
    pub pass_b_clusters: u32,
    pub translucent_clusters: u32,
    /// Draw-list entries dropped because a list was full.
    pub list_overflow: u32,
    /// Visible sections by screen size ([`SECTION_SIZE_UNIT_PX`]) and visible opaque
    /// clusters by screen size ([`CLUSTER_SIZE_UNIT_PX`]).
    pub section_size_bins: [u32; DISTANT_TERRAIN_SIZE_BINS],
    pub cluster_size_bins: [u32; DISTANT_TERRAIN_SIZE_BINS],
    /// Quad slots of visible opaque clusters past their quads, drawn as degenerate points.
    pub degenerate_slots: u32,
    /// Sections handed over that could not be uploaded: the pool, the cluster slots or the
    /// section slots were full, so they wait for space, or they had nothing to draw.
    pub dropped_sections: u64,
    /// GPU milliseconds of each [`DistantTerrainPass`], from encoder-level timestamps; zero
    /// where the device cannot time.
    pub gpu_ms: [f32; DistantTerrainPass::COUNT],
}

impl DistantTerrainRenderStats {
    /// The share of the quad slots of visible opaque clusters drawn as degenerate points;
    /// each cluster draws `slots_per_cluster` slots.
    #[must_use]
    pub fn degenerate_share(&self, slots_per_cluster: u32) -> f32 {
        let slots = u64::from(self.visible_clusters) * u64::from(slots_per_cluster);
        if slots == 0 {
            0.0
        } else {
            self.degenerate_slots as f32 / slots as f32
        }
    }

    /// The share of drawn opaque clusters that the second pass drew.
    #[must_use]
    pub fn pass_b_share(&self) -> f32 {
        if self.visible_clusters == 0 {
            0.0
        } else {
            self.pass_b_clusters as f32 / self.visible_clusters as f32
        }
    }
}

/// What the render world takes in one frame.
#[derive(Debug, Default)]
pub struct DistantTerrainInbox {
    /// `None` while distant terrain is off: the renderer frees everything.
    pub settings: Option<DistantTerrainRenderSettings>,
    /// Moves on whenever the renderer must drop what it draws, as when a new selection
    /// starts from nothing drawn.
    pub generation: u64,
    /// The next update, when the renderer is not still applying one.
    pub update: Option<DrawUpdate>,
    /// New colour tables, when they changed.
    pub colours: Option<DistantTerrainColourTables>,
    /// A new resident column mask, when it changed.
    pub resident: Option<Arc<ResidentColumnMask>>,
    /// Seconds the last main-world frame took, which paces uploads.
    pub frame_seconds: f32,
}

#[derive(Debug, Default)]
struct Exchange {
    settings: Option<DistantTerrainRenderSettings>,
    generation: u64,
    updates: VecDeque<DrawUpdate>,
    colours: Option<DistantTerrainColourTables>,
    resident: Option<Arc<ResidentColumnMask>>,
    frame_seconds: f32,
    /// The renderer holds an update it has not committed.
    applying: bool,
    stats: DistantTerrainRenderStats,
}

/// The shared hand-off; see the module documentation. Cloning shares it.
#[derive(Resource, Clone, Debug, Default)]
pub struct DistantTerrainLink(Arc<Mutex<Exchange>>);

impl DistantTerrainLink {
    /// The exchange, also after a panic poisoned it.
    fn lock(&self) -> MutexGuard<'_, Exchange> {
        self.0.lock().unwrap_or_else(PoisonError::into_inner)
    }

    /// Turns drawing on with `settings`, or off with `None`, which drops queued updates.
    pub fn set_settings(&self, settings: Option<DistantTerrainRenderSettings>) {
        let mut exchange = self.lock();
        if settings.is_none() {
            exchange.updates.clear();
            exchange.applying = false;
        }
        exchange.settings = settings;
    }

    /// Makes the renderer drop what it draws and the updates not handed over yet.
    pub fn reset(&self) {
        let mut exchange = self.lock();
        exchange.generation += 1;
        exchange.updates.clear();
        exchange.applying = false;
    }

    /// True when the renderer committed every update handed over, so the next may go.
    #[must_use]
    pub fn ready_for_update(&self) -> bool {
        let exchange = self.lock();
        exchange.updates.is_empty() && !exchange.applying
    }

    /// Hands over `update`; call only when [`Self::ready_for_update`].
    pub fn push_update(&self, update: DrawUpdate) {
        self.lock().updates.push_back(update);
    }

    /// Hands over new colour tables.
    pub fn set_colours(&self, colours: DistantTerrainColourTables) {
        self.lock().colours = Some(colours);
    }

    /// Hands over a new resident column mask.
    pub fn set_resident(&self, mask: Arc<ResidentColumnMask>) {
        self.lock().resident = Some(mask);
    }

    /// Records how long the last frame took, in seconds.
    pub fn set_frame_seconds(&self, seconds: f32) {
        self.lock().frame_seconds = seconds;
    }

    /// The renderer's latest counters.
    #[must_use]
    pub fn stats(&self) -> DistantTerrainRenderStats {
        self.lock().stats
    }

    /// The render world's side: takes what changed and the next update unless one is
    /// still being applied.
    pub fn take(&self) -> DistantTerrainInbox {
        let mut exchange = self.lock();
        let update = if exchange.applying {
            None
        } else {
            let update = exchange.updates.pop_front();
            exchange.applying = update.is_some();
            update
        };
        DistantTerrainInbox {
            settings: exchange.settings,
            generation: exchange.generation,
            update,
            colours: exchange.colours.take(),
            resident: exchange.resident.take(),
            frame_seconds: exchange.frame_seconds,
        }
    }

    /// The render world's side: the update taken last was applied whole.
    pub fn committed(&self) {
        let mut exchange = self.lock();
        exchange.applying = false;
        exchange.stats.committed_updates += 1;
    }

    /// The render world's side: publishes the counters.
    pub fn publish(&self, stats: DistantTerrainRenderStats) {
        let mut exchange = self.lock();
        let committed = exchange.stats.committed_updates;
        exchange.stats = DistantTerrainRenderStats {
            committed_updates: committed,
            ..stats
        };
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn update() -> DrawUpdate {
        DrawUpdate {
            upserts: Vec::new(),
            removed: Vec::new(),
            relinked: Vec::new(),
            shown: Arc::from([]),
        }
    }

    #[test]
    fn updates_go_one_at_a_time_until_committed() {
        let link = DistantTerrainLink::default();
        assert!(link.ready_for_update());
        link.push_update(update());
        assert!(!link.ready_for_update());
        let inbox = link.take();
        assert!(inbox.update.is_some());
        assert!(!link.ready_for_update(), "applying");
        assert!(link.take().update.is_none());
        link.committed();
        assert!(link.ready_for_update());
        assert_eq!(link.stats().committed_updates, 1);
        link.publish(DistantTerrainRenderStats::default());
        assert_eq!(
            link.stats().committed_updates,
            1,
            "publishing keeps the count"
        );
    }

    #[test]
    fn a_reset_or_turning_off_drops_queued_updates() {
        let link = DistantTerrainLink::default();
        link.push_update(update());
        link.reset();
        assert!(link.ready_for_update());
        let inbox = link.take();
        assert_eq!(inbox.generation, 1);
        assert!(inbox.update.is_none());
        link.push_update(update());
        let _ = link.take();
        link.set_settings(None);
        assert!(link.ready_for_update());
    }

    #[test]
    fn size_bins_double_from_their_unit() {
        assert_eq!(size_bin_floor(0, 4.0), 0.0);
        assert_eq!(size_bin_floor(1, 4.0), 4.0);
        assert_eq!(
            size_bin_floor(DISTANT_TERRAIN_SIZE_BINS - 1, CLUSTER_SIZE_UNIT_PX),
            64.0
        );
        let stats = DistantTerrainRenderStats {
            visible_clusters: 2,
            degenerate_slots: 32,
            ..Default::default()
        };
        assert_eq!(stats.degenerate_share(64), 0.25);
        assert_eq!(
            DistantTerrainRenderStats::default().degenerate_share(64),
            0.0
        );
    }

    #[test]
    fn masks_answer_columns_in_their_window_only() {
        let centre = [1_000, -1_000];
        let mask = ResidentColumnMask::new(
            centre,
            [[1_000, -1_000], [1_001, -999], [1_000 + 600, -1_000]],
        );
        assert!(mask.contains([1_000, -1_000]));
        assert!(mask.contains([1_001, -999]));
        assert!(!mask.contains([1_002, -999]));
        assert!(!mask.contains([1_600, -1_000]), "outside the window");
        // The column one side away aliases the centre's bit but is outside the window.
        assert!(!mask.contains([1_000 + RESIDENT_MASK_SIDE as i32, -1_000]));
        assert_eq!(mask.words.len(), RESIDENT_MASK_WORDS);
        assert_eq!(ResidentColumnMask::column_of([-0.5, 31.9]), [-1, 1]);
    }

    #[test]
    fn masks_cover_the_view_of_a_player_short_of_recentring() {
        let centre = [-40, 7];
        let player = [centre[0] - (RESIDENT_RECENTRE_CHUNKS - 1), centre[1]];
        let radius = render_api::MAX_VIEW_RADIUS_CHUNKS;
        let edges = [
            [player[0] - radius, player[1]],
            [player[0] + radius, player[1] - radius],
            [player[0], player[1] + radius],
        ];
        let mask = ResidentColumnMask::new(centre, edges);
        for column in edges {
            assert!(mask.contains(column), "{column:?}");
        }
    }
}
