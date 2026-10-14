//! A section's mesh: its quads grouped by list, bucket and face, and the 64-quad
//! single-direction clusters the renderer culls and draws.

use std::ops::Range;

use world::SUB_CHUNK_SIDE;

use crate::{
    geometry::{Face, SECTION_SIDE, SectionKey, SectionPos},
    mesh::{
        Bucket, GROUP_COUNT, MeshList, group_index,
        input::WaitingSlabs,
        overlaps_resident_columns,
        quad::Quad,
        translucent::{self, TranslucentClass},
    },
};

/// Quads in a full cluster.
pub const CLUSTER_QUADS: usize = 64;

const _: () = assert!(
    SECTION_SIDE == 2 * SUB_CHUNK_SIDE,
    "a level-0 section spans two chunk columns per axis"
);
const _: () = assert!(
    overlaps_resident_columns(0) && !overlaps_resident_columns(1),
    "chunk-column bits cover a level-0 section's two columns per axis only"
);

/// A box in section-local voxel-corner coordinates, `0..=SECTION_SIDE` per axis; flat along
/// the axis of a single-direction cluster whose quads share one plane.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct ClusterBounds {
    pub min: [u8; 3],
    pub max: [u8; 3],
}

/// Up to [`CLUSTER_QUADS`] consecutive quads of one group.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct Cluster {
    pub key: SectionKey,
    pub list: MeshList,
    pub bucket: Bucket,
    pub face: Face,
    /// Index of the first quad in [`SectionMesh::quads`].
    pub first: u32,
    /// Quads in the cluster, `1..=CLUSTER_QUADS`.
    pub count: u32,
    pub bounds: ClusterBounds,
    /// At level 0, bit `x_half | z_half << 1` is set for each chunk column of the section
    /// the cluster's voxels lie in, so a cluster can be skipped once all of them are
    /// resident; 0 at coarser levels, which are never drawn over resident columns
    /// ([`crate::mesh::overlaps_resident_columns`]).
    pub chunk_columns: u8,
}

/// One section's mesh.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SectionMesh {
    key: SectionKey,
    quads: Box<[Quad]>,
    group_starts: [u32; GROUP_COUNT + 1],
    clusters: Box<[Cluster]>,
    translucent: TranslucentClass,
    waiting: WaitingSlabs,
}

impl SectionMesh {
    /// The meshed section.
    #[must_use]
    pub fn key(&self) -> SectionKey {
        self.key
    }

    /// Every quad, in group order (see [`crate::mesh::group_index`]).
    #[must_use]
    pub fn quads(&self) -> &[Quad] {
        &self.quads
    }

    /// The indices of one group's quads in [`Self::quads`].
    #[must_use]
    pub fn group_range(&self, list: MeshList, bucket: Bucket, face: Face) -> Range<usize> {
        let group = group_index(list, bucket, face);
        self.group_starts[group] as usize..self.group_starts[group + 1] as usize
    }

    /// One group's quads.
    #[must_use]
    pub fn group(&self, list: MeshList, bucket: Bucket, face: Face) -> &[Quad] {
        &self.quads[self.group_range(list, bucket, face)]
    }

    /// The clusters, in group order.
    #[must_use]
    pub fn clusters(&self) -> &[Cluster] {
        &self.clusters
    }

    /// How the translucent quads must be ordered.
    #[must_use]
    pub fn translucent(&self) -> TranslucentClass {
        self.translucent
    }

    /// Sections the mesh read as unknown; their arrival should mesh it again.
    #[must_use]
    pub fn waiting(&self) -> WaitingSlabs {
        self.waiting
    }

    /// True when the mesh has no quads.
    #[must_use]
    pub fn is_empty(&self) -> bool {
        self.quads.is_empty()
    }

    /// Heap bytes held by the quads and clusters.
    #[must_use]
    pub fn heap_bytes(&self) -> usize {
        size_of_val(&*self.quads) + size_of_val(&*self.clusters)
    }
}

/// A quad's voxels as section-local `min` (inclusive) and `max` (exclusive) corners.
type VoxelBox = ([u8; 3], [u8; 3]);

/// Buffers [`build`] reuses between meshes, one entry per quad of the group being copied.
#[derive(Debug, Default)]
pub(super) struct BuildScratch {
    boxes: Vec<VoxelBox>,
    columns: Vec<u8>,
    /// The group's boxes and chunk-column bits in output order.
    sorted_boxes: Vec<VoxelBox>,
    sorted_columns: Vec<u8>,
}

impl BuildScratch {
    /// Bytes held between meshes.
    pub(super) fn heap_bytes(&self) -> usize {
        (self.boxes.capacity() + self.sorted_boxes.capacity()) * size_of::<VoxelBox>()
            + self.columns.capacity()
            + self.sorted_columns.capacity()
    }
}

/// Chunk-column bit combinations a level-0 quad can have.
const COLUMN_MASKS: usize = 1 << 4;

/// Builds the mesh of `pos` from its quad groups. At levels drawn over resident columns,
/// opaque groups are ordered by the chunk columns their quads cover, stably, so clusters
/// cover few; translucent groups keep their far-to-near order.
pub(super) fn build(
    pos: SectionPos,
    groups: &[Vec<Quad>; GROUP_COUNT],
    waiting: WaitingSlabs,
    scratch: &mut BuildScratch,
) -> SectionMesh {
    let level_zero = overlaps_resident_columns(pos.level());
    let total: usize = groups.iter().map(Vec::len).sum();
    let cluster_count: usize = groups
        .iter()
        .map(|group| group.len().div_ceil(CLUSTER_QUADS))
        .sum();
    let mut quads = Vec::with_capacity(total);
    let mut clusters = Vec::with_capacity(cluster_count);
    let mut group_starts = [0; GROUP_COUNT + 1];
    let key = pos.key();
    for list in MeshList::ALL {
        for bucket in Bucket::ALL {
            for face in Face::ALL {
                let group = &groups[group_index(list, bucket, face)];
                let start = quads.len();
                group_starts[group_index(list, bucket, face)] = start as u32;
                scratch.boxes.clear();
                scratch.boxes.extend(group.iter().map(|&quad| {
                    let (min, max) = quad.voxel_box();
                    (min.map(|value| value as u8), max.map(|value| value as u8))
                }));
                scratch.columns.clear();
                if level_zero {
                    scratch
                        .columns
                        .extend(scratch.boxes.iter().map(chunk_columns));
                }
                let (boxes, columns) = if level_zero && bucket == Bucket::Opaque {
                    quads.resize(start + group.len(), Quad::merged(face, [0; 3], [1, 1], 0));
                    sort_by_columns(group, &mut quads[start..], scratch);
                    (&scratch.sorted_boxes, &scratch.sorted_columns)
                } else {
                    quads.extend_from_slice(group);
                    (&scratch.boxes, &scratch.columns)
                };
                for first in (0..group.len()).step_by(CLUSTER_QUADS) {
                    let end = (first + CLUSTER_QUADS).min(group.len());
                    clusters.push(Cluster {
                        key,
                        list,
                        bucket,
                        face,
                        first: (start + first) as u32,
                        count: (end - first) as u32,
                        bounds: bounds(face, &boxes[first..end]),
                        chunk_columns: columns
                            .get(first..end)
                            .map_or(0, |bits| bits.iter().fold(0, |mask, &bit| mask | bit)),
                    });
                }
            }
        }
    }
    group_starts[GROUP_COUNT] = quads.len() as u32;

    SectionMesh {
        key,
        quads: quads.into_boxed_slice(),
        group_starts,
        clusters: clusters.into_boxed_slice(),
        translucent: translucent::classify(groups),
        waiting,
    }
}

/// Writes `group` into `out` stably ordered by the chunk-column bits in
/// `scratch.columns`, and its boxes and bits in that order into `scratch`'s sorted buffers.
fn sort_by_columns(group: &[Quad], out: &mut [Quad], scratch: &mut BuildScratch) {
    let mut next = [0_usize; COLUMN_MASKS];
    for &mask in &scratch.columns {
        next[usize::from(mask)] += 1;
    }
    let mut offset = 0;
    for slot in &mut next {
        let count = *slot;
        *slot = offset;
        offset += count;
    }
    scratch.sorted_boxes.clear();
    scratch.sorted_boxes.resize(group.len(), ([0; 3], [0; 3]));
    scratch.sorted_columns.clear();
    scratch.sorted_columns.resize(group.len(), 0);
    for ((&quad, &voxels), &mask) in group.iter().zip(&scratch.boxes).zip(&scratch.columns) {
        let slot = &mut next[usize::from(mask)];
        out[*slot] = quad;
        scratch.sorted_boxes[*slot] = voxels;
        scratch.sorted_columns[*slot] = mask;
        *slot += 1;
    }
}

/// The level-0 chunk columns a quad's voxels lie in, as [`Cluster::chunk_columns`] bits.
fn chunk_columns((min, max): &VoxelBox) -> u8 {
    let side = SUB_CHUNK_SIDE as u8;
    let x_low = u8::from(min[0] < side);
    let x_high = u8::from(max[0] > side);
    let z_low = u8::from(min[2] < side);
    let z_high = u8::from(max[2] > side);
    (x_low & z_low) | (x_high & z_low) << 1 | (x_low & z_high) << 2 | (x_high & z_high) << 3
}

/// The union of the rectangles of quads with voxel `boxes`, which all face `face`: flat
/// along its axis, on the voxel side the face points to.
fn bounds(face: Face, boxes: &[VoxelBox]) -> ClusterBounds {
    let mut min = [SECTION_SIDE as u8; 3];
    let mut max = [0; 3];
    for (low, high) in boxes {
        for axis in 0..3 {
            min[axis] = min[axis].min(low[axis]);
            max[axis] = max[axis].max(high[axis]);
        }
    }
    let axis = face.axis();
    if face.is_positive() {
        min[axis] += 1;
    } else {
        max[axis] -= 1;
    }
    ClusterBounds { min, max }
}
