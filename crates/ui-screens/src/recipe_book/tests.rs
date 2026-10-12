//! The recipe grid builds every cell whenever server ui files may place them.

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

/// Server ui files may draw recipe cells anywhere, so with any applied the grid builds them all.
#[test]
fn server_ui_builds_every_recipe_cell() {
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
    let built = |server_ui| {
        let bound =
            json_ui::bind_incremental(&root, &data, &RecipeCell, &mut screen_binding(server_ui));
        bound
            .children
            .iter()
            .filter(|cell| cell.children.iter().any(|child| child.name == "label"))
            .count()
    };
    assert!(built(false) < CELLS);
    assert!(built(true) >= CELLS);
}
