//! Section meshes: greedy quads in 64-quad single-direction clusters, with crack border
//! lists for one-level transitions.
//!
//! [`mesher::Mesher`] meshes one section from a [`input::MeshInput`]: the section and, per
//! face, its same-level neighbour, the coarser section across the face and the finer
//! children across it. Faces are found on 34-bit column masks over the section padded with
//! its neighbours' touching slabs, merged greedily into quads of at most
//! [`quad::MAX_QUAD_SIDE`] voxels per side (runs along the first in-plane axis first, then
//! grown along the second), and emitted as [`quad::Quad`] words grouped by
//! [`MeshList`], [`Bucket`] and face, then cut into [`output::Cluster`]s.
//!
//! The binary greedy meshing technique (one occupancy word per padded column, faces found
//! many voxels at a time with shifts and masks, greedy merging on bit rows) follows
//! cgerikj/binary-greedy-meshing, MIT licence, (c) 2020 Erik Johansson; see
//! `THIRD_PARTY_NOTICES.md`.
//!
//! Classes at distant-terrain levels: air is empty; opaque blocks, leaves, hidden voxels and
//! materials the caller cannot class are solid and hide what touches them; water is
//! translucent and shows faces against empty and dry see-through voxels; other see-through
//! blocks are drawn in the opaque bucket and hide nothing, but hide faces of their own
//! material. Waterlogged see-through voxels hold water: water hides faces against them, and
//! against empty voxels they show the water surface in the translucent bucket instead of
//! their own face. Unknown neighbours (not yet sampled or loaded) count as empty voxels lit by
//! [`FALLBACK_LIGHT`], so explored terrain is closed where it meets unexplored space, and
//! the mesh records them ([`input::WaitingSlabs`]) so the section can be meshed again when
//! they arrive.
//!
//! [`cache::MeshCache`] tracks which sections a renderer wants meshed, their versions
//! and the finished meshes kept on the CPU for upload.
//!
//! A face's payload is its voxel's word with the light of the voxel in front of it. Border
//! list faces whose same-level front is solid take the brightest light of the face voxel's
//! other see-through neighbours.

mod border;
pub mod cache;
pub mod class;
mod exterior;
mod greedy;
pub mod input;
pub mod mesher;
pub mod output;
mod padded;
pub mod quad;
#[cfg(test)]
mod reference;
#[cfg(test)]
mod tests;
pub mod translucent;

use crate::{geometry::Face, voxel::Light};

/// Light of the front of faces that face unknown space, and of border faces with no
/// see-through neighbour to borrow light from: full sky, as unsent sub-chunks above
/// terrain are lit.
pub const FALLBACK_LIGHT: Light = Light::FULL_SKY;

/// Which draw a quad belongs to.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum Bucket {
    /// Solid and see-through faces, drawn without blending.
    Opaque,
    /// Water faces, drawn blended after the opaque pass.
    Translucent,
}

impl Bucket {
    /// Every bucket in index order.
    pub const ALL: [Self; 2] = [Self::Opaque, Self::Translucent];

    /// Index in `0..2`.
    #[must_use]
    pub const fn index(self) -> usize {
        self as usize
    }
}

/// Which faces a quad list holds.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum MeshList {
    /// Faces visible against the same-level neighbours; always drawn.
    Main,
    /// Faces on a section face that the same-level neighbour hides but the section one
    /// level coarser across that face shows. Drawn with [`Self::Main`] when the neighbour
    /// region renders one level coarser.
    ToCoarser,
    /// Faces on a section face that the same-level neighbour hides but one of its children
    /// touching the face shows. Drawn with [`Self::Main`] when the neighbour region renders
    /// one level finer.
    ToFiner,
}

impl MeshList {
    /// Every list in index order.
    pub const ALL: [Self; 3] = [Self::Main, Self::ToCoarser, Self::ToFiner];

    /// Index in `0..3`.
    #[must_use]
    pub const fn index(self) -> usize {
        self as usize
    }
}

/// Number of quad groups in a mesh: one per list, bucket and face.
pub const GROUP_COUNT: usize = MeshList::ALL.len() * Bucket::ALL.len() * Face::ALL.len();

/// Index of the quad group of `list`, `bucket` and `face`; groups are ordered list-major,
/// then by bucket, then by [`Face::index`].
#[must_use]
pub const fn group_index(list: MeshList, bucket: Bucket, face: Face) -> usize {
    (list.index() * Bucket::ALL.len() + bucket.index()) * Face::ALL.len() + face.index()
}

/// True for levels whose sections may be drawn over resident chunk columns: level 0 only,
/// whose voxels are blocks, so its faces coincide with resident terrain and lose depth ties
/// to it. Selection refines every section that overlaps a resident column to level 0, as
/// coarser voxels would cover resident terrain and opaque distant terrain never discards
/// fragments. Only these levels seed exterior culling at chunk-column planes and mark
/// [`output::Cluster::chunk_columns`].
#[must_use]
pub const fn overlaps_resident_columns(level: u8) -> bool {
    level == 0
}

/// Whether faces in front of air no see-through path from the section's border reaches
/// are dropped.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Default)]
pub enum ExteriorCulling {
    /// Keep every visible face.
    Off,
    /// Fill see-through voxels from the padded border, and at levels that may be drawn
    /// over resident chunk columns ([`overlaps_resident_columns`]) from both sides of every
    /// chunk-column plane inside the section, since resident chunks can cut it there; then
    /// drop faces whose front the fill missed.
    #[default]
    SectionLocal,
}
