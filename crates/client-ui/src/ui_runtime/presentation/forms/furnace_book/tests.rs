use std::sync::Arc;

use json_ui::{BindState, EmptyLibrary, ResolvedControl, bind_stateful};
use serde_json::json;

use super::*;

pub(in crate::ui_runtime::presentation) fn assert_selection_publication(
    player: &mut player_state::PlayerState,
    runtime: &mut UiRuntime,
) {
    let mut cache = None;
    let frame = HudFrame::default();
    player.inventory.ledger_mut().clear_furnace_recipe();
    let recipe = entries(player, runtime)[0].clone();
    let first = Arc::clone(&cache::publication(player, runtime, &frame, 0, &mut cache).rows);
    player.inventory.ledger_mut().begin_furnace_recipe(&recipe).unwrap();
    let selected = Arc::clone(&cache::publication(player, runtime, &frame, 0, &mut cache).rows);
    assert_eq!(selected[0].values["#is_recipe_selected_slot"], Scalar::Bool(true));
    assert!(!Arc::ptr_eq(&first, &selected));
    assert!(Arc::ptr_eq(&selected, &cache::publication(player, runtime, &frame, 0, &mut cache).rows));
    player.inventory.ledger_mut().begin_furnace_recipe(&recipe).unwrap();
    let cleared = Arc::clone(&cache::publication(player, runtime, &frame, 0, &mut cache).rows);
    assert_eq!(cleared[0].values["#is_recipe_selected_slot"], Scalar::Bool(false));
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
