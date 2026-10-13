//! Section geometry: levels, positions, persistent keys, octants and voxel boxes.

use std::ops::RangeInclusive;

/// Voxels along each section axis at every level.
pub const SECTION_SIDE: usize = 32;
/// `log2(SECTION_SIDE)`.
pub const SECTION_SIDE_LOG2: u32 = SECTION_SIDE.trailing_zeros();
/// Voxels in one section.
pub const SECTION_VOXELS: usize = SECTION_SIDE * SECTION_SIDE * SECTION_SIDE;
/// Vertical voxel columns in one section.
pub const SECTION_COLUMNS: usize = SECTION_SIDE * SECTION_SIDE;
/// Coarsest level; a voxel at level `L` spans `2^L` blocks per axis.
pub const MAX_LEVEL: u8 = 6;
/// Number of levels, `L0..=MAX_LEVEL`.
pub const LEVEL_COUNT: usize = MAX_LEVEL as usize + 1;
/// Voxels along each axis of the region one child section covers in its parent.
pub const CHILD_SPAN: usize = SECTION_SIDE / 2;

const LEVEL_BITS: u32 = 4;
const Y_BITS: u32 = 8;
const HORIZONTAL_BITS: u32 = 24;
const SPARE_BITS: u32 = 4;
const X_SHIFT: u32 = SPARE_BITS;
const Z_SHIFT: u32 = X_SHIFT + HORIZONTAL_BITS;
const Y_SHIFT: u32 = Z_SHIFT + HORIZONTAL_BITS;
const LEVEL_SHIFT: u32 = Y_SHIFT + Y_BITS;
const HORIZONTAL_MASK: u64 = (1 << HORIZONTAL_BITS) - 1;
const Y_MASK: u64 = (1 << Y_BITS) - 1;
const SPARE_MASK: u64 = (1 << SPARE_BITS) - 1;
const _: () = assert!(LEVEL_SHIFT + LEVEL_BITS == u64::BITS);
const _: () = assert!((MAX_LEVEL as u32) < (1 << LEVEL_BITS));

/// Smallest section Y a key can hold.
pub const MIN_SECTION_Y: i32 = -(1 << (Y_BITS - 1));
/// Largest section Y a key can hold.
pub const MAX_SECTION_Y: i32 = (1 << (Y_BITS - 1)) - 1;
/// Smallest section X or Z a key can hold.
pub const MIN_SECTION_HORIZONTAL: i32 = -(1 << (HORIZONTAL_BITS - 1));
/// Largest section X or Z a key can hold.
pub const MAX_SECTION_HORIZONTAL: i32 = (1 << (HORIZONTAL_BITS - 1)) - 1;

/// Linear index of a section-local voxel; X varies fastest, then Z, then Y.
#[must_use]
pub const fn voxel_index(x: usize, y: usize, z: usize) -> usize {
    (y * SECTION_SIDE + z) * SECTION_SIDE + x
}

/// Index of a section-local column in a 32×32 grid; X varies fastest.
#[must_use]
pub const fn column_index(x: usize, z: usize) -> usize {
    z * SECTION_SIDE + x
}

/// Blocks along each axis of one section at `level`.
#[must_use]
pub const fn section_blocks(level: u8) -> i64 {
    (SECTION_SIDE as i64) << level
}

/// Why a section position or key is not representable.
#[derive(Debug, Clone, Copy, PartialEq, Eq, thiserror::Error)]
pub enum KeyError {
    #[error("level {0} exceeds the coarsest level {MAX_LEVEL}")]
    Level(u8),
    #[error("section y {0} is outside {MIN_SECTION_Y}..={MAX_SECTION_Y}")]
    Y(i64),
    #[error("section x/z {0} is outside {MIN_SECTION_HORIZONTAL}..={MAX_SECTION_HORIZONTAL}")]
    Horizontal(i64),
    #[error("key {0:#018x} sets reserved bits")]
    ReservedBits(u64),
}

/// One child slot of a section: bit 0 is +X, bit 1 is +Z, bit 2 is +Y.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct Octant(u8);

impl Octant {
    /// All eight octants in index order.
    pub const ALL: [Self; 8] = [
        Self(0),
        Self(1),
        Self(2),
        Self(3),
        Self(4),
        Self(5),
        Self(6),
        Self(7),
    ];

    /// The octant at `index`, or `None` past 7.
    #[must_use]
    pub const fn new(index: u8) -> Option<Self> {
        if index < 8 { Some(Self(index)) } else { None }
    }

    /// The octant whose offsets are the given half-flags.
    #[must_use]
    pub const fn from_halves(x: bool, y: bool, z: bool) -> Self {
        Self(x as u8 | (z as u8) << 1 | (y as u8) << 2)
    }

    /// Index in `0..8`.
    #[must_use]
    pub const fn index(self) -> usize {
        self.0 as usize
    }

    /// Offset along X, 0 or 1.
    #[must_use]
    pub const fn x(self) -> usize {
        (self.0 & 1) as usize
    }

    /// Offset along Y, 0 or 1.
    #[must_use]
    pub const fn y(self) -> usize {
        (self.0 >> 2 & 1) as usize
    }

    /// Offset along Z, 0 or 1.
    #[must_use]
    pub const fn z(self) -> usize {
        (self.0 >> 1 & 1) as usize
    }

    /// The region this octant covers in its parent section.
    #[must_use]
    pub const fn parent_region(self) -> VoxelBox {
        let min = [
            (self.x() * CHILD_SPAN) as u8,
            (self.y() * CHILD_SPAN) as u8,
            (self.z() * CHILD_SPAN) as u8,
        ];
        VoxelBox {
            min,
            max: [
                min[0] + CHILD_SPAN as u8,
                min[1] + CHILD_SPAN as u8,
                min[2] + CHILD_SPAN as u8,
            ],
        }
    }
}

/// One of a section's six faces.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum Face {
    NegX,
    PosX,
    NegY,
    PosY,
    NegZ,
    PosZ,
}

impl Face {
    /// All six faces in index order.
    pub const ALL: [Self; 6] = [
        Self::NegX,
        Self::PosX,
        Self::NegY,
        Self::PosY,
        Self::NegZ,
        Self::PosZ,
    ];

    /// Index in `0..6`.
    #[must_use]
    pub const fn index(self) -> usize {
        self as usize
    }

    /// The axis the face is normal to: 0 for X, 1 for Y, 2 for Z.
    #[must_use]
    pub const fn axis(self) -> usize {
        self as usize / 2
    }

    /// True for the face on the axis's positive side.
    #[must_use]
    pub const fn is_positive(self) -> bool {
        self as usize % 2 == 1
    }

    /// The face on the other side of the same axis.
    #[must_use]
    pub const fn opposite(self) -> Self {
        Self::ALL[self as usize ^ 1]
    }

    /// The face normal to `axis` (0 X, 1 Y, 2 Z) on the given side; panics past 2.
    #[must_use]
    pub const fn of_axis(axis: usize, positive: bool) -> Self {
        Self::ALL[axis * 2 + positive as usize]
    }

    /// Section coordinate of this face's voxel plane: 0 or `SECTION_SIDE - 1`.
    #[must_use]
    pub const fn plane(self) -> usize {
        if self.is_positive() {
            SECTION_SIDE - 1
        } else {
            0
        }
    }

    /// The one-voxel slab of a section touching this face. A change there alters what the
    /// neighbour across the face sees.
    #[must_use]
    pub fn slab(self) -> VoxelBox {
        let mut min = [0; 3];
        let mut max = [SECTION_SIDE; 3];
        min[self.axis()] = self.plane();
        max[self.axis()] = self.plane() + 1;
        VoxelBox::new(min, max).expect("a face slab is never empty")
    }

    /// The two in-plane coordinates of `[x, y, z]`, in X, Y, Z order without the axis.
    #[must_use]
    pub const fn in_plane(self, at: [usize; 3]) -> [usize; 2] {
        match self.axis() {
            0 => [at[1], at[2]],
            1 => [at[0], at[2]],
            _ => [at[0], at[1]],
        }
    }

    /// The section voxel on this face's plane at in-plane coordinates `[u, v]`.
    #[must_use]
    pub const fn voxel_at(self, [u, v]: [usize; 2]) -> [usize; 3] {
        let plane = self.plane();
        match self.axis() {
            0 => [plane, u, v],
            1 => [u, plane, v],
            _ => [u, v, plane],
        }
    }
}

/// A section's place in the level hierarchy, in that level's section units.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct SectionPos {
    level: u8,
    x: i32,
    y: i32,
    z: i32,
}

impl SectionPos {
    /// Validates that every coordinate fits the key layout.
    pub fn new(level: u8, x: i32, y: i32, z: i32) -> Result<Self, KeyError> {
        Self::checked(level, i64::from(x), i64::from(y), i64::from(z))
    }

    /// A position whose coordinates fit the key fields, or the field that overflows.
    fn checked(level: u8, x: i64, y: i64, z: i64) -> Result<Self, KeyError> {
        if level > MAX_LEVEL {
            return Err(KeyError::Level(level));
        }
        if !(i64::from(MIN_SECTION_Y)..=i64::from(MAX_SECTION_Y)).contains(&y) {
            return Err(KeyError::Y(y));
        }
        let horizontal = i64::from(MIN_SECTION_HORIZONTAL)..=i64::from(MAX_SECTION_HORIZONTAL);
        for value in [x, z] {
            if !horizontal.contains(&value) {
                return Err(KeyError::Horizontal(value));
            }
        }
        Ok(Self {
            level,
            x: x as i32,
            y: y as i32,
            z: z as i32,
        })
    }

    /// The section at `level` containing block `(x, y, z)`.
    pub fn containing_block(level: u8, x: i64, y: i64, z: i64) -> Result<Self, KeyError> {
        if level > MAX_LEVEL {
            return Err(KeyError::Level(level));
        }
        let shift = SECTION_SIDE_LOG2 + u32::from(level);
        Self::checked(level, x >> shift, y >> shift, z >> shift)
    }

    /// The level-0 section holding a 16³ sub-chunk, and the sub-chunk's octant in it.
    pub fn of_sub_chunk(x: i32, y: i32, z: i32) -> Result<(Self, Octant), KeyError> {
        let section = Self::new(0, x.div_euclid(2), y.div_euclid(2), z.div_euclid(2))?;
        let octant = Octant::from_halves(
            x.rem_euclid(2) == 1,
            y.rem_euclid(2) == 1,
            z.rem_euclid(2) == 1,
        );
        Ok((section, octant))
    }

    /// The level; a voxel spans `2^level` blocks.
    #[must_use]
    pub const fn level(self) -> u8 {
        self.level
    }

    /// Section X at this level.
    #[must_use]
    pub const fn x(self) -> i32 {
        self.x
    }

    /// Section Y at this level.
    #[must_use]
    pub const fn y(self) -> i32 {
        self.y
    }

    /// Section Z at this level.
    #[must_use]
    pub const fn z(self) -> i32 {
        self.z
    }

    /// The block at this section's minimum corner.
    #[must_use]
    pub const fn min_block(self) -> [i64; 3] {
        let span = section_blocks(self.level);
        [
            self.x as i64 * span,
            self.y as i64 * span,
            self.z as i64 * span,
        ]
    }

    /// The enclosing section one level coarser, or `None` at [`MAX_LEVEL`].
    #[must_use]
    pub fn parent(self) -> Option<Self> {
        (self.level < MAX_LEVEL).then(|| Self {
            level: self.level + 1,
            x: self.x.div_euclid(2),
            y: self.y.div_euclid(2),
            z: self.z.div_euclid(2),
        })
    }

    /// The ancestor `levels` above this section, or `None` past [`MAX_LEVEL`].
    #[must_use]
    pub fn ancestor(self, levels: u8) -> Option<Self> {
        (0..levels).try_fold(self, |pos, _| pos.parent())
    }

    /// This section's slot in its parent.
    #[must_use]
    pub const fn octant_in_parent(self) -> Octant {
        Octant::from_halves(
            self.x.rem_euclid(2) == 1,
            self.y.rem_euclid(2) == 1,
            self.z.rem_euclid(2) == 1,
        )
    }

    /// The child in `octant`, or `None` at level 0 or when the child is not representable.
    #[must_use]
    pub fn child(self, octant: Octant) -> Option<Self> {
        let level = self.level.checked_sub(1)?;
        Self::checked(
            level,
            i64::from(self.x) * 2 + octant.x() as i64,
            i64::from(self.y) * 2 + octant.y() as i64,
            i64::from(self.z) * 2 + octant.z() as i64,
        )
        .ok()
    }

    /// The same-level section across `face`, or `None` when it is not representable.
    #[must_use]
    pub fn neighbour(self, face: Face) -> Option<Self> {
        let mut at = [i64::from(self.x), i64::from(self.y), i64::from(self.z)];
        at[face.axis()] += if face.is_positive() { 1 } else { -1 };
        Self::checked(self.level, at[0], at[1], at[2]).ok()
    }

    /// The persistent key of this position.
    #[must_use]
    pub const fn key(self) -> SectionKey {
        SectionKey(
            (self.level as u64) << LEVEL_SHIFT
                | ((self.y as u64) & Y_MASK) << Y_SHIFT
                | ((self.z as u64) & HORIZONTAL_MASK) << Z_SHIFT
                | ((self.x as u64) & HORIZONTAL_MASK) << X_SHIFT,
        )
    }
}

/// Persistent section key: level 4 bits | y 8 | z 24 | x 24 | 4 reserved zero bits.
///
/// Coordinates are two's complement. The big-endian bytes sort level-major, so one level
/// is one contiguous byte range; within a level, negative coordinates sort after positive.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct SectionKey(u64);

impl SectionKey {
    /// Validates a raw key from untrusted storage.
    pub const fn from_bits(bits: u64) -> Result<Self, KeyError> {
        if bits & SPARE_MASK != 0 {
            return Err(KeyError::ReservedBits(bits));
        }
        let level = (bits >> LEVEL_SHIFT) as u8;
        if level > MAX_LEVEL {
            return Err(KeyError::Level(level));
        }
        Ok(Self(bits))
    }

    /// Decodes the big-endian byte form.
    pub const fn from_be_bytes(bytes: [u8; 8]) -> Result<Self, KeyError> {
        Self::from_bits(u64::from_be_bytes(bytes))
    }

    /// The packed key as an integer; orders like [`Self::to_be_bytes`].
    #[must_use]
    pub const fn bits(self) -> u64 {
        self.0
    }

    /// The byte form whose lexicographic order is level-major.
    #[must_use]
    pub const fn to_be_bytes(self) -> [u8; 8] {
        self.0.to_be_bytes()
    }

    /// The position this key names.
    #[must_use]
    pub const fn pos(self) -> SectionPos {
        SectionPos {
            level: (self.0 >> LEVEL_SHIFT) as u8,
            y: ((self.0 >> Y_SHIFT) & Y_MASK) as u8 as i8 as i32,
            z: sign_extend_horizontal((self.0 >> Z_SHIFT) & HORIZONTAL_MASK),
            x: sign_extend_horizontal((self.0 >> X_SHIFT) & HORIZONTAL_MASK),
        }
    }

    /// Every raw key value at `level`, for range scans; `None` past [`MAX_LEVEL`].
    #[must_use]
    pub const fn level_range(level: u8) -> Option<RangeInclusive<u64>> {
        if level > MAX_LEVEL {
            return None;
        }
        let start = (level as u64) << LEVEL_SHIFT;
        Some(start..=start | ((1 << LEVEL_SHIFT) - 1))
    }
}

/// Sign-extends a 24-bit two's-complement field.
const fn sign_extend_horizontal(value: u64) -> i32 {
    const UNUSED: u32 = u32::BITS - HORIZONTAL_BITS;
    ((value as u32) << UNUSED) as i32 >> UNUSED
}

impl From<SectionPos> for SectionKey {
    fn from(pos: SectionPos) -> Self {
        pos.key()
    }
}

/// An axis-aligned box of section-local voxels, `min` inclusive and `max` exclusive.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct VoxelBox {
    min: [u8; 3],
    max: [u8; 3],
}

impl VoxelBox {
    /// The whole section.
    pub const FULL: Self = Self {
        min: [0; 3],
        max: [SECTION_SIDE as u8; 3],
    };

    /// A box clamped to the section; `None` when it is empty after clamping.
    #[must_use]
    pub fn new(min: [usize; 3], max: [usize; 3]) -> Option<Self> {
        let clamp = |value: usize| value.min(SECTION_SIDE) as u8;
        let min = min.map(clamp);
        let max = max.map(clamp);
        (0..3)
            .all(|axis| min[axis] < max[axis])
            .then_some(Self { min, max })
    }

    /// The box holding one voxel.
    #[must_use]
    pub fn voxel(x: usize, y: usize, z: usize) -> Option<Self> {
        Self::new([x, y, z], [x + 1, y + 1, z + 1])
    }

    /// Inclusive lower corner.
    #[must_use]
    pub fn min(self) -> [usize; 3] {
        self.min.map(usize::from)
    }

    /// Exclusive upper corner.
    #[must_use]
    pub fn max(self) -> [usize; 3] {
        self.max.map(usize::from)
    }

    /// The smallest box holding both.
    #[must_use]
    pub fn union(self, other: Self) -> Self {
        Self {
            min: [0, 1, 2].map(|axis| self.min[axis].min(other.min[axis])),
            max: [0, 1, 2].map(|axis| self.max[axis].max(other.max[axis])),
        }
    }

    /// Grows the box by `amount` voxels per side, clamped to the section.
    #[must_use]
    pub fn expanded(self, amount: usize) -> Self {
        let min = self.min().map(|value| value.saturating_sub(amount));
        let max = self.max().map(|value| value + amount);
        Self::new(min, max).unwrap_or(self)
    }

    /// The overlap of both boxes, if any.
    #[must_use]
    pub fn intersection(self, other: Self) -> Option<Self> {
        Self::new(
            [0, 1, 2].map(|axis| usize::from(self.min[axis].max(other.min[axis]))),
            [0, 1, 2].map(|axis| usize::from(self.max[axis].min(other.max[axis]))),
        )
    }

    /// True when the voxel lies inside the box.
    #[must_use]
    pub fn contains(self, x: usize, y: usize, z: usize) -> bool {
        [x, y, z]
            .into_iter()
            .enumerate()
            .all(|(axis, value)| self.min()[axis] <= value && value < self.max()[axis])
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn pos(level: u8, x: i32, y: i32, z: i32) -> SectionPos {
        SectionPos::new(level, x, y, z).expect("representable position")
    }

    #[test]
    fn keys_round_trip_at_every_extreme() {
        let horizontal = [MIN_SECTION_HORIZONTAL, -1, 0, 1, MAX_SECTION_HORIZONTAL];
        let vertical = [MIN_SECTION_Y, -1, 0, 1, MAX_SECTION_Y];
        for level in 0..=MAX_LEVEL {
            for &x in &horizontal {
                for &y in &vertical {
                    for &z in &horizontal {
                        let original = pos(level, x, y, z);
                        let key = original.key();
                        assert_eq!(key.pos(), original);
                        assert_eq!(SectionKey::from_be_bytes(key.to_be_bytes()), Ok(key));
                        assert_eq!(key.bits() & SPARE_MASK, 0);
                        assert!(
                            SectionKey::level_range(level)
                                .expect("level")
                                .contains(&key.bits())
                        );
                    }
                }
            }
        }
    }

    #[test]
    fn unrepresentable_positions_and_keys_are_rejected() {
        assert_eq!(SectionPos::new(7, 0, 0, 0), Err(KeyError::Level(7)));
        assert_eq!(SectionPos::new(0, 0, 128, 0), Err(KeyError::Y(128)));
        assert_eq!(SectionPos::new(0, 0, -129, 0), Err(KeyError::Y(-129)));
        assert_eq!(
            SectionPos::new(0, MAX_SECTION_HORIZONTAL + 1, 0, 0),
            Err(KeyError::Horizontal(i64::from(MAX_SECTION_HORIZONTAL) + 1))
        );
        assert_eq!(
            SectionPos::new(0, 0, 0, MIN_SECTION_HORIZONTAL - 1),
            Err(KeyError::Horizontal(i64::from(MIN_SECTION_HORIZONTAL) - 1))
        );
        assert_eq!(SectionKey::from_bits(1), Err(KeyError::ReservedBits(1)));
        assert_eq!(
            SectionKey::from_bits(7 << LEVEL_SHIFT),
            Err(KeyError::Level(7))
        );
        assert_eq!(SectionKey::level_range(7), None);
    }

    #[test]
    fn byte_order_is_level_major() {
        let coarse_min = pos(
            1,
            MIN_SECTION_HORIZONTAL,
            MIN_SECTION_Y,
            MIN_SECTION_HORIZONTAL,
        );
        for fine in [
            pos(0, -1, -1, -1),
            pos(
                0,
                MAX_SECTION_HORIZONTAL,
                MAX_SECTION_Y,
                MAX_SECTION_HORIZONTAL,
            ),
        ] {
            assert!(fine.key().to_be_bytes() < coarse_min.key().to_be_bytes());
            assert!(fine.key() < coarse_min.key());
        }
    }

    #[test]
    fn parents_and_children_invert_with_negative_coordinates() {
        for (x, y, z) in [(0, 0, 0), (-1, -1, -1), (5, -3, -8), (-7, 4, 9)] {
            let section = pos(2, x, y, z);
            let parent = section.parent().expect("below the top level");
            assert_eq!(
                parent,
                pos(3, x.div_euclid(2), y.div_euclid(2), z.div_euclid(2))
            );
            assert_eq!(parent.child(section.octant_in_parent()), Some(section));
            for octant in Octant::ALL {
                let child = section.child(octant).expect("representable child");
                assert_eq!(child.parent(), Some(section));
                assert_eq!(child.octant_in_parent(), octant);
            }
        }
        assert_eq!(pos(MAX_LEVEL, 0, 0, 0).parent(), None);
        assert_eq!(pos(0, 0, 0, 0).child(Octant::ALL[0]), None);
        assert_eq!(pos(1, 0, MAX_SECTION_Y, 0).child(Octant::ALL[4]), None);
        assert_eq!(pos(0, 3, -2, 1).ancestor(2), Some(pos(2, 0, -1, 0)));
        assert_eq!(pos(5, 0, 0, 0).ancestor(2), None);
    }

    #[test]
    fn blocks_and_sub_chunks_map_to_sections() {
        assert_eq!(
            SectionPos::containing_block(0, -1, -1, 31),
            Ok(pos(0, -1, -1, 0))
        );
        assert_eq!(
            SectionPos::containing_block(0, -33, 32, -32),
            Ok(pos(0, -2, 1, -1))
        );
        assert_eq!(
            SectionPos::containing_block(3, 255, -256, -257),
            Ok(pos(3, 0, -1, -2))
        );
        assert_eq!(pos(3, -1, 2, 0).min_block(), [-256, 512, 0]);
        assert_eq!(section_blocks(MAX_LEVEL), 2048);
        assert_eq!(
            SectionPos::of_sub_chunk(-1, -4, 3),
            Ok((pos(0, -1, -2, 1), Octant::from_halves(true, false, true)))
        );
        assert_eq!(
            SectionPos::containing_block(7, 0, 0, 0),
            Err(KeyError::Level(7))
        );
    }

    #[test]
    fn octants_and_boxes() {
        for octant in Octant::ALL {
            assert_eq!(
                Octant::from_halves(octant.x() == 1, octant.y() == 1, octant.z() == 1),
                octant
            );
            let region = octant.parent_region();
            assert_eq!(
                region.min(),
                [octant.x() * 16, octant.y() * 16, octant.z() * 16]
            );
            assert_eq!(region.max(), region.min().map(|value| value + 16));
        }
        assert_eq!(Octant::new(8), None);
        let corner = VoxelBox::voxel(0, 31, 5).expect("in range");
        assert_eq!(corner.expanded(2).min(), [0, 29, 3]);
        assert_eq!(corner.expanded(2).max(), [3, 32, 8]);
        assert_eq!(VoxelBox::new([4, 4, 4], [4, 9, 9]), None);
        let other = VoxelBox::voxel(10, 0, 0).expect("in range");
        let both = corner.union(other);
        assert_eq!((both.min(), both.max()), ([0, 0, 0], [11, 32, 6]));
        assert_eq!(corner.intersection(other), None);
        assert_eq!(both.intersection(corner), Some(corner));
        assert!(both.contains(10, 16, 5));
        assert!(!both.contains(11, 16, 5));
        assert_eq!(voxel_index(31, 31, 31), SECTION_VOXELS - 1);
        assert_eq!(column_index(31, 31), SECTION_COLUMNS - 1);
    }

    #[test]
    fn faces_name_their_planes_neighbours_and_slabs() {
        for (index, face) in Face::ALL.into_iter().enumerate() {
            assert_eq!(face.index(), index);
            assert_eq!(Face::of_axis(face.axis(), face.is_positive()), face);
            assert_eq!(face.opposite().opposite(), face);
            assert_ne!(face.opposite().is_positive(), face.is_positive());
            let slab = face.slab();
            assert_eq!(slab.min()[face.axis()], face.plane());
            assert_eq!(slab.max()[face.axis()], face.plane() + 1);
            for at in [[0, 5, 31], [31, 0, 7], [12, 31, 0]] {
                let on_plane = face.voxel_at(face.in_plane(at));
                let mut expected = at;
                expected[face.axis()] = face.plane();
                assert_eq!(on_plane, expected, "{face:?} {at:?}");
            }
            let origin = pos(3, -1, 0, 1);
            let neighbour = origin.neighbour(face).expect("representable");
            assert_eq!(neighbour.neighbour(face.opposite()), Some(origin));
            assert_eq!(neighbour.level(), 3);
        }
        let edge = pos(
            5,
            MAX_SECTION_HORIZONTAL,
            MIN_SECTION_Y,
            MIN_SECTION_HORIZONTAL,
        );
        assert_eq!(edge.neighbour(Face::PosX), None);
        assert_eq!(edge.neighbour(Face::NegY), None);
        assert_eq!(edge.neighbour(Face::NegZ), None);
        assert_eq!(
            edge.neighbour(Face::NegX),
            Some(pos(
                5,
                MAX_SECTION_HORIZONTAL - 1,
                MIN_SECTION_Y,
                MIN_SECTION_HORIZONTAL
            ))
        );
    }
}
