//! Changed recipe templates keep their complete grid; proven base templates virtualize.

use std::sync::Arc;

use json_ui::ResolvedControl;
use serde_json::json;

use super::*;

/// Hands out a 10×10 recipe cell holding one label for any grid template.
struct RecipeCell;

impl json_ui::ControlLibrary for RecipeCell {
    fn resolve(&self, _: &json_ui::ControlRef) -> Option<ResolvedControl> {
        let label = ResolvedControl {
            name: "label".into(),
            control_type: Some("label".into()),
            base: None,
            unresolved_base: None,
            properties: [("text".into(), json!("x"))].into(),
            children: vec![],
            factory: None,
        };
        Some(ResolvedControl {
            name: "cell".into(),
            control_type: Some("panel".into()),
            base: None,
            unresolved_base: None,
            properties: [("size".into(), json!([10, 10]))].into(),
            children: vec![label],
            factory: None,
        })
    }
}

/// Modified templates may draw cells anywhere and must retain the complete grid.
#[test]
fn unverified_templates_build_every_recipe_cell() {
    const CELLS: usize = 200;
    let root = Arc::new(ResolvedControl {
        name: "grid".into(),
        control_type: Some("grid".into()),
        base: None,
        unresolved_base: None,
        properties: [
            ("collection_name".into(), json!(COLLECTION)),
            ("grid_dimensions".into(), json!([1, CELLS])),
            ("grid_item_template".into(), json!("test.cell")),
        ]
        .into(),
        children: vec![],
        factory: None,
    });
    let mut data = DataSource::new();
    data.set_collection(COLLECTION, vec![json_ui::CollectionItem::default(); CELLS]);
    let data = Arc::new(data);
    let built = |virtualize| {
        let bound =
            json_ui::bind_incremental(&root, &data, &RecipeCell, &mut screen_binding(virtualize));
        bound
            .children
            .iter()
            .filter(|cell| cell.children.iter().any(|child| child.name == "label"))
            .count()
    };
    assert!(built(true) < CELLS);
    assert!(built(false) >= CELLS);
}

#[test]
fn inventory_options_wire_tabs_map_to_ui_categories_and_navigation_controls() {
    use crate::container_actions::{SURVIVAL_TAB, inventory_tab, inventory_tab_ui};
    for (wire, ui) in [
        (1, 0),
        (2, 2),
        (3, 3),
        (4, 1),
        (5, SEARCH_TAB),
        (6, SURVIVAL_TAB),
    ] {
        let tab = InventoryTab::from_index(wire);
        assert_eq!(inventory_tab(ui), Some(tab));
        assert_eq!(inventory_tab_ui(tab), Some(ui));
        let expected = if ui == SEARCH_TAB {
            InventoryCellHit::CreativeSearch
        } else {
            InventoryCellHit::CreativeTab(ui)
        };
        assert_eq!(
            navigation_hit("navigation_tab", wire as u64),
            Some(expected)
        );
    }
    for tab in [
        InventoryTab::None,
        InventoryTab::Unknown(-1),
        InventoryTab::Unknown(82),
    ] {
        assert_eq!(inventory_tab_ui(tab), None);
    }
    assert_eq!(navigation_hit("navigation_tab", 99), None);
    for tab in [
        InventoryRightTab::Fullscreen,
        InventoryRightTab::Crafting,
        InventoryRightTab::Armor,
    ] {
        assert_eq!(
            navigation_hit("right_inventory_navigation_tab", tab.index() as u64),
            Some(InventoryCellHit::Widget(Widget::InventoryRightTab(
                tab.index() as u8
            )))
        );
    }
}

/// Reads a production global through the same binding evaluator used by screen templates.
fn boolean(data: &DataSource, name: &str) -> bool {
    let root = ResolvedControl {
        name: "binding_probe".into(),
        control_type: Some("panel".into()),
        base: None,
        unresolved_base: None,
        properties: [(
            "bindings".into(),
            json!([{"binding_name": name, "binding_name_override": "#visible"}]),
        )]
        .into(),
        children: vec![],
        factory: None,
    };
    json_ui::bind(&root, data, &RecipeCell)
        .properties
        .get("visible")
        .and_then(Value::as_bool)
        .unwrap()
}

#[test]
fn inventory_options_bind_tab_identity_raw_filter_and_context_layout_availability() {
    use inventory::{InventoryContext, InventoryController};
    let mut controller =
        InventoryController::new(Default::default(), InventoryContext::Personal, true);
    controller.options.left_tab = InventoryTab::Survival;
    controller.options.right_tab = InventoryRightTab::Armor;
    controller.options.filtering = true;
    let mut data = DataSource::new();
    option_bindings(&mut data, controller, true, true);
    assert!(boolean(&data, "#is_left_tab_inventory"));
    assert!(boolean(&data, "#is_right_tab_armor"));
    assert!(!boolean(&data, "#is_right_tab_crafting"));
    assert!(boolean(&data, "#is_creative_layout"));
    assert!(boolean(&data, "#is_creative_layout_button_visible"));
    assert!(boolean(&data, "#filtering_enabled"));
    controller.options.left_tab = InventoryTab::Search;
    controller.context = InventoryContext::Workbench;
    controller.set_layout(InventoryLayout::InventoryOnly);
    option_bindings(&mut data, controller, true, false);
    assert!(!boolean(&data, "#is_left_tab_inventory"));
    assert!(!boolean(&data, "#is_creative_layout_button_visible"));
    assert!(boolean(&data, "#is_survival_layout"));
}

#[test]
fn hidden_recipe_book_uses_survival_binding_and_retains_unknown_layouts() {
    use inventory::{InventoryContext, InventoryController};
    for context in [InventoryContext::Personal, InventoryContext::Workbench] {
        for creative in [false, true] {
            let options = protocol::InventoryOptions {
                inventory_layout: InventoryLayout::Unknown(-1),
                crafting_layout: InventoryLayout::Unknown(i32::MIN),
                ..Default::default()
            };
            let controller = InventoryController::new(options, context, creative);
            let mut data = DataSource::new();
            option_bindings(&mut data, controller, creative, false);
            assert!(boolean(&data, "#is_survival_layout"));
            assert!(!boolean(&data, "#is_recipe_book_layout"));
            assert!(!boolean(&data, "#is_creative_layout"));
            assert_eq!(controller.options, options);
        }
    }
}
