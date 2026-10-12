//! The inventory screen's data keeps the recipe book's rows stable when an item moves.

use std::sync::Arc;

use json_ui::{BindState, EmptyLibrary, ResolvedControl, bind_stateful};
use serde_json::{Value, json};

use protocol::NetworkItemStack;
use ui::IconRef;
use ui_screens::containers::{ScreenLayout, screen_data};

use super::{ContainerAuthority, HudFrame, UiRuntime, container_frame};

const BOOK: &str = "recipe_book";

fn control(name: &str, properties: Value, children: Vec<ResolvedControl>) -> ResolvedControl {
    let Value::Object(properties) = properties else {
        unreachable!("control properties are an object");
    };
    ResolvedControl {
        name: name.into(),
        control_type: Some("panel".into()),
        base: None,
        unresolved_base: None,
        properties: properties.into_iter().collect(),
        children,
        factory: None,
    }
}

/// A screen reading the inventory tab global and the first recipe book cell's icon.
fn screen_root() -> Arc<ResolvedControl> {
    let cell = control(
        "cell",
        json!({
            "collection_index": 0,
            "bindings": [{
                "binding_type": "collection",
                "binding_collection_name": BOOK,
                "binding_name": "#item_renderer_data"
            }]
        }),
        Vec::new(),
    );
    let book = control("book", json!({ "collection_name": BOOK }), vec![cell]);
    Arc::new(control(
        "screen",
        json!({ "bindings": [{ "binding_name": "#is_left_tab_inventory" }] }),
        vec![book],
    ))
}

fn stack(network_id: i32) -> NetworkItemStack {
    NetworkItemStack {
        network_id,
        count: 1,
        ..NetworkItemStack::empty()
    }
}

#[test]
fn moving_an_inventory_item_keeps_the_recipe_book_icons_and_layout() {
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
                    stack: stack(1),
                }]),
                skipped: 0,
            },
        ));
    let icon = IconRef {
        page: 0,
        uv: [0, 0, 16, 16],
        glint: false,
    };
    let mut frame = HudFrame::default();
    frame.window_icons.book_entries = vec![Some(icon)];
    // Slot 9 is the first `inventory_items` row.
    frame.inventory_icons.0[9] = Some(icon);
    let root = screen_root();
    let mut state = BindState::new();
    let mut caches = (None, None);
    let mut table_len = None;
    for occupied in [false, true, false] {
        let mut slots = vec![NetworkItemStack::empty(); 36];
        if occupied {
            slots[9] = stack(2);
        }
        runtime
            .inventory_ledger_mut(&mut player)
            .apply(&protocol::InventoryEvent::Content(
                protocol::InventoryContentEvent {
                    container: protocol::ContainerIdentity {
                        window_id: Some(0),
                        slot_type: None,
                        dynamic_id: None,
                    },
                    slots: slots.into(),
                    storage_item: NetworkItemStack::empty(),
                },
            ));
        let mut icons = Vec::new();
        let data = screen_data(
            &ContainerAuthority::new(&player, &runtime),
            &container_frame(&frame),
            ScreenLayout::Personal { book: true },
            "",
            &mut icons,
            (&mut caches.0, &mut caches.1),
        );
        assert_eq!(icons[0], icon);
        assert_eq!(
            *table_len.get_or_insert(icons.len()),
            icons.len(),
            "cells reserve their icon entries whether or not they hold an item"
        );
        let (bound, notes) = bind_stateful(&root, &data, &EmptyLibrary, &mut state);
        assert!(notes.is_empty(), "{notes:?}");
        assert_eq!(
            bound.properties.get("#is_left_tab_inventory"),
            Some(&json!(false)),
            "the shown book replaces the inventory tab"
        );
        assert_eq!(
            bound.children[0].children[0]
                .properties
                .get("#item_renderer_data"),
            Some(&json!(0.0)),
            "the book's icon index does not depend on the inventory"
        );
    }
}
