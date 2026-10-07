//! Small section snapshots keep one light neighbourhood inline and spill only for column solves.

use hashbrown::HashMap;

use crate::SubChunkKey;

const INLINE_SECTIONS: usize = 7;

/// A small set of section handles with allocation-free single-job capture.
#[derive(Debug, Clone)]
pub struct SectionSnapshot<T> {
    inline: [Option<(SubChunkKey, T)>; INLINE_SECTIONS],
    overflow: Option<HashMap<SubChunkKey, T>>,
}

impl<T> Default for SectionSnapshot<T> {
    fn default() -> Self {
        Self {
            inline: std::array::from_fn(|_| None),
            overflow: None,
        }
    }
}

impl<T> SectionSnapshot<T> {
    /// Keeps a single stencil inline; merged column jobs move all handles to a hash table.
    pub fn insert(&mut self, key: SubChunkKey, value: T) {
        if let Some(overflow) = &mut self.overflow {
            overflow.insert(key, value);
            return;
        }
        for entry in &mut self.inline {
            if entry.as_ref().is_none_or(|(old, _)| *old == key) {
                *entry = Some((key, value));
                return;
            }
        }
        let mut overflow = HashMap::with_capacity(INLINE_SECTIONS + 1);
        overflow.extend(self.inline.iter_mut().filter_map(Option::take));
        overflow.insert(key, value);
        self.overflow = Some(overflow);
    }

    /// Removes snapshot-local metadata while keeping inline entries contiguous.
    pub fn remove(&mut self, key: &SubChunkKey) -> Option<T> {
        if let Some(overflow) = &mut self.overflow {
            return overflow.remove(key);
        }
        if let Some(index) = self
            .inline
            .iter()
            .position(|entry| entry.as_ref().is_some_and(|(old, _)| old == key))
        {
            let (_, value) = self.inline[index].take()?;
            self.inline[index..].rotate_left(1);
            Some(value)
        } else {
            None
        }
    }

    /// Borrows the captured value without consulting the mutable world.
    pub fn get(&self, key: &SubChunkKey) -> Option<&T> {
        if let Some(overflow) = &self.overflow {
            return overflow.get(key);
        }
        self.inline
            .iter()
            .flatten()
            .find(|(old, _)| old == key)
            .map(|(_, value)| value)
    }

    /// Updates snapshot-local metadata without changing its shared section payloads.
    pub fn get_mut(&mut self, key: &SubChunkKey) -> Option<&mut T> {
        if let Some(overflow) = &mut self.overflow {
            return overflow.get_mut(key);
        }
        self.inline
            .iter_mut()
            .flatten()
            .find(|(old, _)| old == key)
            .map(|(_, value)| value)
    }

    /// Reports whether the dispatch captured this section.
    pub fn contains_key(&self, key: &SubChunkKey) -> bool {
        self.get(key).is_some()
    }

    /// Visits captured entries; callers must not depend on insertion order.
    pub fn iter(&self) -> impl Iterator<Item = (&SubChunkKey, &T)> {
        self.inline
            .iter()
            .flatten()
            .map(|(key, value)| (key, value))
            .chain(self.overflow.iter().flat_map(|entries| entries.iter()))
    }

    /// Visits each payload once, including overflow from merged column jobs.
    pub fn values(&self) -> impl Iterator<Item = &T> {
        self.iter().map(|(_, value)| value)
    }
}

impl<T> IntoIterator for SectionSnapshot<T> {
    type Item = (SubChunkKey, T);
    type IntoIter = std::iter::Chain<
        std::iter::Flatten<std::array::IntoIter<Option<(SubChunkKey, T)>, INLINE_SECTIONS>>,
        std::iter::Flatten<std::option::IntoIter<HashMap<SubChunkKey, T>>>,
    >;

    fn into_iter(self) -> Self::IntoIter {
        self.inline
            .into_iter()
            .flatten()
            .chain(self.overflow.into_iter().flatten())
    }
}

impl<T> Extend<(SubChunkKey, T)> for SectionSnapshot<T> {
    fn extend<I: IntoIterator<Item = (SubChunkKey, T)>>(&mut self, entries: I) {
        for (key, value) in entries {
            self.insert(key, value);
        }
    }
}

impl<T> FromIterator<(SubChunkKey, T)> for SectionSnapshot<T> {
    fn from_iter<I: IntoIterator<Item = (SubChunkKey, T)>>(entries: I) -> Self {
        let mut snapshot = Self::default();
        snapshot.extend(entries);
        snapshot
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::collections::BTreeMap;

    /// Repeated keys and column overflow retain exactly the latest snapshot-local value.
    #[test]
    fn merged_sections_match_a_map() {
        let mut snapshot = SectionSnapshot::default();
        let mut expected = BTreeMap::new();
        for index in 0..256 {
            let key = SubChunkKey::new(0, index % 3, (index * 13) % 31, index % 5);
            snapshot.insert(key, index);
            expected.insert(key, index);
        }
        assert_eq!(
            snapshot
                .iter()
                .map(|(&key, &value)| (key, value))
                .collect::<BTreeMap<_, _>>(),
            expected
        );
        for (key, value) in expected {
            assert_eq!(snapshot.get(&key), Some(&value));
        }
        assert_eq!(
            snapshot.clone().into_iter().count(),
            snapshot.iter().count()
        );
        let keys: Vec<_> = snapshot.iter().map(|(&key, _)| key).collect();
        for key in &keys {
            assert!(snapshot.remove(key).is_some());
        }
        assert_eq!(snapshot.iter().count(), 0);
        for &key in &keys {
            snapshot.insert(key, 41);
        }
        for key in keys {
            assert!(snapshot.remove(&key).is_some());
            assert!(snapshot.get(&key).is_none());
            snapshot.insert(key, 42);
            assert_eq!(snapshot.get(&key), Some(&42));
            assert_eq!(
                snapshot.iter().filter(|(entry, _)| **entry == key).count(),
                1
            );
        }
    }
}
