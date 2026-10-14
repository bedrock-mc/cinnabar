//! Cluster and section records: the per-cluster words compaction reads and the
//! per-section words vertex pulling and shading read, each laid out once with a
//! compile-time coverage check so shader constants can be generated from the fields.
//!
//! A cluster record is four words: the index of its first quad in the renderer's quad
//! pool, then [`CLUSTER_INFO_FIELDS`], [`CLUSTER_MIN_FIELDS`] and [`CLUSTER_MAX_FIELDS`]. A
//! section record is [`SECTION_RECORD_WORDS`] words: the section's X, Y and Z at its level
//! as two's-complement integers, then [`SECTION_INFO_FIELDS`], then the index of the first
//! quad-sized unit of its packed biome grid in the renderer's pool, then reserved zeros. An
//! all-zero record is a dead slot. The renderer chooses section slots and pool offsets;
//! records carry them so a cluster finds its section, quads and biomes without a lookup.

use crate::{
    bit_field::{BitField, covers_word},
    draw::{NEIGHBOUR_LEVELS_BITS, NeighbourLevels},
    geometry::{Face, MAX_LEVEL, SECTION_SIDE, SectionPos},
    mesh::{
        Bucket, MeshList,
        output::{CLUSTER_QUADS, Cluster, ClusterBounds, SectionMesh},
    },
};

/// Words in one cluster record.
pub const CLUSTER_RECORD_WORDS: usize = 4;
/// Words in one section record; a power of two so records stay aligned.
pub const SECTION_RECORD_WORDS: usize = 8;
/// Index of the X word in a section record.
pub const SECTION_X_WORD: usize = 0;
/// Index of the [`SECTION_INFO_FIELDS`] word in a section record.
pub const SECTION_INFO_WORD: usize = 3;
/// Index of the biome grid's pool offset in a section record.
pub const SECTION_BIOMES_WORD: usize = 4;

/// The renderer's slot of the cluster's section.
pub const CLUSTER_SECTION_FIELD: BitField = BitField::first("section", 20);
/// [`Face::index`] of the cluster's direction.
pub const CLUSTER_FACE_FIELD: BitField = CLUSTER_SECTION_FIELD.after("face", 3);
/// [`MeshList::index`] of the cluster's list.
pub const CLUSTER_LIST_FIELD: BitField = CLUSTER_FACE_FIELD.after("list", 2);
/// [`Bucket::index`] of the cluster's bucket.
pub const CLUSTER_BUCKET_FIELD: BitField = CLUSTER_LIST_FIELD.after("bucket", 1);
/// Quads in the cluster, minus one.
pub const CLUSTER_COUNT_FIELD: BitField = CLUSTER_BUCKET_FIELD.after("count", 6);

/// The second word of a cluster record, in bit order.
pub const CLUSTER_INFO_FIELDS: [BitField; 5] = [
    CLUSTER_SECTION_FIELD,
    CLUSTER_FACE_FIELD,
    CLUSTER_LIST_FIELD,
    CLUSTER_BUCKET_FIELD,
    CLUSTER_COUNT_FIELD,
];

/// Minimum X of the cluster's bounds, in section-local voxel corners.
pub const CLUSTER_MIN_X_FIELD: BitField = BitField::first("min_x", 6);
/// Minimum Y of the cluster's bounds.
pub const CLUSTER_MIN_Y_FIELD: BitField = CLUSTER_MIN_X_FIELD.after("min_y", 6);
/// Minimum Z of the cluster's bounds.
pub const CLUSTER_MIN_Z_FIELD: BitField = CLUSTER_MIN_Y_FIELD.after("min_z", 6);
/// [`Cluster::chunk_columns`]: the level-0 chunk columns the cluster's voxels lie in.
pub const CLUSTER_CHUNK_COLUMNS_FIELD: BitField = CLUSTER_MIN_Z_FIELD.after("chunk_columns", 4);
/// Reserved; zero.
pub const CLUSTER_MIN_SPARE_FIELD: BitField = CLUSTER_CHUNK_COLUMNS_FIELD.after("spare", 10);

/// The third word of a cluster record, in bit order.
pub const CLUSTER_MIN_FIELDS: [BitField; 5] = [
    CLUSTER_MIN_X_FIELD,
    CLUSTER_MIN_Y_FIELD,
    CLUSTER_MIN_Z_FIELD,
    CLUSTER_CHUNK_COLUMNS_FIELD,
    CLUSTER_MIN_SPARE_FIELD,
];

/// Maximum X of the cluster's bounds, in section-local voxel corners.
pub const CLUSTER_MAX_X_FIELD: BitField = BitField::first("max_x", 6);
/// Maximum Y of the cluster's bounds.
pub const CLUSTER_MAX_Y_FIELD: BitField = CLUSTER_MAX_X_FIELD.after("max_y", 6);
/// Maximum Z of the cluster's bounds.
pub const CLUSTER_MAX_Z_FIELD: BitField = CLUSTER_MAX_Y_FIELD.after("max_z", 6);
/// Reserved; zero.
pub const CLUSTER_MAX_SPARE_FIELD: BitField = CLUSTER_MAX_Z_FIELD.after("spare", 14);

/// The fourth word of a cluster record, in bit order.
pub const CLUSTER_MAX_FIELDS: [BitField; 4] = [
    CLUSTER_MAX_X_FIELD,
    CLUSTER_MAX_Y_FIELD,
    CLUSTER_MAX_Z_FIELD,
    CLUSTER_MAX_SPARE_FIELD,
];

/// The section's level.
pub const SECTION_LEVEL_FIELD: BitField = BitField::first("level", 3);
/// The section's [`NeighbourLevels`].
pub const SECTION_NEIGHBOURS_FIELD: BitField =
    SECTION_LEVEL_FIELD.after("neighbours", NEIGHBOUR_LEVELS_BITS);
/// Set in every drawn section's record, so a level-0 section with no neighbours is not
/// mistaken for a dead slot.
pub const SECTION_LIVE_FIELD: BitField = SECTION_NEIGHBOURS_FIELD.after("live", 1);
/// Reserved; zero.
pub const SECTION_SPARE_FIELD: BitField = SECTION_LIVE_FIELD.after("spare", 16);

/// The [`SECTION_INFO_WORD`] of a section record, in bit order.
pub const SECTION_INFO_FIELDS: [BitField; 4] = [
    SECTION_LEVEL_FIELD,
    SECTION_NEIGHBOURS_FIELD,
    SECTION_LIVE_FIELD,
    SECTION_SPARE_FIELD,
];

/// Section slots a cluster record can name.
pub const MAX_SECTION_SLOTS: u32 = CLUSTER_SECTION_FIELD.max_value() + 1;

const _: () = assert!(
    covers_word(&CLUSTER_INFO_FIELDS),
    "cluster info covers its word"
);
const _: () = assert!(
    covers_word(&CLUSTER_MIN_FIELDS),
    "cluster minimum covers its word"
);
const _: () = assert!(
    covers_word(&CLUSTER_MAX_FIELDS),
    "cluster maximum covers its word"
);
const _: () = assert!(
    covers_word(&SECTION_INFO_FIELDS),
    "section info covers its word"
);
const _: () = assert!(CLUSTER_FACE_FIELD.max_value() as usize >= Face::ALL.len() - 1);
const _: () = assert!(CLUSTER_LIST_FIELD.max_value() as usize >= MeshList::ALL.len() - 1);
const _: () = assert!(CLUSTER_BUCKET_FIELD.max_value() as usize >= Bucket::ALL.len() - 1);
const _: () = assert!(CLUSTER_COUNT_FIELD.max_value() as usize == CLUSTER_QUADS - 1);
const _: () = assert!(CLUSTER_MIN_X_FIELD.max_value() as usize >= SECTION_SIDE);
const _: () = assert!(CLUSTER_MAX_X_FIELD.width == CLUSTER_MIN_X_FIELD.width);
const _: () = assert!(
    CLUSTER_CHUNK_COLUMNS_FIELD.width == 4,
    "one bit per chunk column"
);
const _: () = assert!(SECTION_LEVEL_FIELD.max_value() >= MAX_LEVEL as u32);
const _: () = assert!(SECTION_RECORD_WORDS.is_power_of_two());
const _: () =
    assert!(SECTION_INFO_WORD == SECTION_X_WORD + 3 && SECTION_BIOMES_WORD < SECTION_RECORD_WORDS);

/// One cluster's record; see the module documentation.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
#[repr(transparent)]
pub struct ClusterRecord([u32; CLUSTER_RECORD_WORDS]);

impl ClusterRecord {
    /// The record of `cluster` in section slot `slot`, whose quads start at `quad_base` in
    /// the quad pool; `None` when the slot is past [`MAX_SECTION_SLOTS`] or the first quad's
    /// index overflows.
    #[must_use]
    pub fn new(cluster: &Cluster, slot: u32, quad_base: u32) -> Option<Self> {
        if slot >= MAX_SECTION_SLOTS {
            return None;
        }
        let first = quad_base.checked_add(cluster.first)?;
        let [min, max] =
            [cluster.bounds.min, cluster.bounds.max].map(|corner| corner.map(u32::from));
        Some(Self([
            first,
            CLUSTER_SECTION_FIELD.place(slot)
                | CLUSTER_FACE_FIELD.place(cluster.face.index() as u32)
                | CLUSTER_LIST_FIELD.place(cluster.list.index() as u32)
                | CLUSTER_BUCKET_FIELD.place(cluster.bucket.index() as u32)
                | CLUSTER_COUNT_FIELD.place(cluster.count - 1),
            CLUSTER_MIN_X_FIELD.place(min[0])
                | CLUSTER_MIN_Y_FIELD.place(min[1])
                | CLUSTER_MIN_Z_FIELD.place(min[2])
                | CLUSTER_CHUNK_COLUMNS_FIELD.place(u32::from(cluster.chunk_columns)),
            CLUSTER_MAX_X_FIELD.place(max[0])
                | CLUSTER_MAX_Y_FIELD.place(max[1])
                | CLUSTER_MAX_Z_FIELD.place(max[2]),
        ]))
    }

    /// The record's words, for upload.
    #[must_use]
    pub const fn words(&self) -> &[u32; CLUSTER_RECORD_WORDS] {
        &self.0
    }

    /// Index of the first quad in the quad pool.
    #[must_use]
    pub const fn first_quad(&self) -> u32 {
        self.0[0]
    }

    /// The section slot.
    #[must_use]
    pub const fn slot(&self) -> u32 {
        CLUSTER_SECTION_FIELD.read(self.0[1])
    }

    /// The direction every quad faces.
    #[must_use]
    pub const fn face(&self) -> Face {
        Face::ALL[CLUSTER_FACE_FIELD.read(self.0[1]) as usize]
    }

    /// The list the quads belong to.
    #[must_use]
    pub const fn list(&self) -> MeshList {
        MeshList::ALL[CLUSTER_LIST_FIELD.read(self.0[1]) as usize]
    }

    /// The bucket the quads belong to.
    #[must_use]
    pub const fn bucket(&self) -> Bucket {
        Bucket::ALL[CLUSTER_BUCKET_FIELD.read(self.0[1]) as usize]
    }

    /// Quads in the cluster.
    #[must_use]
    pub const fn count(&self) -> u32 {
        CLUSTER_COUNT_FIELD.read(self.0[1]) + 1
    }

    /// The bounds in section-local voxel corners.
    #[must_use]
    pub const fn bounds(&self) -> ClusterBounds {
        let (low, high) = (self.0[2], self.0[3]);
        ClusterBounds {
            min: [
                CLUSTER_MIN_X_FIELD.read(low) as u8,
                CLUSTER_MIN_Y_FIELD.read(low) as u8,
                CLUSTER_MIN_Z_FIELD.read(low) as u8,
            ],
            max: [
                CLUSTER_MAX_X_FIELD.read(high) as u8,
                CLUSTER_MAX_Y_FIELD.read(high) as u8,
                CLUSTER_MAX_Z_FIELD.read(high) as u8,
            ],
        }
    }

    /// The [`Cluster::chunk_columns`] bits.
    #[must_use]
    pub const fn chunk_columns(&self) -> u8 {
        CLUSTER_CHUNK_COLUMNS_FIELD.read(self.0[2]) as u8
    }
}

/// Appends the records of every cluster of `mesh` in section slot `slot`, whose quads
/// start at `quad_base` in the quad pool; false, appending nothing, when a record cannot
/// name them ([`ClusterRecord::new`]).
pub fn push_cluster_records(
    mesh: &SectionMesh,
    slot: u32,
    quad_base: u32,
    out: &mut Vec<ClusterRecord>,
) -> bool {
    let start = out.len();
    for cluster in mesh.clusters() {
        let Some(record) = ClusterRecord::new(cluster, slot, quad_base) else {
            out.truncate(start);
            return false;
        };
        out.push(record);
    }
    true
}

/// One section's record; see the module documentation.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
#[repr(transparent)]
pub struct SectionRecord([u32; SECTION_RECORD_WORDS]);

impl SectionRecord {
    /// A dead slot: no cluster naming it is drawn.
    pub const DEAD: Self = Self([0; SECTION_RECORD_WORDS]);

    /// The live record of the section at `pos` whose faces draw crack lists by
    /// `neighbours` and whose packed biome grid starts at unit `biomes` of the pool.
    #[must_use]
    pub fn new(pos: SectionPos, neighbours: NeighbourLevels, biomes: u32) -> Self {
        let mut words = [0; SECTION_RECORD_WORDS];
        words[SECTION_X_WORD] = pos.x() as u32;
        words[SECTION_X_WORD + 1] = pos.y() as u32;
        words[SECTION_X_WORD + 2] = pos.z() as u32;
        words[SECTION_INFO_WORD] = SECTION_LEVEL_FIELD.place(u32::from(pos.level()))
            | SECTION_NEIGHBOURS_FIELD.place(u32::from(neighbours.bits()))
            | SECTION_LIVE_FIELD.place(1);
        words[SECTION_BIOMES_WORD] = biomes;
        Self(words)
    }

    /// This record with `neighbours`.
    #[must_use]
    pub const fn with_neighbours(mut self, neighbours: NeighbourLevels) -> Self {
        self.0[SECTION_INFO_WORD] = self.0[SECTION_INFO_WORD] & !SECTION_NEIGHBOURS_FIELD.mask()
            | SECTION_NEIGHBOURS_FIELD.place(neighbours.bits() as u32);
        self
    }

    /// True unless the slot is dead.
    #[must_use]
    pub const fn is_live(&self) -> bool {
        SECTION_LIVE_FIELD.read(self.0[SECTION_INFO_WORD]) != 0
    }

    /// The first unit of the packed biome grid in the pool.
    #[must_use]
    pub const fn biomes(&self) -> u32 {
        self.0[SECTION_BIOMES_WORD]
    }

    /// The record's words, for upload.
    #[must_use]
    pub const fn words(&self) -> &[u32; SECTION_RECORD_WORDS] {
        &self.0
    }

    /// The section's position.
    #[must_use]
    pub fn pos(&self) -> Option<SectionPos> {
        SectionPos::new(
            SECTION_LEVEL_FIELD.read(self.0[SECTION_INFO_WORD]) as u8,
            self.0[SECTION_X_WORD] as i32,
            self.0[SECTION_X_WORD + 1] as i32,
            self.0[SECTION_X_WORD + 2] as i32,
        )
        .ok()
    }

    /// The crack-list choice of each face.
    #[must_use]
    pub const fn neighbours(&self) -> NeighbourLevels {
        NeighbourLevels(SECTION_NEIGHBOURS_FIELD.read(self.0[SECTION_INFO_WORD]) as u16)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{draw::NeighbourLevel, geometry::SectionKey};

    #[test]
    fn cluster_records_round_trip_every_field() {
        let cluster = Cluster {
            key: SectionKey::from_bits(0).expect("key"),
            list: MeshList::ToFiner,
            bucket: Bucket::Translucent,
            face: Face::PosZ,
            first: 70,
            count: CLUSTER_QUADS as u32,
            bounds: ClusterBounds {
                min: [0, 3, 32],
                max: [32, 31, 32],
            },
            chunk_columns: 0b1010,
        };
        let slot = MAX_SECTION_SLOTS - 1;
        let record = ClusterRecord::new(&cluster, slot, 1_000).expect("in range");
        assert_eq!(record.first_quad(), 1_070);
        assert_eq!(record.slot(), slot);
        assert_eq!(record.face(), Face::PosZ);
        assert_eq!(record.list(), MeshList::ToFiner);
        assert_eq!(record.bucket(), Bucket::Translucent);
        assert_eq!(record.count(), CLUSTER_QUADS as u32);
        assert_eq!(record.bounds(), cluster.bounds);
        assert_eq!(record.chunk_columns(), 0b1010);
        assert_eq!(ClusterRecord::new(&cluster, MAX_SECTION_SLOTS, 0), None);
        assert_eq!(ClusterRecord::new(&cluster, 0, u32::MAX), None);
    }

    #[test]
    fn section_records_round_trip_negative_positions_and_neighbours() {
        let pos = SectionPos::new(MAX_LEVEL, -5, -2, 1 << 20).expect("position");
        let neighbours = NeighbourLevels::NONE.with(Face::PosX, NeighbourLevel::Finer);
        let record = SectionRecord::new(pos, neighbours, 77);
        assert_eq!(record.pos(), Some(pos));
        assert_eq!(record.neighbours(), neighbours);
        assert_eq!(record.biomes(), 77);
        assert!(record.is_live());
        assert_eq!(record.words()[0], (-5_i32) as u32);
        let relinked = record.with_neighbours(NeighbourLevels::NONE);
        assert_eq!(relinked.neighbours(), NeighbourLevels::NONE);
        assert_eq!(relinked.pos(), Some(pos));
        assert!(relinked.is_live());
        let origin = SectionPos::new(0, 0, 0, 0).expect("origin");
        assert!(SectionRecord::new(origin, NeighbourLevels::NONE, 0).is_live());
        assert!(!SectionRecord::DEAD.is_live());
    }
}
