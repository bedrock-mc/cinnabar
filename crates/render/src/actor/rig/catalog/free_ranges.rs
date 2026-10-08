//! Size-indexed gaps keep a burst from rescanning every retained vertex page.
use std::collections::{BTreeMap, BTreeSet};

pub(super) struct FreeRanges(BTreeSet<(usize, usize)>);

impl FreeRanges {
    /// Records the vacant ranges once, preserving every retained page's address.
    pub(super) fn new(occupied: &BTreeMap<usize, usize>, maximum: usize) -> Self {
        let mut free = BTreeSet::new();
        let mut start = 0;
        for (&offset, &len) in occupied {
            #[cfg(test)]
            super::tests::record_probe();
            if offset > start {
                free.insert((offset - start, start));
            }
            start = start.max(offset + len);
        }
        if maximum > start {
            free.insert((maximum - start, start));
        }
        Self(free)
    }

    /// Takes the smallest fitting gap and indexes its remaining tail for the next page.
    pub(super) fn allocate(&mut self, count: usize) -> Option<usize> {
        #[cfg(test)]
        super::tests::record_probe();
        let (len, start) = self.0.range((count, 0)..).next().copied()?;
        self.0.remove(&(len, start));
        if len > count {
            self.0.insert((len - count, start + count));
        }
        Some(start)
    }
}
