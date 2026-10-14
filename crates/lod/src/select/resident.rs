//! The chunk columns real chunks are drawn for, as a summed-area table so a walk asks in
//! constant time whether a section's footprint holds any or only resident columns.

use world::SUB_CHUNK_SIDE;

use crate::geometry::{SectionPos, section_blocks};

/// Cells of the table beyond which columns are kept as a sorted list instead, so a few
/// scattered columns never allocate a huge table.
const MAX_TABLE_CELLS: usize = 1 << 22;

/// The chunk columns a section's footprint spans: `min` inclusive and `max` exclusive, in
/// chunk X and Z.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct ChunkRect {
    pub min: [i64; 2],
    pub max: [i64; 2],
}

impl ChunkRect {
    /// The footprint of `pos`.
    #[must_use]
    pub fn of(pos: SectionPos) -> Self {
        let side = SUB_CHUNK_SIDE as i64;
        let edge = section_blocks(pos.level());
        let min = pos.min_block();
        Self {
            min: [min[0].div_euclid(side), min[2].div_euclid(side)],
            max: [
                (min[0] + edge).div_euclid(side),
                (min[2] + edge).div_euclid(side),
            ],
        }
    }

    /// Columns in the rectangle.
    #[must_use]
    pub fn area(self) -> u64 {
        ((self.max[0] - self.min[0]) * (self.max[1] - self.min[1])) as u64
    }
}

/// How the columns are kept.
#[derive(Debug, Clone)]
enum Index {
    /// Prefix sums over the bounding box, one row more and one column more than it.
    Table {
        min: [i64; 2],
        width: usize,
        depth: usize,
        sums: Vec<u32>,
    },
    /// Distinct columns in ascending order, for sparse sets.
    List(Vec<[i64; 2]>),
}

/// A set of resident chunk columns; see the module documentation.
#[derive(Debug, Clone)]
pub struct ResidentColumns {
    index: Index,
}

impl Default for ResidentColumns {
    fn default() -> Self {
        Self::new(std::iter::empty())
    }
}

impl ResidentColumns {
    /// The set of `columns`, given as chunk X and Z; duplicates count once.
    #[must_use]
    pub fn new(columns: impl IntoIterator<Item = [i32; 2]>) -> Self {
        let mut list: Vec<[i64; 2]> = columns
            .into_iter()
            .map(|[x, z]| [i64::from(x), i64::from(z)])
            .collect();
        list.sort_unstable();
        list.dedup();
        let Some(&first) = list.first() else {
            return Self {
                index: Index::List(list),
            };
        };
        let (mut min, mut max) = (first, first);
        for column in &list {
            for axis in 0..2 {
                min[axis] = min[axis].min(column[axis]);
                max[axis] = max[axis].max(column[axis]);
            }
        }
        let width = (max[0] - min[0] + 1) as usize;
        let depth = (max[1] - min[1] + 1) as usize;
        let cells = (width + 1).saturating_mul(depth + 1);
        if cells > MAX_TABLE_CELLS {
            return Self {
                index: Index::List(list),
            };
        }
        let stride = width + 1;
        let mut sums = vec![0_u32; cells];
        for column in &list {
            let x = (column[0] - min[0]) as usize;
            let z = (column[1] - min[1]) as usize;
            sums[(z + 1) * stride + x + 1] = 1;
        }
        for z in 1..=depth {
            for x in 1..=width {
                sums[z * stride + x] += sums[(z - 1) * stride + x] + sums[z * stride + x - 1]
                    - sums[(z - 1) * stride + x - 1];
            }
        }
        Self {
            index: Index::Table {
                min,
                width,
                depth,
                sums,
            },
        }
    }

    /// True when no column is resident.
    #[must_use]
    pub fn is_empty(&self) -> bool {
        match &self.index {
            Index::Table { .. } => false,
            Index::List(list) => list.is_empty(),
        }
    }

    /// Resident columns inside `rect`.
    #[must_use]
    pub fn count_in(&self, rect: ChunkRect) -> u64 {
        match &self.index {
            Index::Table {
                min,
                width,
                depth,
                sums,
            } => {
                let clamp = |value: i64, axis: usize, len: usize| {
                    (value - min[axis]).clamp(0, len as i64) as usize
                };
                let (x0, x1) = (clamp(rect.min[0], 0, *width), clamp(rect.max[0], 0, *width));
                let (z0, z1) = (clamp(rect.min[1], 1, *depth), clamp(rect.max[1], 1, *depth));
                if x0 >= x1 || z0 >= z1 {
                    return 0;
                }
                let stride = width + 1;
                let at = |x: usize, z: usize| i64::from(sums[z * stride + x]);
                (at(x1, z1) - at(x0, z1) - at(x1, z0) + at(x0, z0)) as u64
            }
            Index::List(list) => list
                .iter()
                .filter(|column| {
                    (rect.min[0]..rect.max[0]).contains(&column[0])
                        && (rect.min[1]..rect.max[1]).contains(&column[1])
                })
                .count() as u64,
        }
    }

    /// True when any column under `pos` is resident.
    #[must_use]
    pub fn overlaps(&self, pos: SectionPos) -> bool {
        !self.is_empty() && self.count_in(ChunkRect::of(pos)) != 0
    }

    /// True when every column under `pos` is resident.
    #[must_use]
    pub fn covers(&self, pos: SectionPos) -> bool {
        let rect = ChunkRect::of(pos);
        !self.is_empty() && self.count_in(rect) == rect.area()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn pos(level: u8, x: i32, z: i32) -> SectionPos {
        SectionPos::new(level, x, 0, z).expect("position")
    }

    #[test]
    fn footprints_count_resident_columns_like_a_scan() {
        let columns: Vec<[i32; 2]> = (-3_i32..5)
            .flat_map(|x| (-2_i32..3).map(move |z| [x, z]))
            .filter(|[x, z]| (x + z).rem_euclid(3) != 0)
            .collect();
        let resident = ResidentColumns::new(columns.iter().copied());
        for level in 0..3 {
            for x in -3..3 {
                for z in -3..3 {
                    let rect = ChunkRect::of(pos(level, x, z));
                    let scanned = columns
                        .iter()
                        .filter(|[cx, cz]| {
                            (rect.min[0]..rect.max[0]).contains(&i64::from(*cx))
                                && (rect.min[1]..rect.max[1]).contains(&i64::from(*cz))
                        })
                        .count() as u64;
                    assert_eq!(resident.count_in(rect), scanned, "{level} {x} {z}");
                }
            }
        }
    }

    #[test]
    fn overlap_and_cover_follow_the_footprint() {
        // Level-0 section (0, 0) spans chunks 0..2 on both axes.
        let partial = ResidentColumns::new([[0, 0], [1, 0], [0, 1]]);
        assert!(partial.overlaps(pos(0, 0, 0)));
        assert!(!partial.covers(pos(0, 0, 0)));
        let full = ResidentColumns::new([[0, 0], [1, 0], [0, 1], [1, 1]]);
        assert!(full.covers(pos(0, 0, 0)));
        assert!(full.overlaps(pos(4, 0, 0)));
        assert!(!full.covers(pos(1, 0, 0)));
        assert!(!full.overlaps(pos(0, -1, 0)));
        let none = ResidentColumns::default();
        assert!(!none.overlaps(pos(0, 0, 0)) && !none.covers(pos(0, 0, 0)));
        let sparse = ResidentColumns::new([[-1 << 20, 0], [1 << 20, 0]]);
        assert!(sparse.overlaps(pos(0, (1 << 20) / 2, 0)));
        assert!(!sparse.overlaps(pos(0, 0, 0)));
    }
}
