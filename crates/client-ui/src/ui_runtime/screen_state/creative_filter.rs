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
    name_generation: u64,
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
        name_generation: u64,
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
                && cached.name_generation == name_generation
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
                name_generation,
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
        let retained = state.matching_creative_entries(&ledger, 0, name);
        assert_eq!(retained.len(), 1);
        for _ in 0..3 {
            let (repeated, allocations) = crate::allocation_count::count(|| {
                state.matching_creative_entries(&ledger, 0, name)
            });
            assert_eq!(allocations, 0);
            assert_eq!(repeated.len(), 1);
            assert!(Arc::ptr_eq(
                retained.indexes.as_ref().unwrap(),
                repeated.indexes.as_ref().unwrap()
            ));
        }
        assert_eq!(scans.get(), 1);
        state.search = "dirt".into();
        assert!(state.matching_creative_entries(&ledger, 0, name).is_empty());
        assert_eq!(scans.get(), 2);
        ledger.apply_registry(&ItemRegistryEvent {
            entries: Arc::from([]),
        });
        assert!(state.matching_creative_entries(&ledger, 0, name).is_empty());
        assert_eq!(scans.get(), 3);
        ledger.apply(&InventoryEvent::Creative(CreativeContentEvent {
            items: catalog.items.to_vec().into(),
            ..catalog
        }));
        assert!(state.matching_creative_entries(&ledger, 0, name).is_empty());
        assert_eq!(scans.get(), 4);
    }
}

#[cfg(test)]
mod display_name_tests {
    use super::*;
    use crate::ui_runtime::{
        UiRuntime, inventory_actions::visible_creative_entries, item_facts::SessionItemComponents,
    };
    use protocol::{CreativeGroup, InventoryEvent, ItemRegistryEvent, NetworkItemStack};

    /// Gives search a custom item and a vanilla item with an independently retained registry.
    fn catalog() -> PlayerInventoryLedger {
        let mut ledger = PlayerInventoryLedger::default();
        ledger.apply_registry(&ItemRegistryEvent {
            entries: Arc::from([(1, "custom:blade"), (2, "minecraft:stone")].map(
                |(network_id, identifier)| ItemRegistryEntry {
                    network_id,
                    identifier: identifier.into(),
                    component_based: network_id == 1,
                    version: protocol::ItemRegistryVersion::None,
                    component_digest: [0; 32],
                    negotiated_max_stack_size: None,
                    canonical_empty_component_data: true,
                    item_tags: Arc::from([]),
                },
            )),
        });
        ledger.apply(&InventoryEvent::Creative(CreativeContentEvent {
            groups: Arc::from([CreativeGroup {
                category: CreativeCategory::Nature,
                name: "".into(),
                icon: None,
            }]),
            items: Arc::from([1, 2].map(|network_id| CreativeItem {
                creative_network_id: network_id as u32,
                stack: NetworkItemStack {
                    network_id,
                    count: 1,
                    ..NetworkItemStack::empty()
                },
                group: 0,
            })),
            skipped: 0,
        }));
        ledger
    }

    /// Publishes a single component display-name projection without changing catalog identity.
    fn display_name(runtime: &mut UiRuntime, name: &str) {
        runtime.set_session_items(Some(Arc::new(SessionItemComponents::from_iter([(
            Arc::from("custom:blade"),
            protocol::ItemComponents {
                display_name: Some(Arc::from(name)),
                ..Default::default()
            },
        )]))));
    }

    /// Encodes a tiny active-language table for search and tooltip projection.
    fn language(key: &str, value: &str) -> Arc<assets::RuntimeLangCatalog> {
        let entries = [assets::LangEntry {
            key: key.into(),
            value: value.into(),
        }];
        let bytes = assets::encode_lang_catalog([11; 32], [12; 32], &entries).unwrap();
        Arc::new(assets::RuntimeLangCatalog::decode(&bytes).unwrap())
    }

    #[test]
    fn custom_display_names_and_component_reload_change_search_results() {
        let ledger = catalog();
        let mut runtime = UiRuntime::new(1);
        runtime.screen_state_mut().creative_tab =
            crate::ui_runtime::presentation::screens::SEARCH_TAB;
        runtime.screen_state_mut().search = "crystal".into();
        display_name(&mut runtime, "Crystal Blade");
        assert_eq!(
            visible_creative_entries(&ledger, &runtime)
                .get(0)
                .unwrap()
                .stack
                .network_id,
            1
        );
        let (_, allocations) =
            crate::allocation_count::count(|| visible_creative_entries(&ledger, &runtime));
        assert_eq!(allocations, 0);
        display_name(&mut runtime, "Ruby Blade");
        assert!(visible_creative_entries(&ledger, &runtime).is_empty());
        runtime.screen_state_mut().search = "ruby".into();
        assert_eq!(visible_creative_entries(&ledger, &runtime).len(), 1);
    }

    #[test]
    fn active_language_and_component_translation_invalidate_cached_names() {
        let ledger = catalog();
        let mut runtime = UiRuntime::new(1);
        runtime.screen_state_mut().creative_tab =
            crate::ui_runtime::presentation::screens::SEARCH_TAB;
        runtime.screen_state_mut().search = "pierre".into();
        runtime.set_active_language(Some(language("tile.stone.name", "Pierre")));
        assert_eq!(
            visible_creative_entries(&ledger, &runtime)
                .get(0)
                .unwrap()
                .stack
                .network_id,
            2
        );
        runtime.set_active_language(Some(language("tile.stone.name", "Stein")));
        assert!(visible_creative_entries(&ledger, &runtime).is_empty());
        runtime.screen_state_mut().search = "kristall".into();
        display_name(&mut runtime, "custom.blade.name");
        runtime.set_active_language(Some(language("custom.blade.name", "Kristallklinge")));
        assert_eq!(
            visible_creative_entries(&ledger, &runtime)
                .get(0)
                .unwrap()
                .stack
                .network_id,
            1
        );
    }
}
