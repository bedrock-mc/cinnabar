use std::{cmp::Ordering, collections::BTreeSet, ops::Bound, sync::OnceLock};

use world::{ChunkKey, SubChunkKey};

/// Stores each key once, with vertical sections adjacent for bounded column queries.
#[derive(Clone, Debug, Default)]
pub(super) struct ColumnSubChunkSet {
    keys: BTreeSet<ColumnKey>,
    hash: OnceLock<u64>,
}

impl PartialEq for ColumnSubChunkSet {
    /// A cached diagnostic witness does not change residency identity.
    fn eq(&self, other: &Self) -> bool {
        self.keys == other.keys
    }
}

impl Eq for ColumnSubChunkSet {}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
struct ColumnKey(SubChunkKey);

impl Ord for ColumnKey {
    /// Keeps every section in a column contiguous, including custom world heights.
    fn cmp(&self, other: &Self) -> Ordering {
        (self.0.chunk(), self.0.y).cmp(&(other.0.chunk(), other.0.y))
    }
}

impl PartialOrd for ColumnKey {
    /// Uses the same total column ordering as the tree.
    fn partial_cmp(&self, other: &Self) -> Option<Ordering> {
        Some(self.cmp(other))
    }
}

impl ColumnSubChunkSet {
    /// Reports membership without consulting a second index.
    pub(super) fn contains(&self, key: &SubChunkKey) -> bool {
        self.keys.contains(&ColumnKey(*key))
    }

    /// Returns whether the key became resident in this set.
    pub(super) fn insert(&mut self, key: SubChunkKey) -> bool {
        let inserted = self.keys.insert(ColumnKey(key));
        if inserted {
            self.hash.take();
        }
        inserted
    }

    /// Removes only the specified section.
    pub(super) fn remove(&mut self, key: &SubChunkKey) -> bool {
        let removed = self.keys.remove(&ColumnKey(*key));
        if removed {
            self.hash.take();
        }
        removed
    }

    /// Visits all keys in column order; callers needing public key order must sort.
    pub(super) fn iter(&self) -> impl Iterator<Item = &SubChunkKey> {
        self.keys.iter().map(visited_key)
    }

    /// Seeks directly to one column and visits no unrelated sections.
    pub(super) fn column(&self, column: ChunkKey) -> impl Iterator<Item = &SubChunkKey> {
        let first = ColumnKey(SubChunkKey::from_chunk(column, i32::MIN));
        let last = ColumnKey(SubChunkKey::from_chunk(column, i32::MAX));
        self.keys.range(first..=last).map(visited_key)
    }

    /// Each column holding a key, visiting one key per column.
    pub(super) fn columns(&self) -> impl Iterator<Item = ChunkKey> + '_ {
        let mut next = self.keys.first().map(|key| visited_key(key).chunk());
        std::iter::from_fn(move || {
            let column = next?;
            let last = ColumnKey(SubChunkKey::from_chunk(column, i32::MAX));
            next = self
                .keys
                .range((Bound::Excluded(last), Bound::Unbounded))
                .next()
                .map(|key| visited_key(key).chunk());
            Some(column)
        })
    }

    /// Caches the legacy key-order witness until membership actually changes.
    pub(super) fn deterministic_hash(&self) -> u64 {
        *self.hash.get_or_init(|| {
            let mut keys = self.iter().copied().collect::<Vec<_>>();
            keys.sort_unstable();
            super::helpers::deterministic_sub_chunk_key_hash(&keys)
        })
    }

    /// Returns the number of unique resident sections.
    pub(super) fn len(&self) -> usize {
        self.keys.len()
    }

    /// Reports whether any section remains in the test fixture.
    #[cfg(test)]
    pub(super) fn is_empty(&self) -> bool {
        self.keys.is_empty()
    }

    /// Resets fixture residency without changing other stream state.
    #[cfg(test)]
    pub(super) fn clear(&mut self) {
        if !self.keys.is_empty() {
            self.keys.clear();
            self.hash.take();
        }
    }
}

impl Extend<SubChunkKey> for ColumnSubChunkSet {
    /// Admits a batch into the same authoritative column tree.
    fn extend<T: IntoIterator<Item = SubChunkKey>>(&mut self, keys: T) {
        let previous_len = self.keys.len();
        self.keys.extend(keys.into_iter().map(ColumnKey));
        if self.keys.len() != previous_len {
            self.hash.take();
        }
    }
}

#[cfg(test)]
thread_local! {
    static VISITED_KEYS: std::cell::Cell<usize> = const { std::cell::Cell::new(0) };
}

/// Exposes the stored key and counts actual iterator visits in deterministic tests.
fn visited_key(key: &ColumnKey) -> &SubChunkKey {
    #[cfg(test)]
    VISITED_KEYS.set(VISITED_KEYS.get() + 1);
    &key.0
}

/// Starts a fresh count of keys visited by residency queries on this test thread.
#[cfg(test)]
pub(super) fn take_visited_keys() -> usize {
    VISITED_KEYS.replace(0)
}
