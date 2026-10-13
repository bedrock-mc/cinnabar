//! Network-hash to sequential-id lookup for hashed-id sessions, which resolve every block through it.

/// Marks an empty slot; sequential ids are bounded far below it.
const EMPTY: u32 = u32::MAX;

/// Canonical (strictly ascending) hash entries plus an open-addressed copy at most half full,
/// so a lookup is one probe on average instead of a binary search over every state.
pub(super) struct HashedIds {
    sorted: Box<[(u32, u32)]>,
    slots: Box<[(u32, u32)]>,
    shift: u32,
}

impl HashedIds {
    pub(super) fn new(sorted: Box<[(u32, u32)]>) -> Self {
        let capacity = (sorted.len() * 2).next_power_of_two().max(2);
        let shift = 32 - capacity.trailing_zeros();
        let mut slots = vec![(0, EMPTY); capacity].into_boxed_slice();
        for &(hash, id) in &sorted {
            debug_assert_ne!(id, EMPTY);
            let mut index = home(hash, shift);
            while slots[index].1 != EMPTY {
                index = (index + 1) & (capacity - 1);
            }
            slots[index] = (hash, id);
        }
        Self {
            sorted,
            slots,
            shift,
        }
    }

    pub(super) fn get(&self, hash: u32) -> Option<u32> {
        let mask = self.slots.len() - 1;
        let mut index = home(hash, self.shift);
        loop {
            let (candidate, id) = self.slots[index];
            if id == EMPTY {
                return None;
            }
            if candidate == hash {
                return Some(id);
            }
            index = (index + 1) & mask;
        }
    }

    pub(super) fn entries(&self) -> &[(u32, u32)] {
        &self.sorted
    }

    pub(super) const fn len(&self) -> usize {
        self.sorted.len()
    }
}

impl PartialEq for HashedIds {
    fn eq(&self, other: &Self) -> bool {
        self.sorted == other.sorted
    }
}

/// Fibonacci hashing: the product's high bits spread clustered hashes across the table.
const fn home(hash: u32, shift: u32) -> usize {
    (hash.wrapping_mul(0x9e37_79b9) >> shift) as usize
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Every entry resolves and every absent hash misses, matching a binary search, including
    /// hashes that collide in the table's high bits.
    #[test]
    fn lookups_match_the_canonical_entries() {
        for len in [0_u32, 1, 2, 3, 100, 15_000] {
            let mut state = 0x1234_5678_u32 ^ len;
            let mut entries = (0..len)
                .map(|id| {
                    state ^= state << 13;
                    state ^= state >> 17;
                    state ^= state << 5;
                    (state, id)
                })
                .chain((0..len.min(8)).map(|id| (id << 28, len + id)))
                .collect::<Vec<_>>();
            entries.sort_unstable();
            entries.dedup_by_key(|entry| entry.0);
            let ids = HashedIds::new(entries.clone().into_boxed_slice());
            for &(hash, id) in &entries {
                assert_eq!(ids.get(hash), Some(id));
            }
            for probe in (0..20_000_u32).map(|value| value.wrapping_mul(0x0101_0101)) {
                let expected = entries
                    .binary_search_by_key(&probe, |entry| entry.0)
                    .ok()
                    .map(|index| entries[index].1);
                assert_eq!(ids.get(probe), expected);
            }
        }
    }
}
