//! A bounded map that evicts its least recently used entries when full.

use std::borrow::Borrow;
use std::cell::Cell;
use std::collections::HashMap;
use std::hash::Hash;

pub(crate) struct Lru<K, V> {
    entries: HashMap<K, (V, Cell<u64>)>,
    clock: Cell<u64>,
    capacity: usize,
}

impl<K: Eq + Hash, V> Lru<K, V> {
    pub(crate) fn new(capacity: usize) -> Self {
        Self {
            entries: HashMap::new(),
            clock: Cell::new(0),
            capacity: capacity.max(1),
        }
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

    pub(crate) fn retain(&mut self, mut keep: impl FnMut(&V) -> bool) {
        self.entries.retain(|_, (value, _)| keep(value));
    }

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
    fn replacing_a_key_never_evicts() {
        let mut cache = Lru::new(2);
        cache.insert("a", 1);
        cache.insert("b", 2);
        cache.insert("a", 3);
        assert_eq!((cache.get("a"), cache.get("b")), (Some(&3), Some(&2)));
    }
}
