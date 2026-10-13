//! First-fit allocation of quad ranges in the shared far-terrain buffer.

use std::ops::Range;

/// Free ranges kept sorted and coalesced; everything from `end` on is free too.
#[derive(Debug, Default)]
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
    pub(super) fn release(&mut self, range: Range<u32>) {
        if range.is_empty() {
            return;
        }
        let index = self.free.partition_point(|free| free.start < range.start);
        self.free.insert(index, range);
        let mut merged: Vec<Range<u32>> = Vec::with_capacity(self.free.len());
        for range in self.free.drain(..) {
            match merged.last_mut() {
                Some(last) if last.end == range.start => last.end = range.end,
                _ => merged.push(range),
            }
        }
        if merged.last().is_some_and(|last| last.end == self.end) {
            self.end = merged.pop().expect("checked").start;
        }
        self.free = merged;
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
