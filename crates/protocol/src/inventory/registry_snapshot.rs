use std::{num::NonZeroU64, sync::Arc};

use crate::{ItemRegistryEntry, MAX_ITEM_REGISTRY_ENTRIES};

#[derive(Debug, Clone, Copy, PartialEq, Eq, thiserror::Error)]
pub enum RecipeRegistryError {
    #[error("recipe registry exceeds the bounded entry policy")]
    TooManyEntries,
    #[error("recipe registry contains duplicate numeric bindings")]
    DuplicateBinding,
    #[error("recipe registry index allocation was refused")]
    AllocationRefused,
}

#[derive(Debug)]
struct RegistryEntries {
    revision: NonZeroU64,
    entries: Arc<[ItemRegistryEntry]>,
    index: Vec<usize>,
}

/// An immutable index tied to one original registry allocation and revision.
/// Clones share both; callers own revision progression and freshness. The
/// index has at most 16,384 usize entries and never copies registry strings.
#[derive(Debug, Clone)]
pub struct RecipeRegistrySnapshot(Arc<RegistryEntries>);

impl RecipeRegistrySnapshot {
    pub fn new(
        revision: NonZeroU64,
        entries: Arc<[ItemRegistryEntry]>,
    ) -> Result<Self, RecipeRegistryError> {
        if entries.len() > MAX_ITEM_REGISTRY_ENTRIES {
            return Err(RecipeRegistryError::TooManyEntries);
        }
        let mut index = Vec::new();
        index
            .try_reserve_exact(entries.len())
            .map_err(|_| RecipeRegistryError::AllocationRefused)?;
        index.extend(0..entries.len());
        index.sort_unstable_by_key(|&position| entries[position].network_id);
        if index
            .windows(2)
            .any(|pair| entries[pair[0]].network_id == entries[pair[1]].network_id)
        {
            return Err(RecipeRegistryError::DuplicateBinding);
        }
        Ok(Self(Arc::new(RegistryEntries {
            revision,
            entries,
            index,
        })))
    }

    pub fn revision(&self) -> NonZeroU64 {
        self.0.revision
    }
    pub fn entries(&self) -> &[ItemRegistryEntry] {
        &self.0.entries
    }
    pub fn get(&self, id: i32) -> Option<&ItemRegistryEntry> {
        let position = self
            .0
            .index
            .binary_search_by_key(&id, |&index| self.0.entries[index].network_id)
            .ok()?;
        Some(&self.0.entries[self.0.index[position]])
    }
    /// Identity includes the bound index allocation, not just a reused revision.
    pub fn same_authority(&self, other: &Self) -> bool {
        Arc::ptr_eq(&self.0, &other.0)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::ItemRegistryVersion;

    fn entry(id: i32) -> ItemRegistryEntry {
        ItemRegistryEntry {
            identifier: Arc::from("minecraft:oak_log"),
            network_id: id,
            component_based: false,
            version: ItemRegistryVersion::None,
            components: None,
            component_digest: [0; 32],
            negotiated_max_stack_size: Some(64),
            canonical_empty_component_data: true,
            item_tags: std::sync::Arc::from([]),
        }
    }

    #[test]
    fn index_borrows_original_allocation_and_clones_share_it() {
        let entries: Arc<[ItemRegistryEntry]> = vec![entry(7), entry(6)].into();
        let snapshot =
            RecipeRegistrySnapshot::new(NonZeroU64::new(1).unwrap(), Arc::clone(&entries)).unwrap();
        assert!(Arc::ptr_eq(&entries, &snapshot.0.entries));
        assert!(std::ptr::eq(snapshot.get(6).unwrap(), &entries[1]));
        let clone = snapshot.clone();
        assert!(snapshot.same_authority(&clone));
        let separate = RecipeRegistrySnapshot::new(NonZeroU64::new(1).unwrap(), entries).unwrap();
        assert!(!snapshot.same_authority(&separate));
    }

    #[test]
    fn duplicate_and_oversized_indexes_are_refused_and_boundary_is_accepted() {
        let revision = NonZeroU64::new(1).unwrap();
        assert!(matches!(
            RecipeRegistrySnapshot::new(revision, vec![entry(6), entry(6)].into()),
            Err(RecipeRegistryError::DuplicateBinding)
        ));
        let entries: Arc<[ItemRegistryEntry]> =
            (0..MAX_ITEM_REGISTRY_ENTRIES as i32).map(entry).collect();
        let snapshot = RecipeRegistrySnapshot::new(revision, entries).unwrap();
        assert_eq!(snapshot.0.index.len(), MAX_ITEM_REGISTRY_ENTRIES);
        assert!(snapshot.get(MAX_ITEM_REGISTRY_ENTRIES as i32 - 1).is_some());
        assert!(snapshot.get(MAX_ITEM_REGISTRY_ENTRIES as i32).is_none());
        assert!(matches!(
            RecipeRegistrySnapshot::new(
                revision,
                (0..=MAX_ITEM_REGISTRY_ENTRIES as i32).map(entry).collect()
            ),
            Err(RecipeRegistryError::TooManyEntries)
        ));
    }
}
