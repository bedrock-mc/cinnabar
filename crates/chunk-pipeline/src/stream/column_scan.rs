//! Row cursors that skip the unchanged interior of a moving circular view.

use super::*;
use std::ops::Bound::{Excluded, Unbounded};

/// Bounds a column scan strictly after its last completed item.
pub(super) fn after(
    cursor: Option<ChunkKey>,
) -> (std::ops::Bound<ChunkKey>, std::ops::Bound<ChunkKey>) {
    (cursor.map_or(Unbounded, Excluded), Unbounded)
}

/// Finds a circle's inclusive span on one X row using its owning membership predicate.
pub(super) fn row_span(
    centre: ChunkKey,
    reach: i32,
    x: i32,
    contains: impl Fn(ChunkKey) -> bool,
) -> Option<(i32, i32)> {
    if !contains(ChunkKey::new(centre.dimension, x, centre.z)) {
        return None;
    }
    let (mut low, mut high) = (0, reach);
    while low < high {
        let middle = low + (high - low) / 2 + 1;
        // Test a representable side of the row even at the coordinate limits.
        let z = centre
            .z
            .checked_add(middle)
            .unwrap_or_else(|| centre.z - middle);
        if contains(ChunkKey::new(centre.dimension, x, z)) {
            low = middle;
        } else {
            high = middle - 1;
        }
    }
    Some((centre.z.saturating_sub(low), centre.z.saturating_add(low)))
}

/// A circular full-detail region, with overflow-safe coordinate arithmetic.
#[derive(Clone, Copy)]
pub(super) struct Disk {
    pub(super) centre: ChunkKey,
    pub(super) radius: i32,
}

impl Disk {
    /// Reports membership without squaring deltas outside the bounded radius.
    pub(super) fn contains(self, column: ChunkKey) -> bool {
        let dx = i64::from(column.x) - i64::from(self.centre.x);
        let dz = i64::from(column.z) - i64::from(self.centre.z);
        let radius = i64::from(self.radius);
        column.dimension == self.centre.dimension
            && dx.abs() <= radius
            && dz.abs() <= radius
            && dx * dx + dz * dz <= radius * radius
    }

    /// Returns this disk's inclusive Z span on one row.
    pub(super) fn row(self, x: i32) -> Option<(i32, i32)> {
        row_span(self.centre, self.radius, x, |column| self.contains(column))
    }
}

/// Visits only the entering portions of rows, resuming without a candidate allocation.
pub(super) struct EnteringColumns {
    disk: Disk,
    previous: Option<Disk>,
    x: i64,
    z: i64,
}

impl EnteringColumns {
    /// Starts at the first row of the new disk, optionally excluding the old disk.
    pub(super) fn new(disk: Disk, previous: Option<Disk>) -> Self {
        Self {
            disk,
            previous: previous.filter(|old| old.centre.dimension == disk.centre.dimension),
            x: i64::from(disk.centre.x.saturating_sub(disk.radius)),
            z: i64::from(i32::MIN),
        }
    }
}

impl Iterator for EnteringColumns {
    type Item = ChunkKey;

    /// Skips overlapping row intervals instead of testing every interior column.
    fn next(&mut self) -> Option<Self::Item> {
        while self.x <= i64::from(self.disk.centre.x.saturating_add(self.disk.radius)) {
            let x = self.x as i32;
            if let Some((first, last)) = self.disk.row(x) {
                self.z = self.z.max(i64::from(first));
                if let Some((old_first, old_last)) = self.previous.and_then(|old| old.row(x))
                    && self.z >= i64::from(old_first)
                    && self.z <= i64::from(old_last)
                {
                    self.z = i64::from(old_last) + 1;
                }
                if self.z <= i64::from(last) {
                    let column = ChunkKey::new(self.disk.centre.dimension, x, self.z as i32);
                    self.z += 1;
                    return Some(column);
                }
            }
            self.x += 1;
            self.z = i64::from(i32::MIN);
        }
        None
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Enumerates the owning predicate independently of row skipping.
    fn columns(disk: Disk) -> BTreeSet<ChunkKey> {
        let mut columns = BTreeSet::new();
        for x in
            disk.centre.x.saturating_sub(disk.radius)..=disk.centre.x.saturating_add(disk.radius)
        {
            for z in disk.centre.z.saturating_sub(disk.radius)
                ..=disk.centre.z.saturating_add(disk.radius)
            {
                let column = ChunkKey::new(disk.centre.dimension, x, z);
                if disk.contains(column) {
                    columns.insert(column);
                }
            }
        }
        columns
    }

    /// Boundary strips, diagonals, teleports and coordinate limits keep the exact circle.
    #[test]
    fn entering_rows_equal_the_region_difference() {
        for centre in [ChunkKey::new(0, 0, 0), ChunkKey::new(0, i32::MIN, i32::MAX)] {
            for radius in [1, 2, 16] {
                let disk = Disk { centre, radius };
                for (dx, dz, old_radius) in [
                    (0, 0, radius),
                    (1, 0, radius),
                    (0, -1, radius),
                    (-1, 1, radius),
                    (100, 0, radius),
                    (0, 0, radius + 2),
                ] {
                    let old = Disk {
                        centre: ChunkKey::new(
                            centre.dimension,
                            centre.x.saturating_add(dx),
                            centre.z.saturating_add(dz),
                        ),
                        radius: old_radius,
                    };
                    let expected = columns(disk)
                        .difference(&columns(old))
                        .copied()
                        .collect::<BTreeSet<_>>();
                    let actual = EnteringColumns::new(disk, Some(old)).collect::<Vec<_>>();
                    assert_eq!(actual.iter().copied().collect::<BTreeSet<_>>(), expected);
                    assert_eq!(actual.len(), expected.len(), "no repeated columns");
                }
                assert_eq!(
                    EnteringColumns::new(disk, None).collect::<BTreeSet<_>>(),
                    columns(disk)
                );
            }
        }
    }

    /// A chunk crossing examines only the entering edge, even at a large radius.
    #[test]
    fn one_column_crossing_has_boundary_sized_work() {
        for radius in [16, 64, 128] {
            let previous = Disk {
                centre: ChunkKey::new(0, 0, 0),
                radius,
            };
            let disk = Disk {
                centre: ChunkKey::new(0, 1, 0),
                radius,
            };
            assert_eq!(
                EnteringColumns::new(disk, Some(previous)).count(),
                (2 * radius + 1) as usize
            );
        }
    }
}
