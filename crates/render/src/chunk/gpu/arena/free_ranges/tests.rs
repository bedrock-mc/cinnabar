use super::*;

/// The sorted linear free list that [`FreeRanges`] replaced, kept as the reference it matches.
#[derive(Default)]
struct Linear(Vec<Range<u32>>);

impl Linear {
    fn insert(&mut self, mut freed: Range<u32>) {
        if freed.is_empty() {
            return;
        }
        let index = self.0.partition_point(|range| range.end < freed.start);
        while index < self.0.len() && self.0[index].start <= freed.end {
            let adjacent = self.0.remove(index);
            freed.start = freed.start.min(adjacent.start);
            freed.end = freed.end.max(adjacent.end);
        }
        self.0.insert(index, freed);
    }

    fn best_fit(&self, required: u32, alignment: u32) -> Option<(usize, u32)> {
        self.0
            .iter()
            .enumerate()
            .filter_map(|(index, range)| {
                let start = checked_align_up(range.start, alignment)?;
                let end = start.checked_add(required)?;
                (end <= range.end).then_some((range.end - range.start, index, start))
            })
            .min_by_key(|&(size, index, _)| (size, index))
            .map(|(_, index, start)| (index, start))
    }

    /// Unaligned takes shrink the hole in place; aligned ones split it, as the arena did.
    fn take(&mut self, required: u32, alignment: u32) -> Option<u32> {
        let (index, start) = self.best_fit(required, alignment)?;
        if alignment == 1 {
            self.0[index].start += required;
            if self.0[index].is_empty() {
                self.0.remove(index);
            }
        } else {
            let source = self.0.remove(index);
            self.insert(source.start..start);
            self.insert(start + required..source.end);
        }
        Some(start)
    }

    fn trim_tail(&mut self, len: &mut usize) {
        while let Some(last) = self.0.last() {
            if last.end as usize != *len {
                break;
            }
            *len = last.start as usize;
            self.0.pop();
        }
    }
}

/// Deterministic xorshift, so a failure replays.
struct Rng(u64);

impl Rng {
    fn below(&mut self, bound: u32) -> u32 {
        self.0 ^= self.0 << 13;
        self.0 ^= self.0 >> 7;
        self.0 ^= self.0 << 17;
        (self.0 % u64::from(bound)) as u32
    }
}

/// Every operation chooses, coalesces and trims exactly as the linear list did.
#[test]
fn indexed_free_ranges_match_the_linear_free_list() {
    let alignments = [1, SHARED_GEOMETRY_ALIGNMENT_WORDS];
    let mut rng = Rng(0x9e37_79b9_7f4a_7c15);
    let mut indexed = FreeRanges::default();
    let mut linear = Linear::default();
    for _ in 0..20_000 {
        match rng.below(4) {
            0 | 1 => {
                let start = rng.below(4_096);
                let freed = start..start + 1 + rng.below(48);
                indexed.insert(freed.clone());
                linear.insert(freed);
            }
            2 => {
                let required = rng.below(96);
                let alignment = alignments[rng.below(2) as usize];
                let expected = linear
                    .best_fit(required, alignment)
                    .map(|(index, start)| (linear.0[index].start, start));
                assert_eq!(indexed.best_fit(required, alignment), expected);
                assert_eq!(
                    indexed.take(required, alignment),
                    linear.take(required, alignment)
                );
            }
            _ => {
                let mut len = linear.0.last().map_or(0, |last| last.end as usize);
                if rng.below(2) == 0 {
                    len += 1;
                }
                let mut indexed_len = len;
                indexed.trim_tail(&mut indexed_len);
                linear.trim_tail(&mut len);
                assert_eq!(indexed_len, len);
            }
        }
        assert!(indexed.iter().eq(linear.0.iter().cloned()));
        assert_eq!(indexed.is_empty(), linear.0.is_empty());
        // Coalesced lists build in bulk; any other order merges hole by hole.
        assert_eq!(linear.0.iter().cloned().collect::<FreeRanges>(), indexed);
        assert_eq!(
            linear.0.iter().rev().cloned().collect::<FreeRanges>(),
            indexed
        );
    }
}
