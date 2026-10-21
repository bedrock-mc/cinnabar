use std::sync::Arc;

use json_ui::{BindState, DataSource, EmptyLibrary, ResolvedControl, Scalar, bind_stateful};
use serde_json::json;
use ui_screens::furnace_book::{data, publication};

use super::super::containers::{ContainerAuthority, container_frame};
use super::*;
use crate::ui_runtime::presentation::HudFrame;

pub(in crate::ui_runtime::presentation) fn assert_selection_publication(
    player: &mut player_state::PlayerState,
    runtime: &mut UiRuntime,
) {
    let mut cache = None;
    let frame = HudFrame::default();
    let frame = container_frame(&frame);
    player.inventory.ledger_mut().clear_furnace_recipe();
    let recipe = entries(player, runtime).get(0).unwrap().clone();
    let rows = |player: &player_state::PlayerState, runtime: &UiRuntime, cache: &mut _| {
        Arc::clone(publication(&ContainerAuthority::new(player, runtime), &frame, 0, cache).rows())
    };
    let first = rows(player, runtime, &mut cache);
    player
        .inventory
        .ledger_mut()
        .begin_furnace_recipe(&recipe)
        .unwrap();
    let selected = rows(player, runtime, &mut cache);
    assert_eq!(
        selected[0].values["#is_recipe_selected_slot"],
        Scalar::Bool(true)
    );
    assert!(!Arc::ptr_eq(&first, &selected));
    assert!(Arc::ptr_eq(&selected, &rows(player, runtime, &mut cache)));
    player
        .inventory
        .ledger_mut()
        .begin_furnace_recipe(&recipe)
        .unwrap();
    let cleared = rows(player, runtime, &mut cache);
    assert_eq!(
        cleared[0].values["#is_recipe_selected_slot"],
        Scalar::Bool(false)
    );
    assert!(!Arc::ptr_eq(&selected, &cleared));
}

pub(in crate::ui_runtime::presentation) fn assert_empty_search_clears_grid(
    player: &player_state::PlayerState,
    runtime: &mut UiRuntime,
) {
    let root = Arc::new(ResolvedControl {
        name: "grid".into(),
        control_type: Some("grid".into()),
        base: None,
        unresolved_base: None,
        factory: None,
        children: vec![],
        properties: [(
            "bindings".into(),
            json!([{
                "binding_type": "collection", "binding_collection_name": "recipe_book",
                "binding_name": "#recipe_book_total_items",
                "binding_name_override": "#maximum_grid_items", "binding_condition": "visible"
            }]),
        )]
        .into(),
    });
    let mut state = BindState::new();
    let mut cache = None;
    for (query, expected) in [("iron", 1), ("zzzz", 0), ("iron", 1), ("zzzz", 0)] {
        runtime.screen_state_mut().search = query.into();
        let mut published = DataSource::new();
        data(
            &ContainerAuthority::new(player, runtime),
            &container_frame(&HudFrame::default()),
            &mut published,
            &mut Vec::new(),
            &mut cache,
        );
        let (bound, notes) = bind_stateful(&root, &published, &EmptyLibrary, &mut state);
        assert!(notes.is_empty(), "{notes:?}");
        assert_eq!(
            bound.properties["maximum_grid_items"],
            json!(expected),
            "empty furnace search must clear the retained recipe grid"
        );
    }
}

#[test]
fn furnace_recipe_rows_reuse_scroll_and_hover_frames() {
    let mut player = player_state::PlayerState::new(1);
    let mut runtime = crate::test_support::inventory_session(&mut player);
    player
        .inventory
        .ledger_mut()
        .apply(&protocol::InventoryEvent::Open(
            protocol::ContainerOpenEvent {
                container: protocol::ContainerIdentity::window(7),
                window_type: protocol::WINDOW_TYPE_FURNACE,
                position: [0, 64, 0],
                runtime_entity_id: -1,
            },
        ));
    runtime.observe_inventory_window(&player);
    let frame = HudFrame::default();
    let frame = container_frame(&frame);
    let mut cache = None;
    let rows = |runtime: &UiRuntime, cache: &mut _| {
        Arc::clone(publication(&ContainerAuthority::new(&player, runtime), &frame, 0, cache).rows())
    };
    let first = rows(&runtime, &mut cache);
    runtime
        .screen_state_mut()
        .container_scroll
        .insert("recipes".into(), 24.0);
    runtime.screen_state_mut().search_focused = true;
    assert!(Arc::ptr_eq(&first, &rows(&runtime, &mut cache)));
    runtime.screen_state_mut().search = "iron".into();
    let searched = rows(&runtime, &mut cache);
    assert!(!Arc::ptr_eq(&first, &searched));
    assert!(Arc::ptr_eq(&searched, &rows(&runtime, &mut cache)));
}

#[test]
fn furnace_inventory_tab_binding_is_independent_of_the_selected_layout() {
    for layout in [
        protocol::FurnaceLayout::InventoryOnly,
        protocol::FurnaceLayout::RecipeBook,
    ] {
        for left_tab in [
            protocol::FurnaceTab::Inventory,
            protocol::FurnaceTab::Search,
        ] {
            let mut player = player_state::PlayerState::new(1);
            let mut runtime = crate::test_support::inventory_session(&mut player);
            player.inventory.set_furnace_options(
                protocol::FurnaceKind::Furnace,
                protocol::FurnaceOptions {
                    left_tab,
                    filtering: false,
                    layout,
                },
            );
            player
                .inventory
                .ledger_mut()
                .apply(&protocol::InventoryEvent::Open(
                    protocol::ContainerOpenEvent {
                        container: protocol::ContainerIdentity::window(7),
                        window_type: protocol::WINDOW_TYPE_FURNACE,
                        position: [0, 64, 0],
                        runtime_entity_id: -1,
                    },
                ));
            runtime.observe_inventory_window(&player);
            let mut published = DataSource::new();
            data(
                &ContainerAuthority::new(&player, &runtime),
                &container_frame(&HudFrame::default()),
                &mut published,
                &mut Vec::new(),
                &mut None,
            );
            let root = Arc::new(ResolvedControl {
                name: "selected_inventory".into(), control_type: Some("panel".into()),
                base: None, unresolved_base: None, factory: None, children: vec![],
                properties: [("bindings".into(), json!([{
                    "binding_name": "#is_left_tab_inventory", "binding_name_override": "#visible"
                }]))].into(),
            });
            let (bound, notes) =
                bind_stateful(&root, &published, &EmptyLibrary, &mut BindState::new());
            assert!(notes.is_empty(), "{notes:?}");
            assert_eq!(
                bound.properties["visible"],
                json!(left_tab == protocol::FurnaceTab::Inventory)
            );
        }
    }
}
