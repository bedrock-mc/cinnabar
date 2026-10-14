//! A first-fit range allocator that keeps allocations near the start, so the kernels that
//! walk every slot up to the highest one in use stay short.

use std::{
    collections::{BTreeMap, BTreeSet},
    ops::Range,
};

/// Hands out ranges of `0..capacity`, lowest fitting address first, and merges freed
/// neighbours. An allocation looks at the lowest free range of each distinct free length
/// that fits, not at every free range.
#[derive(Debug, Clone)]
pub(crate) struct RangeAllocator {
    /// Free ranges by start, never adjacent.
    free: BTreeMap<u32, u32>,
    /// The starts of the free ranges by length.
    by_len: BTreeMap<u32, BTreeSet<u32>>,
    /// Allocated ranges by start, to find the highest end.
    used: BTreeMap<u32, u32>,
    /// Values in use.
    used_values: u32,
}

impl RangeAllocator {
    /// An allocator over `start..capacity`; values below `start` are never handed out.
    pub(crate) fn new(start: u32, capacity: u32) -> Self {
        let mut ranges = Self {
            free: BTreeMap::new(),
            by_len: BTreeMap::new(),
            used: BTreeMap::new(),
            used_values: 0,
        };
        if start < capacity {
            ranges.insert_free(start, capacity);
        }
        ranges
    }

    /// The lowest free range of `len` values; `None` when none fits or `len` is zero.
    pub(crate) fn allocate(&mut self, len: u32) -> Option<Range<u32>> {
        if len == 0 {
            return None;
        }
        let start = self
            .by_len
            .range(len..)
            .filter_map(|(_, starts)| starts.first().copied())
            .min()?;
        let end = self.remove_free(start);
        if end - start > len {
            self.insert_free(start + len, end);
        }
        self.used.insert(start, start + len);
        self.used_values += len;
        Some(start..start + len)
    }

    /// Returns `range`, which [`Self::allocate`] handed out and was not freed since.
    pub(crate) fn free(&mut self, range: Range<u32>) {
        let removed = self.used.remove(&range.start);
        debug_assert_eq!(removed, Some(range.end), "a live allocation");
        self.used_values -= range.end - range.start;
        let mut start = range.start;
        let mut end = range.end;
        if let Some((&before, &before_end)) = self.free.range(..start).next_back()
            && before_end == start
        {
            self.remove_free(before);
            start = before;
        }
        if self.free.contains_key(&end) {
            end = self.remove_free(end);
        }
        self.insert_free(start, end);
    }

    /// One past the highest value in use; the lowest bound of the allocator when none is.
    pub(crate) fn high_water(&self) -> u32 {
        self.used.last_key_value().map_or(0, |(_, &end)| end)
    }

    /// Values in use.
    pub(crate) fn used(&self) -> u32 {
        self.used_values
    }

    /// Records `start..end` as free.
    fn insert_free(&mut self, start: u32, end: u32) {
        self.free.insert(start, end);
        self.by_len.entry(end - start).or_default().insert(start);
    }

    /// Forgets the free range at `start`, returning its end.
    fn remove_free(&mut self, start: u32) -> u32 {
        let end = self.free.remove(&start).expect("a free range");
        let len = end - start;
        let starts = self.by_len.get_mut(&len).expect("indexed by length");
        starts.remove(&start);
        if starts.is_empty() {
            self.by_len.remove(&len);
        }
        end
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn allocations_fill_from_the_start_and_freed_neighbours_merge() {
        let mut ranges = RangeAllocator::new(1, 16);
        assert_eq!(ranges.allocate(4), Some(1..5));
        assert_eq!(ranges.allocate(4), Some(5..9));
        assert_eq!(ranges.allocate(4), Some(9..13));
        assert_eq!(ranges.high_water(), 13);
        assert_eq!(ranges.allocate(4), None, "only three values remain");
        ranges.free(1..5);
        ranges.free(5..9);
        assert_eq!(ranges.allocate(8), Some(1..9), "freed neighbours merged");
        ranges.free(9..13);
        assert_eq!(ranges.high_water(), 9);
        assert_eq!(ranges.used(), 8);
        ranges.free(1..9);
        assert_eq!(ranges.high_water(), 0);
        assert_eq!(ranges.allocate(15), Some(1..16), "everything merged back");
        assert_eq!(ranges.allocate(0), None);
    }

    #[test]
    fn the_lowest_fitting_range_wins_over_shorter_or_higher_ones() {
        let mut ranges = RangeAllocator::new(0, 100);
        let blocks: Vec<_> = (0..10)
            .map(|_| ranges.allocate(10).expect("fits"))
            .collect();
        // Free ranges of lengths 10 at 10, 10 at 50 and 20 at 70.
        for index in [1, 5, 7, 8] {
            ranges.free(blocks[index].clone());
        }
        assert_eq!(ranges.used(), 60);
        assert_eq!(
            ranges.allocate(15),
            Some(70..85),
            "only the long range fits"
        );
        assert_eq!(
            ranges.allocate(5),
            Some(10..15),
            "the lowest of every fitting range"
        );
        assert_eq!(ranges.allocate(10), Some(50..60));
        assert_eq!(ranges.used(), 90);
    }
}
