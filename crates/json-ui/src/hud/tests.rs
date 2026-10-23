use std::sync::Arc;

use serde_json::{Value, json};

use super::{BossBar, ChatText, HudModel, HudSlot, HudTitle, Timed, hud_data_source};
use crate::{BindState, EmptyLibrary, ResolvedControl, bind_stateful};

#[test]
fn absent_hud_title_answers_empty_text_and_clears_retained_labels() {
    for binding in ["#hud_title_text_string", "#hud_subtitle_text_string"] {
        let root = Arc::new(ResolvedControl {
            name: "retained_title".into(),
            control_type: Some("label".into()),
            base: None,
            unresolved_base: None,
            properties: [
                ("text".into(), json!("#text")),
                ("property_bag".into(), json!({"#text":"seed"})),
                ("bindings".into(), json!([
                    {"binding_name":binding,"binding_name_override":"#text"},
                    {"binding_type":"view","source_property_name":"(not (#text = ''))","target_property_name":"#visible"}
                ])),
            ].into(),
            children: Vec::new(),
            factory: None,
        });
        let mut state = BindState::new();
        for title in [
            None,
            Some(HudTitle {
                title: "Title".into(),
                subtitle: "Subtitle".into(),
                ..Default::default()
            }),
            None,
        ] {
            let expected = title.as_ref().map_or("", |title| {
                if binding == "#hud_title_text_string" {
                    title.title.as_str()
                } else {
                    title.subtitle.as_str()
                }
            });
            let model = HudModel {
                title: title.clone(),
                ..Default::default()
            };
            let (bound, notes) =
                bind_stateful(&root, &hud_data_source(&model), &EmptyLibrary, &mut state);
            assert!(notes.is_empty(), "{notes:?}");
            assert_eq!(bound.properties.get("text"), Some(&json!(expected)));
            assert_eq!(
                bound.properties.get("visible"),
                Some(&json!(!expected.is_empty()))
            );
        }
    }
}

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

#[test]
fn legacy_game_mode_bindings_keep_pack_hotbars_and_status_rows_visible() {
    let mut catalog = crate::Catalog::default();
    catalog.overlay_text(
        "ui/legacy_hud.json",
        r##"{"namespace":"test","hud":{"type":"panel","controls":[
            {"survival":{"type":"panel","bindings":[
                {"binding_name":"#is_not_creative_mode","binding_name_override":"#visible"}
            ]}},
            {"creative":{"type":"panel","bindings":[
                {"binding_name":"#is_creative_mode","binding_name_override":"#visible"}
            ]}}
        ]}}"##,
    );
    let root = Arc::new(
        crate::resolve(&catalog, "test.hud", &crate::Context::desktop())
            .control
            .unwrap(),
    );
    let mut state = BindState::new();
    for (survival_ui, spectator, shown) in [
        (true, false, [true, false]),
        (false, false, [false, true]),
        (false, true, [false, false]),
        (true, false, [true, false]),
    ] {
        let model = HudModel {
            survival_ui,
            spectator,
            ..Default::default()
        };
        let (bound, notes) =
            bind_stateful(&root, &hud_data_source(&model), &EmptyLibrary, &mut state);
        assert!(notes.is_empty(), "{notes:?}");
        for (child, shown) in bound.children.iter().zip(shown) {
            assert_eq!(child.properties.get("visible"), Some(&json!(shown)));
        }
    }
}

#[test]
fn hotbar_item_id_aux_shows_a_pack_overlay_only_over_its_item() {
    let overlay = |index| ResolvedControl {
        name: format!("overlay{index}"),
        control_type: Some("image".into()),
        base: None,
        unresolved_base: None,
        properties: [
            ("collection_index".into(), json!(index)),
            (
                "bindings".into(),
                json!([
                    {
                        "binding_type": "collection",
                        "binding_collection_name": "hotbar_items",
                        "binding_name": "#item_id_aux"
                    },
                    {
                        "binding_type": "view",
                        "source_property_name": "(#item_id_aux / 65536)",
                        "target_property_name": "#item_runtime_id"
                    },
                    {
                        "binding_type": "view",
                        "source_property_name": "(#item_runtime_id = 448)",
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
        name: "hotbar".into(),
        control_type: Some("panel".into()),
        base: None,
        unresolved_base: None,
        properties: [("collection_name".into(), json!("hotbar_items"))].into(),
        children: (0..3).map(overlay).collect(),
        factory: None,
    });
    let model = HudModel {
        hotbar: [Some(448 << 16), None, Some(5 << 16)]
            .into_iter()
            .map(|id_aux| HudSlot {
                icon: id_aux.map(|_| 0),
                id_aux,
                count: u32::from(id_aux.is_some()),
                ..Default::default()
            })
            .collect(),
        ..Default::default()
    };
    let (bound, diagnostics) = bind_stateful(
        &root,
        &hud_data_source(&model),
        &EmptyLibrary,
        &mut BindState::new(),
    );
    assert!(diagnostics.is_empty(), "{diagnostics:?}");
    let visible: Vec<_> = bound
        .children
        .iter()
        .map(|child| child.properties.get("visible").cloned())
        .collect();
    assert_eq!(
        visible,
        [Some(json!(true)), Some(json!(false)), Some(json!(false))]
    );
}

#[test]
fn closed_chat_lines_use_the_chosen_font_size_and_line_spacing() {
    let chat_text = ChatText {
        font_scale: 1.5,
        line_spacing: 0.201,
    };
    let data = hud_data_source(&HudModel {
        chat_visible: true,
        chat: vec![Timed {
            text: "Hello".into(),
            born: 0.0,
        }],
        chat_text,
        ..Default::default()
    });
    let vars = &data.factories["chat_item_factory"][0].vars;
    assert_eq!(vars["chat_font_scale_factor"], json!(chat_text.font_scale));
    assert_eq!(vars["chat_line_spacing"], json!(chat_text.line_spacing));
}
