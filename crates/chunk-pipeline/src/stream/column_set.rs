use std::{collections::BTreeMap, sync::OnceLock};

use hashbrown::HashMap;
use world::{ChunkKey, SubChunkKey};

/// Stores each key once, grouped by column with sections in ascending height, so
/// membership and single-column queries cost one hash lookup plus a short column scan.
#[derive(Clone, Debug, Default)]
pub(super) struct ColumnSubChunkSet {
    /// Never holds an empty column.
    columns: HashMap<ChunkKey, Vec<SubChunkKey>>,
    /// Updated only when a column gains its first key or loses its last one.
    ordered_columns: BTreeMap<ChunkKey, u64>,
    /// Advances when a column first gains a section, including after eviction.
    column_generation: u64,
    /// Advances on every actual section-membership change, independently of column age.
    membership_revision: u64,
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
        if column.is_empty() {
            self.column_generation += 1;
            self.ordered_columns
                .insert(key.chunk(), self.column_generation);
        }
        let Err(at) = column.binary_search_by_key(&key.y, |key| key.y) else {
            return false;
        };
        column.insert(at, key);
        self.len += 1;
        self.membership_revision += 1;
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
            self.ordered_columns.remove(&chunk);
        }
        self.len -= 1;
        self.membership_revision += 1;
        self.hash.take();
        true
    }

    /// Replaces one column's sections without making a resend look newly resident.
    pub(super) fn replace_column(
        &mut self,
        column: ChunkKey,
        keys: impl IntoIterator<Item = SubChunkKey>,
    ) {
        let mut replacement: Vec<_> = keys.into_iter().collect();
        assert!(replacement.iter().all(|key| key.chunk() == column));
        replacement.sort_unstable();
        replacement.dedup();
        if self.columns.get(&column).map_or(&[][..], Vec::as_slice) == replacement {
            return;
        }
        self.membership_revision += 1;
        let added = replacement.len();
        let removed = if replacement.is_empty() {
            self.ordered_columns.remove(&column);
            self.columns.remove(&column).map_or(0, |old| old.len())
        } else {
            if !self.ordered_columns.contains_key(&column) {
                self.column_generation += 1;
                self.ordered_columns.insert(column, self.column_generation);
            }
            self.columns
                .insert(column, replacement)
                .map_or(0, |old| old.len())
        };
        self.len = self.len - removed + added;
        self.hash.take();
    }

    /// Visits all keys in column order; callers needing public key order must sort.
    pub(super) fn iter(&self) -> impl Iterator<Item = &SubChunkKey> {
        self.ordered_columns
            .keys()
            .flat_map(|column| self.columns[column].iter())
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

    /// Finds the next populated column without scanning or cloning unrelated columns.
    pub(super) fn next_column(&self, cursor: Option<ChunkKey>) -> Option<ChunkKey> {
        self.ordered_columns
            .range(super::column_scan::after(cursor))
            .next()
            .map(|(column, _)| *column)
    }

    /// Seeks the next section in column/height order without scanning earlier sections.
    pub(super) fn next_key(&self, cursor: Option<SubChunkKey>) -> Option<SubChunkKey> {
        if let Some(cursor) = cursor
            && let Some(keys) = self.columns.get(&cursor.chunk())
            && let Some(key) = keys.get(keys.partition_point(|key| key.y <= cursor.y))
        {
            return Some(*key);
        }
        let column = self.next_column(cursor.map(SubChunkKey::chunk))?;
        self.columns.get(&column)?.first().copied()
    }

    /// Captures column membership age without copying a resident view.
    pub(super) fn column_generation(&self) -> u64 {
        self.column_generation
    }

    /// Identifies exact section membership without changing column-age checkpoints.
    pub(super) fn membership_revision(&self) -> u64 {
        self.membership_revision
    }

    /// Reports whether the current column existed at a captured membership generation.
    pub(super) fn existed_at(&self, column: ChunkKey, generation: u64) -> bool {
        self.ordered_columns
            .get(&column)
            .is_some_and(|created| *created <= generation)
    }

    /// Detaches storage while preserving the clock used by active membership checkpoints.
    pub(super) fn retire(&mut self) -> Self {
        let retired = std::mem::take(self);
        self.column_generation = retired.column_generation;
        self.membership_revision = retired.membership_revision + u64::from(retired.len != 0);
        retired
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
            self.ordered_columns.clear();
            self.len = 0;
            self.membership_revision += 1;
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
