//! Retained creative search results follow the catalog, registry and query.

use std::{collections::BTreeMap, sync::Arc};

use protocol::{CreativeCategory, CreativeContentEvent, CreativeItem, ItemRegistryEntry};

use super::{ScreenState, tab_category};
use inventory::inventory_ledger::PlayerInventoryLedger;

#[derive(Clone, Debug)]
pub(super) struct CreativeFilterCache {
    catalog: CreativeContentEvent,
    registry: Option<Arc<BTreeMap<i32, ItemRegistryEntry>>>,
    tab: u8,
    search: String,
    indexes: Arc<[usize]>,
}

/// A retained result view borrows catalog items without allocating a reference list.
pub struct CreativeEntries<'a> {
    items: &'a [CreativeItem],
    indexes: Option<Arc<[usize]>>,
}

impl<'a> CreativeEntries<'a> {
    /// Returns the number of retained matches.
    pub fn len(&self) -> usize {
        self.indexes.as_ref().map_or(0, |indexes| indexes.len())
    }

    /// Reports whether the current filter has no matches.
    pub fn is_empty(&self) -> bool {
        self.len() == 0
    }

    /// Resolves one retained match into the borrowed catalog.
    pub fn get(&self, index: usize) -> Option<&'a CreativeItem> {
        self.indexes
            .as_ref()?
            .get(index)
            .map(|index| &self.items[*index])
    }

    /// Iterates the retained matches without rebuilding their reference list.
    pub fn iter(&self) -> impl ExactSizeIterator<Item = &'a CreativeItem> + '_ {
        self.indexes
            .as_deref()
            .unwrap_or(&[])
            .iter()
            .map(|index| &self.items[*index])
    }
}

impl ScreenState {
    /// Reuses filtered indexes while catalog identity and search inputs match.
    pub(crate) fn matching_creative_entries<'a>(
        &self,
        ledger: &'a PlayerInventoryLedger,
        name_of: impl Fn(&CreativeItem) -> Option<String>,
    ) -> CreativeEntries<'a> {
        let Some(catalog) = ledger.creative_catalog() else {
            return CreativeEntries {
                items: &[],
                indexes: None,
            };
        };
        let registry = ledger.item_registry_snapshot();
        let mut cache = self
            .creative_filter
            .lock()
            .expect("creative filter lock poisoned");
        let reusable = cache.as_ref().is_some_and(|cached| {
            Arc::ptr_eq(&cached.catalog.items, &catalog.items)
                && Arc::ptr_eq(&cached.catalog.groups, &catalog.groups)
                && cached.tab == self.creative_tab
                && cached.search == self.search
                && match (cached.registry.as_ref(), registry) {
                    (Some(previous), Some(current)) => Arc::ptr_eq(previous, current),
                    (None, None) => true,
                    _ => false,
                }
        });
        if !reusable {
            *cache = Some(CreativeFilterCache {
                catalog: catalog.clone(),
                registry: registry.cloned(),
                tab: self.creative_tab,
                search: self.search.clone(),
                indexes: creative_entry_indexes(catalog, self.creative_tab, &self.search, name_of)
                    .into(),
            });
        }
        CreativeEntries {
            items: &catalog.items,
            indexes: Some(Arc::clone(
                &cache
                    .as_ref()
                    .expect("the current creative filter was retained")
                    .indexes,
            )),
        }
    }
}

/// The catalog entries a tab shows; the search tab shows every entry whose
/// name contains the text.
pub fn creative_entries<'a>(
    catalog: &'a CreativeContentEvent,
    tab: u8,
    search: &str,
    name_of: impl Fn(&CreativeItem) -> Option<String>,
) -> Vec<&'a CreativeItem> {
    creative_entry_indexes(catalog, tab, search, name_of)
        .into_iter()
        .map(|index| &catalog.items[index])
        .collect()
}

/// Computes catalog order once for a changed tab or search.
fn creative_entry_indexes(
    catalog: &CreativeContentEvent,
    tab: u8,
    search: &str,
    name_of: impl Fn(&CreativeItem) -> Option<String>,
) -> Vec<usize> {
    let needle = search.to_lowercase();
    catalog
        .items
        .iter()
        .enumerate()
        .filter(|(_, item)| {
            let category = catalog
                .groups
                .get(item.group as usize)
                .map(|group| group.category);
            if category == Some(CreativeCategory::CommandOnly) {
                return false;
            }
            match tab_category(tab) {
                Some(wanted) => category == Some(wanted),
                None => {
                    needle.is_empty()
                        || name_of(item).is_some_and(|name| name.to_lowercase().contains(&needle))
                }
            }
        })
        .map(|(index, _)| index)
        .collect()
}

#[cfg(test)]
mod tests {
    use protocol::{
        CreativeCategory, CreativeGroup, InventoryEvent, ItemRegistryEvent, NetworkItemStack,
    };
    use std::cell::Cell;
    use {super::*, inventory::inventory_ledger::PlayerInventoryLedger};

    #[test]
    fn creative_search_only_rescans_when_its_inputs_change() {
        let mut ledger = PlayerInventoryLedger::default();
        let catalog = CreativeContentEvent {
            groups: Arc::from([CreativeGroup {
                category: CreativeCategory::Nature,
                name: Arc::from(""),
                icon: None,
            }]),
            items: Arc::from([CreativeItem {
                creative_network_id: 1,
                stack: NetworkItemStack::default(),
                group: 0,
            }]),
            skipped: 0,
        };
        ledger.apply(&InventoryEvent::Creative(catalog.clone()));
        let mut state = ScreenState {
            creative_tab: crate::ui_runtime::presentation::screens::SEARCH_TAB,
            search: "stone".into(),
            ..Default::default()
        };
        let scans = Cell::new(0);
        let name = |_: &CreativeItem| {
            scans.set(scans.get() + 1);
            Some("Stone".into())
        };
        let retained = state.matching_creative_entries(&ledger, name);
        assert_eq!(retained.len(), 1);
        for _ in 0..3 {
            let (repeated, allocations) =
                crate::allocation_count::count(|| state.matching_creative_entries(&ledger, name));
            assert_eq!(allocations, 0);
            assert_eq!(repeated.len(), 1);
            assert!(Arc::ptr_eq(
                retained.indexes.as_ref().unwrap(),
                repeated.indexes.as_ref().unwrap()
            ));
        }
        assert_eq!(scans.get(), 1);
        state.search = "dirt".into();
        assert!(state.matching_creative_entries(&ledger, name).is_empty());
        assert_eq!(scans.get(), 2);
        ledger.apply_registry(&ItemRegistryEvent {
            entries: Arc::from([]),
        });
        assert!(state.matching_creative_entries(&ledger, name).is_empty());
        assert_eq!(scans.get(), 3);
        ledger.apply(&InventoryEvent::Creative(CreativeContentEvent {
            items: catalog.items.to_vec().into(),
            ..catalog
        }));
        assert!(state.matching_creative_entries(&ledger, name).is_empty());
        assert_eq!(scans.get(), 4);
    }
}
