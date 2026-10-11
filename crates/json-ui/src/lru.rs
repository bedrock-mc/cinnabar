//! A bounded map that evicts its least recently used entries when full.

use std::borrow::Borrow;
use std::cell::Cell;
use std::collections::HashMap;
use std::hash::Hash;

pub(crate) struct Lru<K, V> {
    entries: HashMap<K, (V, Cell<u64>)>,
    clock: Cell<u64>,
    capacity: usize,
    /// The size at which [`Self::prune_dead`] next scans.
    prune_at: usize,
}

/// The smallest size [`Lru::prune_dead`] scans at.
const PRUNE_FLOOR: usize = 256;

impl<K: Eq + Hash, V> Lru<K, V> {
    pub(crate) fn new(capacity: usize) -> Self {
        Self {
            entries: HashMap::new(),
            clock: Cell::new(0),
            capacity: capacity.max(1),
            prune_at: PRUNE_FLOOR,
        }
    }

    /// Drops the entries `live` rejects once the map has doubled since the last scan, so entries
    /// whose owners died, such as a replaced catalog's trees, leave soon after rather than only
    /// once the map fills. The scan amortizes over the growth that triggers it.
    pub(crate) fn prune_dead(&mut self, mut live: impl FnMut(&V) -> bool) {
        if self.entries.len() < self.prune_at {
            return;
        }
        self.entries.retain(|_, (value, _)| live(value));
        self.prune_at = (self.entries.len() * 2).max(PRUNE_FLOOR);
    }

    /// A hit counts as a use, so it survives the next eviction.
    pub(crate) fn get<Q>(&self, key: &Q) -> Option<&V>
    where
        K: Borrow<Q>,
        Q: Eq + Hash + ?Sized,
    {
        let (value, used) = self.entries.get(key)?;
        used.set(self.tick());
        Some(value)
    }

    pub(crate) fn insert(&mut self, key: K, value: V) {
        if self.entries.len() >= self.capacity && !self.entries.contains_key(&key) {
            self.evict();
        }
        let used = Cell::new(self.tick());
        self.entries.insert(key, (value, used));
    }

    #[cfg(test)]
    pub(crate) fn len(&self) -> usize {
        self.entries.len()
    }

    #[cfg(test)]
    pub(crate) fn contains_key<Q>(&self, key: &Q) -> bool
    where
        K: Borrow<Q>,
        Q: Eq + Hash + ?Sized,
    {
        self.entries.contains_key(key)
    }

    #[cfg(test)]
    pub(crate) fn clear(&mut self) {
        self.entries.clear();
    }

    fn tick(&self) -> u64 {
        let now = self.clock.get() + 1;
        self.clock.set(now);
        now
    }

    /// Drops the least recently used quarter at once, so the scan amortizes over the refill.
    fn evict(&mut self) {
        let mut stamps: Vec<u64> = self.entries.values().map(|(_, used)| used.get()).collect();
        if stamps.is_empty() {
            return;
        }
        let last = (self.capacity / 4).clamp(1, stamps.len()) - 1;
        let cutoff = *stamps.select_nth_unstable(last).1;
        self.entries.retain(|_, (_, used)| used.get() > cutoff);
    }
}

#[cfg(test)]
mod tests {
    use super::Lru;

    #[test]
    fn eviction_keeps_recently_used_entries() {
        let mut cache = Lru::new(8);
        for key in 0..8 {
            cache.insert(key, key);
        }
        for key in [0, 1, 2] {
            assert_eq!(cache.get(&key), Some(&key));
        }
        cache.insert(8, 8);
        assert!(cache.len() <= 8);
        for key in [0, 1, 2, 8] {
            assert_eq!(cache.get(&key), Some(&key), "recent key {key} evicted");
        }
        assert_eq!(cache.get(&3), None, "least recently used key survived");
        assert_eq!(cache.get(&4), None, "least recently used key survived");
    }

    #[test]
    fn dead_entries_leave_once_the_map_doubles() {
        use std::sync::{Arc, Weak};
        let mut cache: Lru<usize, Weak<()>> = Lru::new(4096);
        let owners: Vec<Arc<()>> = (0..300).map(|_| Arc::new(())).collect();
        for (key, owner) in owners.iter().enumerate() {
            cache.prune_dead(|entry| entry.strong_count() != 0);
            cache.insert(key, Arc::downgrade(owner));
        }
        drop(owners);
        let live = Arc::new(());
        for key in 300..600 {
            cache.prune_dead(|entry| entry.strong_count() != 0);
            cache.insert(key, Arc::downgrade(&live));
        }
        assert!(
            cache.len() <= 300,
            "{} entries kept after their owners died",
            cache.len()
        );
        assert_eq!(cache.get(&599).map(Weak::strong_count), Some(1));
    }

    #[test]
    fn replacing_a_key_never_evicts() {
        let mut cache = Lru::new(2);
        cache.insert("a", 1);
        cache.insert("b", 2);
        cache.insert("a", 3);
        assert_eq!((cache.get("a"), cache.get("b")), (Some(&3), Some(&2)));
    }
}
