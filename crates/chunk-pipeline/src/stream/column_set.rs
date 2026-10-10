use std::sync::OnceLock;

use hashbrown::HashMap;
use world::{ChunkKey, SubChunkKey};

/// Stores each key once, grouped by column with sections in ascending height, so
/// membership and single-column queries cost one hash lookup plus a short column scan.
#[derive(Clone, Debug, Default)]
pub(super) struct ColumnSubChunkSet {
    /// Never holds an empty column.
    columns: HashMap<ChunkKey, Vec<SubChunkKey>>,
    len: usize,
    hash: OnceLock<u64>,
}

impl PartialEq for ColumnSubChunkSet {
    /// A cached diagnostic witness does not change residency identity.
    fn eq(&self, other: &Self) -> bool {
        self.len == other.len && self.columns == other.columns
    }
}

impl Eq for ColumnSubChunkSet {}

impl ColumnSubChunkSet {
    /// Reports membership without consulting a second index.
    pub(super) fn contains(&self, key: &SubChunkKey) -> bool {
        self.columns
            .get(&key.chunk())
            .is_some_and(|column| column.binary_search_by_key(&key.y, |key| key.y).is_ok())
    }

    /// Returns whether the key became resident in this set.
    pub(super) fn insert(&mut self, key: SubChunkKey) -> bool {
        let column = self.columns.entry(key.chunk()).or_default();
        let Err(at) = column.binary_search_by_key(&key.y, |key| key.y) else {
            return false;
        };
        column.insert(at, key);
        self.len += 1;
        self.hash.take();
        true
    }

    /// Removes only the specified section.
    pub(super) fn remove(&mut self, key: &SubChunkKey) -> bool {
        let chunk = key.chunk();
        let Some(column) = self.columns.get_mut(&chunk) else {
            return false;
        };
        let Ok(at) = column.binary_search_by_key(&key.y, |key| key.y) else {
            return false;
        };
        column.remove(at);
        if column.is_empty() {
            self.columns.remove(&chunk);
        }
        self.len -= 1;
        self.hash.take();
        true
    }

    /// Visits all keys in column order; callers needing public key order must sort.
    pub(super) fn iter(&self) -> impl Iterator<Item = &SubChunkKey> {
        let mut columns: Vec<_> = self.columns.iter().collect();
        columns.sort_unstable_by_key(|(column, _)| **column);
        columns
            .into_iter()
            .flat_map(|(_, keys)| keys.iter())
            .map(visited_key)
    }

    /// Visits one column's sections in ascending height and no unrelated sections.
    pub(super) fn column(&self, column: ChunkKey) -> impl Iterator<Item = &SubChunkKey> {
        self.columns
            .get(&column)
            .into_iter()
            .flatten()
            .map(visited_key)
    }

    /// Each column holding a key, in no particular order, visiting one key per column.
    pub(super) fn columns(&self) -> impl Iterator<Item = ChunkKey> + '_ {
        self.columns
            .values()
            .map(|keys| visited_key(&keys[0]).chunk())
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
        self.len
    }

    /// Reports whether any section remains in the test fixture.
    #[cfg(test)]
    pub(super) fn is_empty(&self) -> bool {
        self.len == 0
    }

    /// Resets fixture residency without changing other stream state.
    #[cfg(test)]
    pub(super) fn clear(&mut self) {
        if self.len != 0 {
            self.columns.clear();
            self.len = 0;
            self.hash.take();
        }
    }
}

impl Extend<SubChunkKey> for ColumnSubChunkSet {
    /// Admits a batch into the same column index.
    fn extend<T: IntoIterator<Item = SubChunkKey>>(&mut self, keys: T) {
        for key in keys {
            self.insert(key);
        }
    }
}

#[cfg(test)]
thread_local! {
    static VISITED_KEYS: std::cell::Cell<usize> = const { std::cell::Cell::new(0) };
}

/// Exposes the stored key and counts actual iterator visits in deterministic tests.
fn visited_key(key: &SubChunkKey) -> &SubChunkKey {
    #[cfg(test)]
    VISITED_KEYS.set(VISITED_KEYS.get() + 1);
    key
}

/// Starts a fresh count of keys visited by residency queries on this test thread.
#[cfg(test)]
pub(super) fn take_visited_keys() -> usize {
    VISITED_KEYS.replace(0)
}
