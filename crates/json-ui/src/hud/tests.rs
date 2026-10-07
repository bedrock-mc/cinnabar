use std::sync::Arc;

use serde_json::{Value, json};

use super::{BossBar, HudModel, HudSlot, hud_data_source};
use crate::{BindState, EmptyLibrary, ResolvedControl, bind_stateful};

fn hotbar() -> Arc<ResolvedControl> {
    let child = |index| ResolvedControl {
        name: format!("slot{index}"),
        control_type: Some("custom".into()),
        base: None,
        unresolved_base: None,
        properties: [
            ("collection_index".into(), json!(index)),
            ("renderer".into(), json!("inventory_item_renderer")),
            (
                "bindings".into(),
                json!([{
                    "binding_type": "collection",
                    "binding_collection_name": "hotbar_items",
                    "binding_name": "#item_renderer_data"
                }]),
            ),
        ]
        .into(),
        children: Vec::new(),
        factory: None,
    };
    Arc::new(ResolvedControl {
        name: "hotbar".into(),
        control_type: Some("panel".into()),
        base: None,
        unresolved_base: None,
        properties: [("collection_name".into(), json!("hotbar_items"))].into(),
        children: (0..3).map(child).collect(),
        factory: None,
    })
}

#[test]
fn retained_hotbar_clears_an_emptied_slot_when_the_icon_table_compacts() {
    let root = hotbar();
    let mut state = BindState::new();
    let refresh = |icons: [Option<usize>; 3], state: &mut BindState| {
        let model = HudModel {
            hotbar: icons
                .into_iter()
                .map(|icon| HudSlot {
                    icon,
                    count: u32::from(icon.is_some()),
                    ..Default::default()
                })
                .collect(),
            ..Default::default()
        };
        let (bound, diagnostics) =
            bind_stateful(&root, &hud_data_source(&model), &EmptyLibrary, state);
        assert!(diagnostics.is_empty(), "{diagnostics:?}");
        bound
            .children
            .iter()
            .map(|child| child.properties["#item_renderer_data"].clone())
            .collect::<Vec<_>>()
    };
    assert_eq!(
        refresh([Some(0), Some(1), Some(2)], &mut state),
        [json!(0.0), json!(1.0), json!(2.0)]
    );
    // Moving the middle item to offhand removes its icon from the table;
    // the following occupied slot now names index 1 instead of index 2.
    for _ in 0..2 {
        assert_eq!(
            refresh([Some(0), None, Some(1)], &mut state),
            [json!(0.0), Value::Null, json!(1.0)]
        );
    }
    assert_eq!(
        refresh([Some(0), Some(1), Some(2)], &mut state),
        [json!(0.0), json!(1.0), json!(2.0)]
    );
}

#[test]
fn unused_boss_slots_clear_names_and_hide_prefix_gated_backgrounds() {
    let child = |index| ResolvedControl {
        name: format!("slot{index}"),
        control_type: Some("panel".into()),
        base: None,
        unresolved_base: None,
        properties: [
            ("collection_index".into(), json!(index)),
            (
                "bindings".into(),
                json!([
                    {
                        "binding_type": "collection",
                        "binding_collection_name": "boss_bars",
                        "binding_name": "#bossName"
                    },
                    {
                        "binding_type": "view",
                        "source_property_name": "(not ((#bossName - 'message:') = #bossName))",
                        "target_property_name": "#visible"
                    }
                ]),
            ),
        ]
        .into(),
        children: Vec::new(),
        factory: None,
    };
    let root = Arc::new(ResolvedControl {
        name: "fixed_boss_slots".into(),
        control_type: Some("panel".into()),
        base: None,
        unresolved_base: None,
        properties: [("collection_name".into(), json!("boss_bars"))].into(),
        children: (0..8).map(child).collect(),
        factory: None,
    });
    let mut state = BindState::new();
    for names in [vec![], vec!["Dragon"], vec!["message:Hello"], vec![]] {
        let model = HudModel {
            boss_bars: names
                .iter()
                .map(|name| BossBar {
                    name: (*name).into(),
                    ..Default::default()
                })
                .collect(),
            ..Default::default()
        };
        let data = hud_data_source(&model);
        let (bound, diagnostics) = bind_stateful(&root, &data, &EmptyLibrary, &mut state);
        assert!(diagnostics.is_empty(), "{diagnostics:?}");
        for (index, child) in bound.children.iter().enumerate() {
            let name = names.get(index).copied().unwrap_or("");
            assert_eq!(child.properties.get("#bossName"), Some(&json!(name)));
            assert_eq!(
                child.properties.get("visible"),
                Some(&json!(name.contains("message:"))),
                "unused or unrelated slot {index} must not draw a background"
            );
        }
    }
}

#[test]
fn absorption_native_hearts_are_gated_by_survival_ui_and_keep_their_renderer() {
    let control = Arc::new(ResolvedControl {
        name: "heart_renderer".into(),
        control_type: Some("custom".into()),
        base: None,
        unresolved_base: None,
        properties: [
            ("renderer".into(), json!("heart_renderer")),
            (
                "bindings".into(),
                json!([{
                    "binding_name": "#show_survival_ui", "binding_name_override": "#visible"
                }]),
            ),
        ]
        .into(),
        children: Vec::new(),
        factory: None,
    });
    let mut state = BindState::new();
    for survival_ui in [true, false, true] {
        let (bound, diagnostics) = bind_stateful(
            &control,
            &hud_data_source(&HudModel {
                survival_ui,
                ..Default::default()
            }),
            &EmptyLibrary,
            &mut state,
        );
        assert!(diagnostics.is_empty());
        assert_eq!(bound.properties["visible"], json!(survival_ui));
        assert_eq!(bound.properties["renderer"], json!("heart_renderer"));
    }
}
