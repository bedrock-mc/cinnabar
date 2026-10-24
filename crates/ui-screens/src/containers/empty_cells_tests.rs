use std::sync::Arc;

use json_ui::{BindState, DataSource, EmptyLibrary, ResolvedControl, bind_stateful};
use serde_json::{Value, json};

use {
    super::{Cells, NetworkItemStack},
    ui::IconRef,
};

#[test]
fn same_item_menu_cells_keep_distinct_custom_names_including_empty_names() {
    let names = [((1, 0), Arc::from("Diamond"))].into();
    let mut icons = Vec::new();
    let mut cells = Cells {
        names: &names,
        facts: &inventory::SessionItemFacts::default(),
        icons: &mut icons,
    };
    for name in [Some("Market"), Some("Next page"), Some(""), None] {
        let extra_data = protocol::edit_item_extra(
            &[],
            &protocol::ItemNbtEdit {
                custom_name: name.map(|name| Some(name.to_owned())),
                ..Default::default()
            },
        )
        .unwrap();
        let stack = NetworkItemStack {
            network_id: 1,
            count: 1,
            extra_data,
            ..NetworkItemStack::empty()
        };
        let cell = cells.cell(Some(&stack), None, None);
        assert_eq!(
            cell.values["#hover_text"],
            json_ui::Scalar::Text(name.unwrap_or("Diamond").to_owned()),
        );
    }
}

fn cell_control() -> Arc<ResolvedControl> {
    Arc::new(ResolvedControl {
        name: "inventory".into(),
        control_type: Some("panel".into()),
        base: None,
        unresolved_base: None,
        properties: [("collection_name".into(), json!("inventory_items"))].into(),
        children: vec![ResolvedControl {
            name: "item".into(),
            control_type: Some("custom".into()),
            base: None,
            unresolved_base: None,
            properties: [
                ("collection_index".into(), json!(0)),
                ("renderer".into(), json!("inventory_item_renderer")),
                (
                    "bindings".into(),
                    json!([{
                        "binding_type": "collection",
                        "binding_collection_name": "inventory_items",
                        "binding_name": "#item_renderer_data"
                    }]),
                ),
            ]
            .into(),
            children: Vec::new(),
            factory: None,
        }],
        factory: None,
    })
}

#[test]
fn retained_inventory_cell_answers_empty_after_occupied_and_can_be_refilled() {
    let root = cell_control();
    let mut state = BindState::new();
    let names = std::collections::HashMap::new();
    let mut stack = NetworkItemStack::empty();
    stack.network_id = 1;
    stack.stack_network_id = 1;
    stack.count = 1;
    let icon = IconRef {
        color_mask: None,
        page: 0,
        uv: [0, 0, 16, 16],
        glint: false,
    };
    for (occupied, expected) in [
        (true, json!(0.0)),
        (false, Value::Null),
        (false, Value::Null),
        (true, json!(0.0)),
    ] {
        let mut icons = Vec::new();
        let mut cells = Cells {
            names: &names,
            facts: &inventory::SessionItemFacts::default(),
            icons: &mut icons,
        };
        let cell = cells.cell(occupied.then_some(&stack), occupied.then_some(icon), None);
        let mut data = DataSource::new();
        data.set_collection("inventory_items", vec![cell]);
        let (bound, diagnostics) = bind_stateful(&root, &data, &EmptyLibrary, &mut state);
        assert!(diagnostics.is_empty(), "{diagnostics:?}");
        assert_eq!(
            bound.children[0].properties["#item_renderer_data"],
            expected
        );
        assert_eq!(icons.len(), 1, "empty cells reserve their stable entry");
        if occupied {
            assert_eq!(icons[0], icon);
        }
    }
}

#[test]
fn emptying_a_cell_keeps_other_cells_icon_bindings() {
    let names = std::collections::HashMap::new();
    let mut stack = NetworkItemStack::empty();
    stack.network_id = 1;
    stack.count = 1;
    let icon = IconRef {
        color_mask: None,
        page: 0,
        uv: [0, 0, 16, 16],
        glint: false,
    };
    let mut reference = None;
    for occupied in [true, false] {
        let mut icons = Vec::new();
        let mut cells = Cells {
            names: &names,
            facts: &inventory::SessionItemFacts::default(),
            icons: &mut icons,
        };
        cells.cell(occupied.then_some(&stack), occupied.then_some(icon), None);
        let other = cells.cell(Some(&stack), Some(icon), None);
        let binding = &other.values["#item_renderer_data"];
        if let Some(reference) = &reference {
            assert_eq!(
                binding, reference,
                "moving one item must not change another cell's binding"
            );
        } else {
            reference = Some(binding.clone());
        }
    }
}

#[test]
fn occupied_cell_without_art_explicitly_clears_the_old_icon() {
    let names = std::collections::HashMap::new();
    let mut stack = NetworkItemStack::empty();
    stack.network_id = 1;
    stack.count = 1;
    let mut icons = Vec::new();
    let mut cells = Cells {
        names: &names,
        facts: &inventory::SessionItemFacts::default(),
        icons: &mut icons,
    };
    let cell = cells.cell(Some(&stack), None, None);
    assert_eq!(
        cell.values["#item_renderer_data"],
        json_ui::Scalar::Json(Value::Null)
    );
    assert_eq!(icons.len(), 1);
}
