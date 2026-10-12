//! Retained creative rows follow every input they were built from, and only those.

use std::sync::Arc;

use json_ui::{CollectionItem, DataSource};
use protocol::{CreativeContentEvent, PlayerGameMode};
use ui_screens::container_actions::SEARCH_TAB;
use ui_screens::recipe_book::{BookCache, book_data};

use super::super::containers::{ContainerAuthority, container_frame};
use crate::ui_runtime::UiRuntime;
use crate::ui_runtime::presentation::HudFrame;

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
    let cache = BookCache::capture(
        &ContainerAuthority::new(&player_runtime, &runtime),
        &container_frame(&frame),
        0,
        &[],
        &items,
    );
    let reuse =
        |player_runtime: &player_state::PlayerState, runtime: &UiRuntime, frame: &HudFrame| {
            BookCache::reuse(
                &cache,
                &ContainerAuthority::new(player_runtime, runtime),
                &container_frame(frame),
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
    runtime
        .inventory_ledger_mut(&mut player_runtime)
        .apply(&protocol::InventoryEvent::Creative(CreativeContentEvent {
            items: Arc::from([protocol::CreativeItem {
                creative_network_id: 1,
                stack: protocol::NetworkItemStack::empty(),
                group: 0,
            }]),
            ..catalog
        }));
    assert!(!reuse(&player_runtime, &runtime, &frame));
}

/// A name replacement refreshes published rows even when the query and icons stay unchanged.
#[test]
fn display_name_replacement_refreshes_retained_creative_rows() {
    use crate::ui_runtime::item_facts::SessionItemComponents;
    let mut player = player_state::PlayerState::new(1);
    let mut runtime = crate::test_support::creative_with(&mut player, 1);
    runtime
        .inventory_ledger_mut(&mut player)
        .apply_registry(&protocol::ItemRegistryEvent {
            entries: Arc::from([protocol::ItemRegistryEntry {
                components: None,
                network_id: 1,
                identifier: "custom:blade".into(),
                component_based: true,
                version: protocol::ItemRegistryVersion::None,
                component_digest: [0; 32],
                negotiated_max_stack_size: None,
                canonical_empty_component_data: true,
                item_tags: Arc::from([]),
            }]),
        });
    runtime.set_session_items(Some(Arc::new(SessionItemComponents::from_iter([(
        Arc::from("custom:blade"),
        protocol::ItemComponents {
            display_name: Some(Arc::from("Crystal Blade")),
            ..Default::default()
        },
    )]))));
    runtime.screen_state_mut().creative_tab = SEARCH_TAB;
    runtime.screen_state_mut().search = "Crystal".into();
    let mut cache = None;
    let frame = HudFrame::default();
    for expected in [1, 0] {
        let mut data = DataSource::new();
        book_data(
            &ContainerAuthority::new(&player, &runtime),
            &mut data,
            &container_frame(&frame),
            &mut Vec::new(),
            true,
            &mut cache,
        );
        assert_eq!(
            cache.as_ref().unwrap().row_count(),
            expected,
            "published rows must follow the current displayed name"
        );
        runtime.set_session_items(None);
    }
}
