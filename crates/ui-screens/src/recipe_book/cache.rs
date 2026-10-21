//! Creative rows remain immutable until the catalog, names, selected groups or icons change.

use std::{
    collections::{BTreeMap, BTreeSet},
    sync::Arc,
};

use json_ui::{CollectionItem, DataSource};
use protocol::{CreativeContentEvent, PlayerGameMode};

use {
    super::COLLECTION,
    crate::container_source::{ContainerFrame, ContainerSource},
    ui::IconRef,
};

/// The creative rows last published, with every input they were built from.
pub struct BookCache {
    catalog: CreativeContentEvent,
    tab: protocol::InventoryTab,
    search: String,
    registry: Option<Arc<BTreeMap<i32, protocol::ItemRegistryEntry>>>,
    name_generation: u64,
    expanded: BTreeSet<u32>,
    source_icons: Vec<Option<IconRef>>,
    first_icon: usize,
    icons: Vec<IconRef>,
    items: Arc<[CollectionItem]>,
}

impl BookCache {
    fn eligible(source: &dyn ContainerSource) -> Option<&CreativeContentEvent> {
        let player_runtime = source.player();
        (player_runtime.facts.player_game_mode() == Some(PlayerGameMode::Creative))
            .then(|| player_runtime.inventory.ledger().creative_catalog())
            .flatten()
    }

    /// Publish the same row allocation when all row inputs still match.
    pub fn reuse(
        cache: &Option<Self>,
        source: &dyn ContainerSource,
        frame: &ContainerFrame<'_>,
        icons: &mut Vec<IconRef>,
        data: &mut DataSource,
    ) -> bool {
        let (Some(cache), Some(catalog)) = (cache, Self::eligible(source)) else {
            return false;
        };
        let state = source.screen();
        if !Arc::ptr_eq(&cache.catalog.items, &catalog.items)
            || !Arc::ptr_eq(&cache.catalog.groups, &catalog.groups)
            || cache.tab != state.inventory.options.left_tab
            || cache.search != state.search
            || !same_registry(
                cache.registry.as_ref(),
                source.player().inventory.ledger().item_registry_snapshot(),
            )
            || cache.name_generation != source.item_name_generation()
            || cache.expanded != *state.creative_expanded
            || cache.source_icons[..] != *frame.book_entries
            || cache.first_icon != icons.len()
        {
            return false;
        }
        icons.extend_from_slice(&cache.icons);
        data.set_shared_collection(COLLECTION, Arc::clone(&cache.items));
        true
    }

    /// Retain a creative publication and its icon-table positions.
    pub fn capture(
        source: &dyn ContainerSource,
        frame: &ContainerFrame<'_>,
        first_icon: usize,
        icons: &[IconRef],
        items: &Arc<[CollectionItem]>,
    ) -> Option<Self> {
        let catalog = Self::eligible(source)?;
        let state = source.screen();
        Some(Self {
            catalog: catalog.clone(),
            tab: state.inventory.options.left_tab,
            search: state.search.to_owned(),
            registry: source
                .player()
                .inventory
                .ledger()
                .item_registry_snapshot()
                .cloned(),
            name_generation: source.item_name_generation(),
            expanded: state.creative_expanded.clone(),
            source_icons: frame.book_entries.to_vec(),
            first_icon,
            icons: icons[first_icon..].to_vec(),
            items: Arc::clone(items),
        })
    }
}

/// Compares registry identity without scanning its item definitions.
fn same_registry(
    previous: Option<&Arc<BTreeMap<i32, protocol::ItemRegistryEntry>>>,
    current: Option<&Arc<BTreeMap<i32, protocol::ItemRegistryEntry>>>,
) -> bool {
    match (previous, current) {
        (Some(previous), Some(current)) => Arc::ptr_eq(previous, current),
        (None, None) => true,
        _ => false,
    }
}

#[cfg(any(test, feature = "test-support"))]
impl BookCache {
    /// Rows the cache retains.
    pub fn row_count(&self) -> usize {
        self.items.len()
    }
}
