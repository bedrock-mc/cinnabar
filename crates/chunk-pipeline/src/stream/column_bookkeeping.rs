//! Column-local records and stable queue positions for retirement without whole-view scans.

use super::*;

/// Identifies the column owning a record; reservations have no column yet.
pub(super) trait ColumnOwned {
    /// Returns the owning column, or none for an unfilled reservation.
    fn column(&self) -> Option<ChunkKey>;
}

impl ColumnOwned for SubChunkKey {
    fn column(&self) -> Option<ChunkKey> {
        Some(self.chunk())
    }
}

impl ColumnOwned for PendingSubChunkRequest {
    fn column(&self) -> Option<ChunkKey> {
        Some(self.chunk)
    }
}

/// Stores records under their owning column instead of maintaining a duplicate key index.
#[derive(Debug)]
pub(super) struct ColumnRecords<K, V> {
    columns: BTreeMap<ChunkKey, BTreeMap<K, V>>,
    len: usize,
    #[cfg(test)]
    cleanup_visits: usize,
}

impl<K, V> Default for ColumnRecords<K, V> {
    fn default() -> Self {
        Self {
            columns: BTreeMap::new(),
            len: 0,
            #[cfg(test)]
            cleanup_visits: 0,
        }
    }
}

impl<K: ColumnOwned + Copy + Ord, V> ColumnRecords<K, V> {
    /// Counts live records in constant time.
    pub(super) fn len(&self) -> usize {
        self.len
    }

    /// Reads one record without visiting other columns.
    pub(super) fn get(&self, key: &K) -> Option<&V> {
        self.columns.get(&key.column()?)?.get(key)
    }

    /// Mutates one record without changing its ownership.
    pub(super) fn get_mut(&mut self, key: &K) -> Option<&mut V> {
        self.columns.get_mut(&key.column()?)?.get_mut(key)
    }

    /// Tests whether a record is live.
    pub(super) fn contains_key(&self, key: &K) -> bool {
        self.get(key).is_some()
    }

    /// Inserts or replaces a record in its authoritative column bucket.
    pub(super) fn insert(&mut self, key: K, value: V) -> Option<V> {
        let previous = self
            .columns
            .entry(key.column().expect("a record has a column"))
            .or_default()
            .insert(key, value);
        self.len += usize::from(previous.is_none());
        previous
    }

    /// Removes a record and any resulting empty column bucket.
    pub(super) fn remove(&mut self, key: &K) -> Option<V> {
        let column = key.column()?;
        let records = self.columns.get_mut(&column)?;
        let removed = records.remove(key);
        self.len -= usize::from(removed.is_some());
        if records.is_empty() {
            self.columns.remove(&column);
        }
        removed
    }

    /// Removes only records owned by this column.
    pub(super) fn remove_column(&mut self, column: ChunkKey) -> BTreeMap<K, V> {
        let records = self.columns.remove(&column).unwrap_or_default();
        self.len -= records.len();
        #[cfg(test)]
        {
            self.cleanup_visits += records.len();
        }
        records
    }

    /// Iterates live records for diagnostics and acknowledgment ordering.
    pub(super) fn iter(&self) -> impl Iterator<Item = (&K, &V)> {
        self.columns.values().flat_map(BTreeMap::iter)
    }

    /// Reads and resets the number of records examined during column cleanup.
    #[cfg(test)]
    pub(super) fn take_cleanup_visits(&mut self) -> usize {
        std::mem::take(&mut self.cleanup_visits)
    }
}

/// A queue with stable order keys and a derived column index updated by every mutation.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(super) struct ColumnQueue<T> {
    entries: BTreeMap<i128, T>,
    columns: BTreeMap<ChunkKey, BTreeSet<i128>>,
    front: i128,
    back: i128,
    #[cfg(test)]
    cleanup_visits: usize,
}

impl<T> Default for ColumnQueue<T> {
    fn default() -> Self {
        Self {
            entries: BTreeMap::new(),
            columns: BTreeMap::new(),
            front: 0,
            back: 0,
            #[cfg(test)]
            cleanup_visits: 0,
        }
    }
}

impl<T: ColumnOwned> ColumnQueue<T> {
    /// Counts live queue entries, including reservations.
    pub(super) fn len(&self) -> usize {
        self.entries.len()
    }
    /// Reports whether the live queue is empty.
    pub(super) fn is_empty(&self) -> bool {
        self.entries.is_empty()
    }
    /// Reads the first live entry.
    #[cfg(test)]
    pub(super) fn front(&self) -> Option<&T> {
        self.entries.first_key_value().map(|(_, entry)| entry)
    }
    /// Reads the last live entry.
    #[cfg(test)]
    pub(super) fn back(&self) -> Option<&T> {
        self.entries.last_key_value().map(|(_, entry)| entry)
    }
    /// Iterates live entries in dispatch order.
    pub(super) fn iter(&self) -> impl Iterator<Item = &T> {
        self.entries.values()
    }
    /// Iterates stable positions and live entries in dispatch order.
    pub(super) fn entries(&self) -> impl Iterator<Item = (i128, &T)> {
        self.entries
            .iter()
            .map(|(&position, entry)| (position, entry))
    }
    /// Reads a live entry by its stable position.
    pub(super) fn get(&self, position: i128) -> Option<&T> {
        self.entries.get(&position)
    }
    /// Appends a live entry without changing any existing position.
    pub(super) fn push_back(&mut self, entry: T) {
        let position = self.back;
        self.back = self
            .back
            .checked_add(1)
            .expect("queue position space exhausted");
        self.insert(position, entry);
    }
    /// Prepends a live entry without changing any existing position.
    pub(super) fn push_front(&mut self, entry: T) {
        self.front = self
            .front
            .checked_sub(1)
            .expect("queue position space exhausted");
        self.insert(self.front, entry);
    }
    /// Replaces a live payload and updates column ownership together.
    pub(super) fn replace(&mut self, position: i128, entry: T) {
        assert!(self.remove(position).is_some());
        self.insert(position, entry);
    }
    /// Adds a new payload and its derived column index together.
    fn insert(&mut self, position: i128, entry: T) {
        if let Some(column) = entry.column() {
            self.columns.entry(column).or_default().insert(position);
        }
        assert!(self.entries.insert(position, entry).is_none());
    }
    /// Removes a live payload and its derived column index together.
    pub(super) fn remove(&mut self, position: i128) -> Option<T> {
        let entry = self.entries.remove(&position)?;
        if let Some(column) = entry.column() {
            let positions = self.columns.get_mut(&column).expect("an indexed entry");
            positions.remove(&position);
            if positions.is_empty() {
                self.columns.remove(&column);
            }
        }
        Some(entry)
    }
    /// Pops the first live payload, with no stale tombstones to scan.
    pub(super) fn pop_front(&mut self) -> Option<T> {
        self.remove(*self.entries.first_key_value()?.0)
    }
    /// Removes matching live entries when a caller needs a queue-wide predicate.
    pub(super) fn retain(&mut self, keep: impl Fn(&T) -> bool) {
        let removed: Vec<_> = self
            .entries()
            .filter(|(_, entry)| !keep(entry))
            .map(|(position, _)| position)
            .collect();
        for position in removed {
            self.remove(position);
        }
    }
    /// Removes only this column's payloads and leaves reservations and order intact.
    pub(super) fn remove_column(&mut self, column: ChunkKey) -> Vec<T> {
        let positions = self.columns.remove(&column).unwrap_or_default();
        #[cfg(test)]
        {
            self.cleanup_visits += positions.len();
        }
        positions
            .into_iter()
            .map(|position| self.entries.remove(&position).expect("an indexed entry"))
            .collect()
    }
    /// Reads and resets the number of payloads examined during column cleanup.
    #[cfg(test)]
    pub(super) fn take_cleanup_visits(&mut self) -> usize {
        std::mem::take(&mut self.cleanup_visits)
    }
    /// Checks the derived ownership index against every authoritative payload.
    #[cfg(test)]
    pub(super) fn assert_consistent(&self) {
        let mut expected = BTreeMap::<ChunkKey, BTreeSet<i128>>::new();
        for (position, entry) in self.entries() {
            if let Some(column) = entry.column() {
                expected.entry(column).or_default().insert(position);
            }
        }
        assert_eq!(self.columns, expected);
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    /// Column buckets preserve replacement, removal and total-count semantics.
    #[test]
    fn column_records_keep_authoritative_counts_across_mutations() {
        let a = SubChunkKey::new(0, 1, 0, 0);
        let b = SubChunkKey::new(0, 1, 1, 0);
        let c = SubChunkKey::new(1, 1, 0, 0);
        let mut records = ColumnRecords::default();
        assert_eq!(records.insert(a, 1), None);
        assert_eq!(records.insert(b, 2), None);
        assert_eq!(records.insert(c, 3), None);
        assert_eq!(records.insert(a, 4), Some(1));
        assert_eq!(records.len(), records.iter().count());
        *records.get_mut(&b).unwrap() = 5;
        assert_eq!(records.remove(&a), Some(4));
        assert_eq!(records.remove(&a), None);
        assert_eq!(records.remove_column(a.chunk()), BTreeMap::from([(b, 5)]));
        assert_eq!(records.len(), records.iter().count());
        assert_eq!(records.get(&c), Some(&3));
        assert_eq!(records.remove(&c), Some(3));
        assert!(records.columns.is_empty());
        assert_eq!(records.len(), 0);
    }

    #[test]
    fn column_queue_preserves_order_and_index_across_mutations() {
        let a = SubChunkKey::new(0, 1, 0, 0);
        let b = SubChunkKey::new(0, 2, 0, 0);
        let mut queue = ColumnQueue::default();
        queue.push_back(a);
        queue.push_front(b);
        queue.push_back(b);
        queue.assert_consistent();
        queue.replace(0, b);
        queue.assert_consistent();
        assert_eq!(queue.pop_front(), Some(b));
        queue.assert_consistent();
        queue.push_front(a);
        queue.retain(|key| *key != a);
        queue.assert_consistent();
        assert_eq!(queue.remove_column(b.chunk()), vec![b, b]);
        queue.assert_consistent();
        assert!(queue.is_empty());
        queue.push_back(a);
        queue.push_front(b);
        queue.assert_consistent();
        assert_eq!(queue.pop_front(), Some(b));
        assert_eq!(queue.pop_front(), Some(a));
        queue.assert_consistent();
    }
}
