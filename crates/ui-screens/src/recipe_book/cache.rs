//! Creative rows remain immutable until the catalog, names, selected groups or icons change.
//! Other books reuse their rows while every row's shown inputs stay the same.

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

/// The rows last published, with every input they were built from.
pub struct BookCache {
    inputs: Inputs,
    first_icon: usize,
    icons: Vec<IconRef>,
    items: Arc<[CollectionItem]>,
}

enum Inputs {
    Creative(Box<CreativeInputs>),
    /// Any other book: whether counts are hidden, and what each row shows.
    Rows {
        creative: bool,
        rows: Vec<RowKey>,
    },
}

/// What one row shows besides its position.
#[derive(Clone, Copy, PartialEq)]
pub(super) struct RowKey {
    pub(super) count: u16,
    pub(super) id_aux: i32,
    pub(super) background: &'static str,
    pub(super) modifier: i64,
    pub(super) icon: Option<IconRef>,
}

struct CreativeInputs {
    catalog: CreativeContentEvent,
    tab: protocol::InventoryTab,
    search: String,
    registry: Option<Arc<BTreeMap<i32, protocol::ItemRegistryEntry>>>,
    name_generation: u64,
    expanded: BTreeSet<u32>,
    source_icons: Vec<Option<IconRef>>,
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
        let Inputs::Creative(cached) = &cache.inputs else {
            return false;
        };
        if cache.first_icon != icons.len() || !cached.matches(source, catalog, frame) {
            return false;
        }
        icons.extend_from_slice(&cache.icons);
        data.set_shared_collection(COLLECTION, Arc::clone(&cache.items));
        true
    }

    /// Publishes the same rows when every row shows what the cached one showed.
    pub(super) fn reuse_rows(
        cache: &Option<Self>,
        creative: bool,
        rows: impl ExactSizeIterator<Item = RowKey>,
        icons: &mut Vec<IconRef>,
        data: &mut DataSource,
    ) -> bool {
        let Some(cache) = cache else {
            return false;
        };
        let Inputs::Rows {
            creative: cached_creative,
            rows: cached,
        } = &cache.inputs
        else {
            return false;
        };
        if *cached_creative != creative
            || cache.first_icon != icons.len()
            || cached.len() != rows.len()
            || !rows.zip(cached).all(|(row, cached)| row == *cached)
        {
            return false;
        }
        icons.extend_from_slice(&cache.icons);
        data.set_shared_collection(COLLECTION, Arc::clone(&cache.items));
        true
    }

    /// Retains published rows by what each one shows.
    pub(super) fn capture_rows(
        creative: bool,
        rows: Vec<RowKey>,
        first_icon: usize,
        icons: &[IconRef],
        items: &Arc<[CollectionItem]>,
    ) -> Self {
        Self {
            inputs: Inputs::Rows { creative, rows },
            first_icon,
            icons: icons[first_icon..].to_vec(),
            items: Arc::clone(items),
        }
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
            inputs: Inputs::Creative(Box::new(CreativeInputs {
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
            })),
            first_icon,
            icons: icons[first_icon..].to_vec(),
            items: Arc::clone(items),
        })
    }
}

impl CreativeInputs {
    fn matches(
        &self,
        source: &dyn ContainerSource,
        catalog: &CreativeContentEvent,
        frame: &ContainerFrame<'_>,
    ) -> bool {
        let state = source.screen();
        Arc::ptr_eq(&self.catalog.items, &catalog.items)
            && Arc::ptr_eq(&self.catalog.groups, &catalog.groups)
            && self.tab == state.inventory.options.left_tab
            && self.search == state.search
            && same_registry(
                self.registry.as_ref(),
                source.player().inventory.ledger().item_registry_snapshot(),
            )
            && self.name_generation == source.item_name_generation()
            && self.expanded == *state.creative_expanded
            && self.source_icons[..] == *frame.book_entries
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

    /// The published rows, to check that a reused publication shares them.
    pub fn rows(&self) -> &Arc<[CollectionItem]> {
        &self.items
    }
}
