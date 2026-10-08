//! Creative rows remain immutable until the catalog, selected groups or icons change.

use std::{
    collections::{BTreeMap, BTreeSet},
    sync::Arc,
};

use json_ui::{CollectionItem, DataSource};
use protocol::{CreativeContentEvent, PlayerGameMode};

use super::{COLLECTION, HudFrame, IconRef, UiRuntime};

pub(in super::super) struct BookCache {
    catalog: CreativeContentEvent,
    tab: u8,
    search: String,
    registry: Option<Arc<BTreeMap<i32, protocol::ItemRegistryEntry>>>,
    expanded: BTreeSet<u32>,
    source_icons: Vec<Option<IconRef>>,
    first_icon: usize,
    icons: Vec<IconRef>,
    items: Arc<[CollectionItem]>,
}

impl BookCache {
    fn eligible<'a>(
        player_runtime: &'a player_state::PlayerState,
        runtime: &UiRuntime,
    ) -> Option<&'a CreativeContentEvent> {
        (player_runtime.facts.player_game_mode() == Some(PlayerGameMode::Creative))
            .then(|| runtime.inventory_ledger(player_runtime).creative_catalog())
            .flatten()
    }

    /// Publish the same row allocation when all row inputs still match.
    pub(super) fn reuse(
        player_runtime: &player_state::PlayerState,
        cache: &Option<Self>,
        runtime: &UiRuntime,
        frame: &HudFrame,
        icons: &mut Vec<IconRef>,
        data: &mut DataSource,
    ) -> bool {
        let (Some(cache), Some(catalog)) = (cache, Self::eligible(player_runtime, runtime)) else {
            return false;
        };
        let state = runtime.screen_state();
        if !Arc::ptr_eq(&cache.catalog.items, &catalog.items)
            || !Arc::ptr_eq(&cache.catalog.groups, &catalog.groups)
            || cache.tab != state.creative_tab
            || cache.search != state.search
            || !same_registry(
                cache.registry.as_ref(),
                runtime
                    .inventory_ledger(player_runtime)
                    .item_registry_snapshot(),
            )
            || cache.expanded != state.creative_expanded
            || cache.source_icons != frame.window_icons.book_entries
            || cache.first_icon != icons.len()
        {
            return false;
        }
        icons.extend_from_slice(&cache.icons);
        data.set_shared_collection(COLLECTION, Arc::clone(&cache.items));
        true
    }

    /// Retain a creative publication and its icon-table positions.
    pub(super) fn capture(
        player_runtime: &player_state::PlayerState,
        runtime: &UiRuntime,
        frame: &HudFrame,
        first_icon: usize,
        icons: &[IconRef],
        items: &Arc<[CollectionItem]>,
    ) -> Option<Self> {
        let catalog = Self::eligible(player_runtime, runtime)?;
        let state = runtime.screen_state();
        Some(Self {
            catalog: catalog.clone(),
            tab: state.creative_tab,
            search: state.search.clone(),
            registry: runtime
                .inventory_ledger(player_runtime)
                .item_registry_snapshot()
                .cloned(),
            expanded: state.creative_expanded.clone(),
            source_icons: frame.window_icons.book_entries.clone(),
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

#[cfg(test)]
mod tests {
    use super::*;
    use crate::ui_runtime::presentation::screens::SEARCH_TAB;

    /// Scroll and pointer changes reuse rows; tab, group, catalog and icon changes invalidate them.
    #[test]
    fn creative_rows_follow_their_inputs() {
        let mut player_runtime = player_state::PlayerState::new(1);

        let mut runtime = UiRuntime::new(1);
        runtime.screen_state_mut().creative_tab = SEARCH_TAB;
        runtime.screen_state_mut().search = "stone".into();
        player_runtime
            .facts
            .publish_player_game_mode(PlayerGameMode::Creative);
        let catalog = CreativeContentEvent {
            groups: Arc::from([]),
            items: Arc::from([]),
            skipped: 0,
        };
        runtime
            .inventory_ledger_mut(&mut player_runtime)
            .apply(&protocol::InventoryEvent::Creative(catalog.clone()));
        let frame = HudFrame::default();
        let items = Arc::from([CollectionItem::default()]);
        let cache = BookCache::capture(&player_runtime, &runtime, &frame, 0, &[], &items);
        let reuse =
            |player_runtime: &player_state::PlayerState, runtime: &UiRuntime, frame: &HudFrame| {
                BookCache::reuse(
                    player_runtime,
                    &cache,
                    runtime,
                    frame,
                    &mut Vec::new(),
                    &mut DataSource::new(),
                )
            };
        runtime
            .screen_state_mut()
            .container_scroll
            .insert("grid".into(), 60.0);
        assert!(reuse(&player_runtime, &runtime, &frame));
        runtime.screen_state_mut().search.push('s');
        assert!(!reuse(&player_runtime, &runtime, &frame));
        runtime.screen_state_mut().search.pop();
        assert!(reuse(&player_runtime, &runtime, &frame));
        runtime.screen_state_mut().creative_expanded.insert(1);
        assert!(!reuse(&player_runtime, &runtime, &frame));
        runtime.screen_state_mut().creative_expanded.clear();
        runtime.screen_state_mut().creative_tab += 1;
        assert!(!reuse(&player_runtime, &runtime, &frame));
        runtime.screen_state_mut().creative_tab -= 1;
        let mut other_frame = frame.clone();
        other_frame.window_icons.book_entries.push(None);
        assert!(!reuse(&player_runtime, &runtime, &other_frame));
        runtime
            .inventory_ledger_mut(&mut player_runtime)
            .apply_registry(&protocol::ItemRegistryEvent {
                entries: Arc::from([]),
            });
        assert!(!reuse(&player_runtime, &runtime, &frame));
        runtime.inventory_ledger_mut(&mut player_runtime).apply(
            &protocol::InventoryEvent::Creative(CreativeContentEvent {
                items: Arc::from([protocol::CreativeItem {
                    creative_network_id: 1,
                    stack: protocol::NetworkItemStack::empty(),
                    group: 0,
                }]),
                ..catalog
            }),
        );
        assert!(!reuse(&player_runtime, &runtime, &frame));
    }
}
