//! Free address ranges of one arena stream, indexed by address and by size.

use super::*;

#[cfg(test)]
mod tests;

/// Disjoint, non-adjacent free ranges. A best fit walks sizes upward from the request and
/// coalescing looks neighbours up by address, so neither scans every hole.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub(in crate::chunk) struct FreeRanges {
    /// Each hole's start mapped to its end.
    by_start: BTreeMap<u32, u32>,
    /// Each hole as `(length, start)`.
    by_size: BTreeSet<(u32, u32)>,
}

impl FreeRanges {
    #[cfg(test)]
    pub(in crate::chunk) fn is_empty(&self) -> bool {
        self.by_start.is_empty()
    }

    /// The holes in address order.
    pub(in crate::chunk) fn iter(&self) -> impl Iterator<Item = Range<u32>> + '_ {
        self.by_start.iter().map(|(&start, &end)| start..end)
    }

    /// Frees `freed`, merging it with every hole it overlaps or touches.
    pub(in crate::chunk) fn insert(&mut self, mut freed: Range<u32>) {
        if freed.is_empty() {
            return;
        }
        if let Some((&start, &end)) = self.by_start.range(..=freed.start).next_back()
            && end >= freed.start
        {
            self.remove(start);
            freed.start = start;
            freed.end = freed.end.max(end);
        }
        while let Some((&start, &end)) = self.by_start.range(freed.start..=freed.end).next() {
            self.remove(start);
            freed.end = freed.end.max(end);
        }
        self.by_start.insert(freed.start, freed.end);
        self.by_size.insert((freed.end - freed.start, freed.start));
    }

    /// The smallest hole that holds `required` items from an `alignment`-aligned start, the
    /// lowest such hole among equal sizes, as `(hole start, aligned start)`. Best fit keeps
    /// larger contiguous ranges for large uploads.
    pub(in crate::chunk) fn best_fit(&self, required: u32, alignment: u32) -> Option<(u32, u32)> {
        self.by_size
            .range((required, 0)..)
            .find_map(|&(len, start)| {
                let aligned = checked_align_up(start, alignment)?;
                (aligned.checked_add(required)? <= start + len).then_some((start, aligned))
            })
    }

    /// Allocates `required` items from the [`Self::best_fit`] hole, keeping the alignment
    /// padding before them and the remainder after them free.
    pub(in crate::chunk) fn take(&mut self, required: u32, alignment: u32) -> Option<u32> {
        let (hole, start) = self.best_fit(required, alignment)?;
        let hole = self.remove(hole);
        self.insert(hole.start..start);
        self.insert(start + required..hole.end);
        Some(start)
    }

    /// Lowers `len` past every hole that ends at it.
    pub(in crate::chunk) fn trim_tail(&mut self, len: &mut usize) {
        while let Some((&start, &end)) = self.by_start.last_key_value() {
            if end as usize != *len {
                break;
            }
            *len = start as usize;
            self.remove(start);
        }
    }

    fn remove(&mut self, start: u32) -> Range<u32> {
        let end = self
            .by_start
            .remove(&start)
            .expect("indexed holes start where their size entry says");
        self.by_size.remove(&(end - start, start));
        start..end
    }
}

/// Builds address-ordered, already coalesced holes in bulk, and merges anything else hole by
/// hole as [`FreeRanges::insert`] does.
impl FromIterator<Range<u32>> for FreeRanges {
    fn from_iter<T: IntoIterator<Item = Range<u32>>>(ranges: T) -> Self {
        let ranges = ranges.into_iter().collect::<Vec<_>>();
        if ranges.iter().all(|range| !range.is_empty())
            && ranges.windows(2).all(|pair| pair[0].end < pair[1].start)
        {
            let mut sizes = ranges
                .iter()
                .map(|range| (range.end - range.start, range.start))
                .collect::<Vec<_>>();
            // Unstable sorting is cheaper than the set's own stable sort, which then sees a run.
            sizes.sort_unstable();
            return Self {
                by_size: sizes.into_iter().collect(),
                by_start: ranges
                    .into_iter()
                    .map(|range| (range.start, range.end))
                    .collect(),
            };
        }
        let mut free = Self::default();
        for range in ranges {
            free.insert(range);
        }
        free
    }
}
