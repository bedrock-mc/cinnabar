//! Recipe publications must explicitly clear a retained cell's previous icon.

use std::sync::Arc;

use json_ui::{BindState, EmptyLibrary, ResolvedControl, bind_stateful};
use serde_json::{Value, json};

use {super::*, ui::IconRef};

#[test]
fn review_recipe_cell_clears_an_icon_when_replacement_has_no_art() {
    let mut player_runtime = player_state::PlayerState::new(1);

    let mut runtime = UiRuntime::new(1);
    player_runtime
        .facts
        .publish_player_game_mode(protocol::PlayerGameMode::Creative);
    runtime
        .inventory_ledger_mut(&mut player_runtime)
        .apply(&protocol::InventoryEvent::Creative(
            protocol::CreativeContentEvent {
                groups: vec![protocol::CreativeGroup {
                    category: protocol::CreativeCategory::Construction,
                    name: "".into(),
                    icon: None,
                }]
                .into(),
                items: vec![protocol::CreativeItem {
                    creative_network_id: 1,
                    group: 0,
                    stack: protocol::NetworkItemStack {
                        network_id: 1,
                        count: 1,
                        ..protocol::NetworkItemStack::empty()
                    },
                }]
                .into(),
                skipped: 0,
            },
        ));
    let root = Arc::new(ResolvedControl {
        name: "book".into(), control_type: Some("panel".into()), base: None, unresolved_base: None,
        properties: [("collection_name".into(), json!(COLLECTION))].into(), factory: None,
        children: vec![ResolvedControl {
            name: "cell".into(), control_type: Some("custom".into()), base: None, unresolved_base: None,
            properties: [("collection_index".into(), json!(0)), ("bindings".into(), json!([{
                "binding_type":"collection", "binding_collection_name":COLLECTION, "binding_name":"#item_renderer_data"
            }]))].into(), children: vec![], factory: None,
        }],
    });
    let mut state = BindState::new();
    let mut cache = None;
    for has_icon in [true, false, true] {
        let mut frame = HudFrame::default();
        frame.window_icons.book_entries = vec![has_icon.then_some(IconRef {
            page: 0,
            uv: [0, 0, 16, 16],
            glint: false,
        })];
        let mut data = DataSource::new();
        let mut icons = Vec::new();
        book_data(
            &player_runtime,
            &mut data,
            &runtime,
            &frame,
            &mut icons,
            true,
            &mut cache,
        );
        let (bound, notes) = bind_stateful(&root, &data, &EmptyLibrary, &mut state);
        assert!(notes.is_empty(), "{notes:?}");
        assert_eq!(
            bound.children[0].properties.get("#item_renderer_data"),
            Some(&if has_icon { json!(0.0) } else { Value::Null })
        );
        assert_eq!(icons.len(), usize::from(has_icon));
    }
}

#[test]
fn an_empty_recipe_tab_clears_the_retained_grid_size() {
    let mut player = player_state::PlayerState::new(1);
    let mut runtime = UiRuntime::new(1);
    player
        .facts
        .publish_player_game_mode(protocol::PlayerGameMode::Creative);
    runtime
        .inventory_ledger_mut(&mut player)
        .apply(&protocol::InventoryEvent::Creative(
            protocol::CreativeContentEvent {
                groups: Arc::from([protocol::CreativeGroup {
                    category: protocol::CreativeCategory::Construction,
                    name: "".into(),
                    icon: None,
                }]),
                items: Arc::from([protocol::CreativeItem {
                    creative_network_id: 1,
                    group: 0,
                    stack: protocol::NetworkItemStack {
                        network_id: 1,
                        count: 1,
                        ..protocol::NetworkItemStack::empty()
                    },
                }]),
                skipped: 0,
            },
        ));
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
                "binding_type": "collection", "binding_collection_name": COLLECTION,
                "binding_name": "#recipe_book_total_items",
                "binding_name_override": "#maximum_grid_items", "binding_condition": "visible"
            }]),
        )]
        .into(),
    });
    let mut state = BindState::new();
    let mut cache = None;
    for (tab, expected) in [(0, 1), (1, 0), (0, 1), (1, 0)] {
        runtime.screen_state_mut().creative_tab = tab;
        let mut data = DataSource::new();
        book_data(
            &player,
            &mut data,
            &runtime,
            &HudFrame::default(),
            &mut Vec::new(),
            true,
            &mut cache,
        );
        let (bound, notes) = bind_stateful(&root, &data, &EmptyLibrary, &mut state);
        assert!(notes.is_empty(), "{notes:?}");
        assert_eq!(bound.properties["maximum_grid_items"], json!(expected));
    }
}
