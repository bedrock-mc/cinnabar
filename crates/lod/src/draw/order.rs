//! Far-to-near order of the sections whose translucent quads are drawn: a counting sort on
//! the integer Euclidean distance from the camera to each section's centre, in
//! [`TRANSLUCENT_ORDER_BUCKETS`] buckets, farthest first, ties kept in key order.

use crate::geometry::{SectionPos, section_blocks};

/// Distance buckets of the sort.
pub const TRANSLUCENT_ORDER_BUCKETS: usize = 1024;

/// Reusable buffers of the sort; keep one per renderer.
#[derive(Debug, Default)]
pub struct TranslucentOrder {
    buckets: Vec<u32>,
    starts: Vec<u32>,
    order: Vec<u32>,
}

impl TranslucentOrder {
    /// Indices into `sections`, which are in ascending key order, from the farthest
    /// section to the nearest seen from `camera` (block coordinates). Distances at or past
    /// `max_distance` blocks share the farthest bucket.
    pub fn sort(&mut self, camera: [f64; 3], sections: &[SectionPos], max_distance: u32) -> &[u32] {
        debug_assert!(sections.is_sorted_by_key(|pos| pos.key()));
        let width = (u64::from(max_distance) / TRANSLUCENT_ORDER_BUCKETS as u64).max(1);
        self.buckets.clear();
        self.buckets.extend(sections.iter().map(|&pos| {
            let bucket = distance_blocks(camera, pos) / width;
            // Far first: the farthest bucket sorts to index 0.
            (TRANSLUCENT_ORDER_BUCKETS as u64
                - 1
                - bucket.min(TRANSLUCENT_ORDER_BUCKETS as u64 - 1)) as u32
        }));
        self.starts.clear();
        self.starts.resize(TRANSLUCENT_ORDER_BUCKETS + 1, 0);
        for &bucket in &self.buckets {
            self.starts[bucket as usize + 1] += 1;
        }
        for index in 1..self.starts.len() {
            self.starts[index] += self.starts[index - 1];
        }
        self.order.clear();
        self.order.resize(sections.len(), 0);
        for (index, &bucket) in self.buckets.iter().enumerate() {
            let slot = &mut self.starts[bucket as usize];
            self.order[*slot as usize] = index as u32;
            *slot += 1;
        }
        &self.order
    }
}

/// The integer Euclidean distance in blocks from `camera` to the centre of `pos`.
#[must_use]
pub fn distance_blocks(camera: [f64; 3], pos: SectionPos) -> u64 {
    let half = section_blocks(pos.level()) as f64 / 2.0;
    let min = pos.min_block();
    let squared: f64 = (0..3)
        .map(|axis| (min[axis] as f64 + half - camera[axis]).powi(2))
        .sum();
    squared.sqrt() as u64
}

#[cfg(test)]
mod tests {
    use super::*;

    fn pos(level: u8, x: i32, y: i32, z: i32) -> SectionPos {
        SectionPos::new(level, x, y, z).expect("position")
    }

    #[test]
    fn sections_sort_far_first_with_ties_in_key_order() {
        let mut sections = vec![
            pos(0, 0, 0, 0),
            pos(0, 10, 0, 0),
            pos(0, -10, 0, 0),
            pos(0, 3, 0, 0),
            pos(2, 1, 0, 0),
        ];
        sections.sort_by_key(|pos| pos.key());
        let mut order = TranslucentOrder::default();
        let sorted: Vec<SectionPos> = order
            .sort([16.0, 16.0, 16.0], &sections, 4096)
            .iter()
            .map(|&index| sections[index as usize])
            .collect();
        let distances: Vec<u64> = sorted
            .iter()
            .map(|&pos| distance_blocks([16.0, 16.0, 16.0], pos))
            .collect();
        assert!(distances.is_sorted_by(|a, b| a >= b), "{distances:?}");
        assert_eq!(sorted.last(), Some(&pos(0, 0, 0, 0)));
        // The two sections ten apart share a distance and keep key order.
        let ties: Vec<SectionPos> = sorted
            .iter()
            .copied()
            .filter(|pos| pos.x().abs() == 10)
            .collect();
        assert_eq!(ties.len(), 2);
        assert!(ties[0].key() < ties[1].key());
    }
}
