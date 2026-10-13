//! Region file identity: which file holds a section key or a chunk's content hash, and the
//! slot it takes there.
//!
//! A region file holds every section Y of one level's [`REGION_SIDE_SECTIONS`]² group of
//! section columns. These are storage regions, unrelated to the 64-block signature regions
//! of [`lod::epoch`].

use lod::{
    epoch::CHUNK_SIDE,
    geometry::{MAX_LEVEL, MAX_SECTION_Y, MIN_SECTION_Y, SECTION_SIDE, SectionKey, section_blocks},
};

/// Section columns along each side of one region file.
pub const REGION_SIDE_SECTIONS: i32 = 32;
const REGION_SIDE_LOG2: u32 = REGION_SIDE_SECTIONS.trailing_zeros();
const Y_SLOTS: u32 = (MAX_SECTION_Y - MIN_SECTION_Y + 1) as u32;
/// Section slots one region file can hold: every column at every representable Y.
pub const REGION_SLOTS: u32 = Y_SLOTS << (2 * REGION_SIDE_LOG2);
/// Chunks along each side of a level-0 region file.
pub const REGION_SIDE_CHUNKS: i32 = REGION_SIDE_SECTIONS * (SECTION_SIDE / CHUNK_SIDE) as i32;
const REGION_SIDE_CHUNKS_LOG2: u32 = REGION_SIDE_CHUNKS.trailing_zeros();
/// Chunks along each side of one level-0 section.
const SECTION_SIDE_CHUNKS: u32 = (SECTION_SIDE / CHUNK_SIDE) as u32;
/// Chunk content-hash slots of one level-0 region file.
pub const REGION_CHUNKS: u32 = 1 << (2 * REGION_SIDE_CHUNKS_LOG2);
/// Extension of region file names.
pub const REGION_FILE_EXTENSION: &str = "lodr";
const FILE_PREFIX: &str = "r";

const _: () = assert!((REGION_SIDE_SECTIONS as u32).is_power_of_two());
const _: () = assert!((REGION_SIDE_CHUNKS as u32).is_power_of_two());
const _: () = assert!(SECTION_SIDE.is_multiple_of(CHUNK_SIDE));
const _: () = assert!(REGION_CHUNKS <= u16::MAX as u32 + 1);

/// One region file of one dimension: a level and its region coordinates.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct RegionId {
    level: u8,
    x: i32,
    z: i32,
}

impl RegionId {
    /// A region id, or `None` past [`MAX_LEVEL`].
    #[must_use]
    pub const fn new(level: u8, x: i32, z: i32) -> Option<Self> {
        if level > MAX_LEVEL {
            return None;
        }
        Some(Self { level, x, z })
    }

    /// The region holding `key` and the key's slot in it.
    #[must_use]
    pub const fn of_key(key: SectionKey) -> (Self, SectionSlot) {
        let pos = key.pos();
        let region = Self {
            level: pos.level(),
            x: pos.x() >> REGION_SIDE_LOG2,
            z: pos.z() >> REGION_SIDE_LOG2,
        };
        let mask = REGION_SIDE_SECTIONS - 1;
        let y = (pos.y() - MIN_SECTION_Y) as u32;
        let slot = y << (2 * REGION_SIDE_LOG2)
            | ((pos.z() & mask) as u32) << REGION_SIDE_LOG2
            | (pos.x() & mask) as u32;
        (region, SectionSlot(slot))
    }

    /// The level-0 region holding the content hash of chunk `(x, z)` and the chunk's slot.
    #[must_use]
    pub const fn of_chunk(x: i32, z: i32) -> (Self, ChunkSlot) {
        let region = Self {
            level: 0,
            x: x >> REGION_SIDE_CHUNKS_LOG2,
            z: z >> REGION_SIDE_CHUNKS_LOG2,
        };
        let mask = REGION_SIDE_CHUNKS - 1;
        let slot = ((z & mask) as u32) << REGION_SIDE_CHUNKS_LOG2 | (x & mask) as u32;
        (region, ChunkSlot(slot as u16))
    }

    /// The level.
    #[must_use]
    pub const fn level(self) -> u8 {
        self.level
    }

    /// Region X, in units of [`REGION_SIDE_SECTIONS`] sections at this level.
    #[must_use]
    pub const fn x(self) -> i32 {
        self.x
    }

    /// Region Z, in units of [`REGION_SIDE_SECTIONS`] sections at this level.
    #[must_use]
    pub const fn z(self) -> i32 {
        self.z
    }

    /// The horizontal block coordinates of this region's centre.
    #[must_use]
    pub const fn center_block(self) -> [i64; 2] {
        let span = section_blocks(self.level) * REGION_SIDE_SECTIONS as i64;
        [
            self.x as i64 * span + span / 2,
            self.z as i64 * span + span / 2,
        ]
    }

    /// The file name of this region, which [`Self::from_file_name`] reads back.
    #[must_use]
    pub fn file_name(self) -> String {
        format!(
            "{FILE_PREFIX}.{}.{}.{}.{REGION_FILE_EXTENSION}",
            self.level, self.x, self.z
        )
    }

    /// Parses a name written by [`Self::file_name`]; `None` for any other name.
    #[must_use]
    pub fn from_file_name(name: &str) -> Option<Self> {
        let stem = name
            .strip_prefix(FILE_PREFIX)?
            .strip_prefix('.')?
            .strip_suffix(REGION_FILE_EXTENSION)?
            .strip_suffix('.')?;
        let mut parts = stem.split('.');
        let level = parts.next()?.parse().ok()?;
        let x = parts.next()?.parse().ok()?;
        let z = parts.next()?.parse().ok()?;
        if parts.next().is_some() {
            return None;
        }
        let id = Self::new(level, x, z)?;
        // Only the canonical spelling, so one region never has two files.
        (id.file_name() == name).then_some(id)
    }

    /// The key in `slot` of this region, or `None` when the coordinates are not
    /// representable.
    #[must_use]
    pub fn key_at(self, slot: SectionSlot) -> Option<SectionKey> {
        let raw = slot.0;
        let side = REGION_SIDE_SECTIONS as u32;
        let local_x = (raw % side) as i32;
        let local_z = (raw >> REGION_SIDE_LOG2 & (side - 1)) as i32;
        let y = (raw >> (2 * REGION_SIDE_LOG2)) as i32 + MIN_SECTION_Y;
        let x = self
            .x
            .checked_mul(REGION_SIDE_SECTIONS)?
            .checked_add(local_x)?;
        let z = self
            .z
            .checked_mul(REGION_SIDE_SECTIONS)?
            .checked_add(local_z)?;
        lod::geometry::SectionPos::new(self.level, x, y, z)
            .ok()
            .map(|pos| pos.key())
    }
}

/// A section's slot inside its region file, below [`REGION_SLOTS`].
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct SectionSlot(u32);

impl SectionSlot {
    /// Validates a raw slot from untrusted storage.
    #[must_use]
    pub const fn from_raw(raw: u32) -> Option<Self> {
        if raw < REGION_SLOTS {
            Some(Self(raw))
        } else {
            None
        }
    }

    /// The raw slot.
    #[must_use]
    pub const fn raw(self) -> u32 {
        self.0
    }

    /// The content-hash slots of the chunks a level-0 section in this slot covers.
    pub fn chunk_slots(self) -> impl Iterator<Item = ChunkSlot> {
        let side = REGION_SIDE_SECTIONS as u32;
        let (section_x, section_z) = (self.0 % side, self.0 >> REGION_SIDE_LOG2 & (side - 1));
        (0..SECTION_SIDE_CHUNKS * SECTION_SIDE_CHUNKS).map(move |index| {
            let x = section_x * SECTION_SIDE_CHUNKS + index % SECTION_SIDE_CHUNKS;
            let z = section_z * SECTION_SIDE_CHUNKS + index / SECTION_SIDE_CHUNKS;
            ChunkSlot((z << REGION_SIDE_CHUNKS_LOG2 | x) as u16)
        })
    }
}

/// A chunk's content-hash slot inside its level-0 region file, below [`REGION_CHUNKS`].
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct ChunkSlot(u16);

impl ChunkSlot {
    /// Validates a raw slot from untrusted storage.
    #[must_use]
    pub const fn from_raw(raw: u16) -> Option<Self> {
        if (raw as u32) < REGION_CHUNKS {
            Some(Self(raw))
        } else {
            None
        }
    }

    /// The raw slot.
    #[must_use]
    pub const fn raw(self) -> u16 {
        self.0
    }
}

#[cfg(test)]
mod tests {
    use lod::geometry::{MAX_SECTION_HORIZONTAL, MIN_SECTION_HORIZONTAL, SectionPos};

    use super::*;

    #[test]
    fn keys_round_trip_through_region_and_slot() {
        for (level, x, y, z) in [
            (0, 0, 0, 0),
            (0, -1, -1, -1),
            (3, 31, MAX_SECTION_Y, -32),
            (
                MAX_LEVEL,
                MIN_SECTION_HORIZONTAL,
                MIN_SECTION_Y,
                MAX_SECTION_HORIZONTAL,
            ),
        ] {
            let key = SectionPos::new(level, x, y, z).expect("pos").key();
            let (region, slot) = RegionId::of_key(key);
            assert!(slot.raw() < REGION_SLOTS);
            assert_eq!(region.key_at(slot), Some(key));
        }
    }

    #[test]
    fn neighbouring_sections_share_a_region_until_its_edge() {
        let key = |x| SectionPos::new(0, x, 0, 0).expect("pos").key();
        assert_eq!(RegionId::of_key(key(0)).0, RegionId::of_key(key(31)).0);
        assert_ne!(RegionId::of_key(key(31)).0, RegionId::of_key(key(32)).0);
        assert_eq!(RegionId::of_key(key(-1)).0.x(), -1);
    }

    #[test]
    fn chunks_map_to_the_level_zero_region_of_their_sections() {
        for (chunk_x, chunk_z) in [(0, 0), (-1, 5), (63, -64), (64, 64)] {
            let (region, slot) = RegionId::of_chunk(chunk_x, chunk_z);
            let section =
                SectionPos::new(0, chunk_x.div_euclid(2), 0, chunk_z.div_euclid(2)).expect("pos");
            assert_eq!(region, RegionId::of_key(section.key()).0);
            assert!(u32::from(slot.raw()) < REGION_CHUNKS);
        }
        assert_ne!(RegionId::of_chunk(0, 0).1, RegionId::of_chunk(1, 0).1);
    }

    #[test]
    fn a_level_zero_section_covers_the_chunk_slots_of_its_chunks() {
        for (x, z) in [(0, 0), (-1, 5), (31, -32), (17, 3)] {
            let (_, slot) = RegionId::of_key(SectionPos::new(0, x, 2, z).expect("pos").key());
            let mut covered: Vec<_> = slot.chunk_slots().collect();
            covered.sort_unstable();
            let mut expected: Vec<_> = [(0, 0), (1, 0), (0, 1), (1, 1)]
                .map(|(dx, dz)| RegionId::of_chunk(x * 2 + dx, z * 2 + dz).1)
                .to_vec();
            expected.sort_unstable();
            assert_eq!(covered, expected);
        }
    }

    #[test]
    fn only_canonical_file_names_parse() {
        let id = RegionId::new(2, -7, 300).expect("id");
        assert_eq!(RegionId::from_file_name(&id.file_name()), Some(id));
        for name in [
            "r.2.-7.300.tmp",
            "r.2.+7.300.lodr",
            "r.02.7.300.lodr",
            "r.9.0.0.lodr",
            "r.2.7.lodr",
            "r.2.7.3.4.lodr",
            "x.2.7.3.lodr",
        ] {
            assert_eq!(RegionId::from_file_name(name), None, "{name}");
        }
    }
}
