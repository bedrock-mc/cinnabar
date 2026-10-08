use std::sync::Arc;

use json_ui::{BindState, EmptyLibrary, ResolvedControl, bind_stateful};
use serde_json::json;

use super::*;

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
            player,
            runtime,
            &HudFrame::default(),
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
