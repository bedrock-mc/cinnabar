//! First-fit allocation of quad ranges in the shared far-terrain buffer.

use std::ops::Range;

/// Free ranges kept sorted and coalesced; everything from `end` on is free too.
#[derive(Clone, Debug, Default)]
pub(super) struct QuadRanges {
    free: Vec<Range<u32>>,
    end: u32,
}

impl QuadRanges {
    /// The first free range that fits, else fresh space past the end.
    pub(super) fn allocate(&mut self, count: u32) -> Range<u32> {
        if count == 0 {
            return 0..0;
        }
        if let Some(index) = self
            .free
            .iter()
            .position(|range| range.len() as u32 >= count)
        {
            let range = &mut self.free[index];
            let start = range.start;
            range.start += count;
            if range.start == range.end {
                self.free.remove(index);
            }
            return start..start + count;
        }
        self.end += count;
        self.end - count..self.end
    }

    /// Returns `range`, merging it with free neighbours and trimming the end.
    pub(super) fn release(&mut self, mut range: Range<u32>) {
        if range.is_empty() {
            return;
        }
        let mut index = self.free.partition_point(|free| free.start < range.start);
        if index > 0 && self.free[index - 1].end == range.start {
            index -= 1;
            range.start = self.free[index].start;
            self.free.remove(index);
        }
        if index < self.free.len() && self.free[index].start == range.end {
            range.end = self.free.remove(index).end;
        }
        if range.end == self.end {
            self.end = range.start;
        } else {
            self.free.insert(index, range);
        }
    }

    /// One past the highest allocated quad.
    #[cfg(test)]
    pub(super) const fn end(&self) -> u32 {
        self.end
    }
}

#[cfg(test)]
mod tests {
    use super::QuadRanges;

    /// Releasing beside an existing hole needs no replacement free-list allocation.
    #[test]
    fn adjacent_release_does_not_allocate() {
        let mut ranges = QuadRanges::default();
        let blocks: Vec<_> = (0..64).map(|_| ranges.allocate(3)).collect();
        for range in blocks.iter().step_by(2) {
            ranges.release(range.clone());
        }
        let (_, allocations) = test_alloc::count(|| ranges.release(blocks[1].clone()));
        eprintln!("adjacent release: allocations={allocations}");
        assert_eq!(allocations, 0);
        assert_eq!(ranges.allocate(9), 0..9);
    }

    /// Freed space is reused first and coalesces, so churn does not grow the buffer.
    #[test]
    fn released_ranges_coalesce_and_are_reused() {
        let mut ranges = QuadRanges::default();
        let a = ranges.allocate(10);
        let b = ranges.allocate(20);
        let c = ranges.allocate(5);
        assert_eq!((a.clone(), b.clone(), c.clone()), (0..10, 10..30, 30..35));
        ranges.release(a);
        ranges.release(b);
        assert_eq!(ranges.allocate(25), 0..25);
        assert_eq!(ranges.allocate(5), 25..30);
        ranges.release(c);
        assert_eq!(ranges.end(), 30);
        for _ in 0..100 {
            let churn = ranges.allocate(7);
            ranges.release(churn);
        }
        assert_eq!(ranges.end(), 30);
    }
}
