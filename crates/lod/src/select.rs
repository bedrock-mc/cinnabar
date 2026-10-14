//! Which distant terrain sections to draw, and when to choose again.
//!
//! A walk ([`walk::Selector`]) descends the explored section tree ([`tree::ExploredTree`],
//! the child masks of stored sections) from the [`ROOT_LEVEL`] sections within the drawing
//! distance. A section refines into its non-empty children while its view-independent
//! projected area ([`projected_area`]) exceeds the square of the quality's threshold in
//! pixels per section edge ([`edge_threshold_px`]), and a refined one coarsens
//! again only below that over [`COARSEN_FACTOR`], so sections near the threshold do not
//! flip at every walk. The measure looks at each section from its centre's direction, so
//! turning the camera changes nothing and needs no walk ([`policy::WalkPolicy`]).
//!
//! Only level-0 sections may be drawn over resident chunk columns
//! ([`crate::mesh::overlaps_resident_columns`]), so a walk refines every section that
//! overlaps one ([`resident::ResidentColumns`]) down to level 0 and selects nothing whose
//! chunk columns are all resident, as every cluster there would be culled. Finally face
//! neighbours are balanced to at most one level apart, the transitions the crack lists
//! close.
//!
//! [`shown::ShownSet`] turns successive selections and the meshes that arrive into what a
//! renderer draws: it swaps a refined or coarsened region only once every replacement
//! mesh is ready, so the swap leaves no hole, and reports the changes with each drawn
//! section's [`crate::draw::NeighbourLevels`].

pub mod policy;
pub mod resident;
pub mod shown;
#[cfg(test)]
mod tests;
pub mod tree;
pub mod walk;

use std::collections::{HashMap, HashSet};

use foldhash::fast::FixedState;
use render_api::DistantTerrainQuality;
use world::SUB_CHUNK_SIDE;

use crate::{
    geometry::{SectionKey, SectionPos, section_blocks},
    mip::LOCAL_MIP_TOP_LEVEL,
};

/// Level of the sections a walk starts from: the coarsest level ingestion stores.
pub const ROOT_LEVEL: u8 = LOCAL_MIP_TOP_LEVEL;
/// Farthest drawing distance in chunks while the walk starts at [`ROOT_LEVEL`].
pub const MAX_DISTANCE_CHUNKS: u32 = 512;
/// A section whose centre lies within this many of its own edges of the camera always
/// refines. Beyond 1.27 edges a child's projected area never exceeds its parent's, so the
/// measure is monotone on every root-to-leaf path.
pub const ALWAYS_REFINE_EDGES: f64 = 1.5;
// A child's centre lies within √3/4 parent edges of its parent's, and its area per
// distance² at most √3/4 of the parent's; the bound 1.27 follows from both.
const _: () = assert!(ALWAYS_REFINE_EDGES > 1.27, "the measure stays monotone");
/// A refined section coarsens once its projected area falls below the refine threshold
/// over this factor: 1.5 times fewer pixels per edge.
pub const COARSEN_FACTOR: f64 = 2.25;

/// Sections by key, hashed without per-map random state.
pub(crate) type KeyMap<V> = HashMap<SectionKey, V, FixedState>;
/// A set of section keys, hashed as [`KeyMap`].
pub(crate) type KeySet = HashSet<SectionKey, FixedState>;

/// Pixels a 32-voxel section edge may span on screen at `quality` before the section
/// refines; each finer quality halves it.
#[must_use]
pub const fn edge_threshold_px(quality: DistantTerrainQuality) -> f64 {
    match quality {
        DistantTerrainQuality::Low => 256.0,
        DistantTerrainQuality::Medium => 128.0,
        DistantTerrainQuality::High => 64.0,
    }
}

/// What a walk selects for: the camera, the projection's scale, the quality threshold,
/// the drawing distance and the dimension's height.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct View {
    /// Camera position in blocks.
    pub camera: [f64; 3],
    /// Pixels per unit of `tan(angle)` on screen ([`focal_px`]); follows the field of view.
    pub focal_px: f64,
    /// Pixels per section edge at which sections refine ([`edge_threshold_px`]).
    pub threshold_px: f64,
    /// Horizontal drawing distance in blocks; walks clamp it to [`MAX_DISTANCE_CHUNKS`].
    pub distance_blocks: f64,
    /// Lowest block Y of the dimension.
    pub min_block_y: i32,
    /// One past the highest block Y of the dimension.
    pub max_block_y: i32,
}

impl View {
    /// [`Self::distance_blocks`] clamped to [`MAX_DISTANCE_CHUNKS`].
    #[must_use]
    pub fn clamped_distance(&self) -> f64 {
        self.distance_blocks
            .min(f64::from(MAX_DISTANCE_CHUNKS) * SUB_CHUNK_SIDE as f64)
    }

    /// Projected area in pixels² above which an unrefined section refines.
    #[must_use]
    pub fn refine_area(&self) -> f64 {
        self.threshold_px * self.threshold_px
    }

    /// Projected area in pixels² below which a refined section coarsens.
    #[must_use]
    pub fn coarsen_area(&self) -> f64 {
        self.refine_area() / COARSEN_FACTOR
    }
}

/// [`View::focal_px`] of a viewport `viewport_height_px` pixels tall with vertical field
/// of view `vertical_fov_radians`.
#[must_use]
pub fn focal_px(viewport_height_px: f64, vertical_fov_radians: f64) -> f64 {
    viewport_height_px * 0.5 / (vertical_fov_radians * 0.5).tan()
}

/// The section's area on screen in pixels² as if the camera looked straight at its
/// centre: the orthographic silhouette of the cube along the centre's direction scaled by
/// perspective, `(f·s/d)²·(|dx|+|dy|+|dz|)/d` for edge `s` and centre offset `(dx, dy, dz)`
/// at distance `d`. Infinite within [`ALWAYS_REFINE_EDGES`] edges of the centre.
#[must_use]
pub fn projected_area(camera: [f64; 3], focal_px: f64, pos: SectionPos) -> f64 {
    let edge = section_blocks(pos.level()) as f64;
    let min = pos.min_block();
    let offset: [f64; 3] = std::array::from_fn(|axis| min[axis] as f64 + edge * 0.5 - camera[axis]);
    let squared: f64 = offset.iter().map(|value| value * value).sum();
    let near = ALWAYS_REFINE_EDGES * edge;
    if squared <= near * near {
        return f64::INFINITY;
    }
    let distance = squared.sqrt();
    let scale = focal_px * edge;
    let spread: f64 = offset.iter().map(|value| value.abs()).sum();
    scale * scale / squared * spread / distance
}

/// The squared horizontal distance in blocks from `camera` to the nearest point of the
/// section's footprint.
#[must_use]
pub fn horizontal_distance_squared(camera: [f64; 3], pos: SectionPos) -> f64 {
    let edge = section_blocks(pos.level()) as f64;
    let min = pos.min_block();
    [0, 2]
        .into_iter()
        .map(|axis| {
            let low = min[axis] as f64;
            let gap = (low - camera[axis])
                .max(camera[axis] - (low + edge))
                .max(0.0);
            gap * gap
        })
        .sum()
}
