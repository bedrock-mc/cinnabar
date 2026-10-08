use json_ui::{
    Catalog, Context, Draw, DrawNode, HUD_SCREEN, HudModel, HudSlot, LayoutEnv, ScreenRender,
    Sidebar, TextMeasure, TextureMeta, TextureSource, Timed, hud_context, hud_data_source,
    render_screen,
};

use super::layer_pack_catalog;

mod admitted_titles;
mod effects;
mod nested_titles;
mod retained_titles;
mod titles;
mod visibility;

struct FixedText;

impl TextMeasure for FixedText {
    fn extent(&self, text: &str) -> [f64; 2] {
        if text.is_empty() {
            [0.0; 2]
        } else {
            [text.chars().count() as f64 * 6.0, 9.0]
        }
    }
}

struct NoTextures;

impl TextureSource for NoTextures {
    fn texture(&self, _: &str) -> Option<TextureMeta> {
        None
    }
}

fn vanilla(test: &str) -> Option<Catalog> {
    let Some(carrier) = crate::test_support::pack_harness::carrier() else {
        eprintln!("skipping {test}: missing installed UI carrier; make assets");
        return None;
    };
    Some(
        Catalog::from_files(
            carrier
                .ui_files()
                .iter()
                .map(|file| (file.path.as_ref(), file.bytes.as_ref())),
        )
        .expect("installed vanilla UI definitions"),
    )
}

fn layer(hud: &[u8]) -> Vec<(String, Vec<u8>)> {
    vec![("ui/hud_screen.json".to_owned(), hud.to_vec())]
}

fn render(catalog: &Catalog) -> ScreenRender {
    let model = HudModel {
        survival_ui: true,
        hotbar_visible: true,
        xp_bar: true,
        hotbar: vec![HudSlot::default(); 9],
        item_name: Some(Timed {
            text: "Selected item".to_owned(),
            born: 0.0,
        }),
        chat_visible: true,
        chat_lifetime: 10.0,
        chat: vec![Timed {
            text: "Chat line".to_owned(),
            born: 0.0,
        }],
        ..Default::default()
    };
    render_model(catalog, &model, [480.0, 270.0])
}

fn render_model(catalog: &Catalog, model: &HudModel, viewport: [f64; 2]) -> ScreenRender {
    render_screen(
        HUD_SCREEN,
        catalog,
        &hud_context(&Context::desktop()),
        &hud_data_source(model),
        viewport,
        &LayoutEnv {
            text: &FixedText,
            textures: &NoTextures,
        },
        &Default::default(),
    )
    .expect("HUD screen")
}

fn text<'a>(render: &'a ScreenRender, expected: &str) -> &'a DrawNode {
    render
        .nodes
        .iter()
        .find(|node| matches!(&node.draw, Draw::Text { text, .. } if text == expected))
        .unwrap_or_else(|| panic!("HUD did not draw {expected}"))
}

fn hotbar_top(render: &ScreenRender) -> f64 {
    render
        .nodes
        .iter()
        .filter_map(|node| match &node.draw {
            Draw::Custom { renderer, .. } if renderer == "hotbar_renderer" => Some(node.dest.y),
            _ => None,
        })
        .min_by(f64::total_cmp)
        .expect("hotbar cells")
}

const CHAT_STYLE: &[u8] = br#"{
    "namespace": "hud",
    "chat_label": {"color": [0.25, 0.5, 0.75]}
}"#;

#[test]
fn server_chat_style_keeps_builtin_chat_geometry_and_color() {
    let Some(vanilla) = vanilla("server_chat_style_keeps_builtin_chat_geometry_and_color") else {
        return;
    };
    let base = render(&layer_pack_catalog(&vanilla, &[]));
    let styled = render(&layer_pack_catalog(&vanilla, &[layer(CHAT_STYLE)]));
    let expected = text(&base, "Chat line");
    let actual = text(&styled, "Chat line");
    assert_eq!(actual.dest, expected.dest, "an unrelated style moved chat");
    assert!(actual.dest.y > 270.0 / 2.0);
    assert_eq!(actual.draw, expected.draw);
}

#[test]
fn partial_server_hud_style_keeps_item_name_clear_of_hotbar() {
    let Some(vanilla) = vanilla("partial_server_hud_style_keeps_item_name_clear_of_hotbar") else {
        return;
    };
    let base = render(&layer_pack_catalog(&vanilla, &[]));
    let styled = render(&layer_pack_catalog(&vanilla, &[layer(CHAT_STYLE)]));
    let expected = text(&base, "Selected item");
    let actual = text(&styled, "Selected item");
    assert_eq!(
        actual.dest, expected.dest,
        "an unrelated style moved the item name"
    );
    assert!(
        actual.dest.y + actual.dest.h < hotbar_top(&styled),
        "selected-item name intersects the hotbar"
    );
}

#[test]
fn explicit_server_chat_anchor_keeps_the_builtin_hud() {
    let Some(vanilla) = vanilla("explicit_server_chat_anchor_keeps_the_builtin_hud") else {
        return;
    };
    let base = render(&layer_pack_catalog(&vanilla, &[]));
    let overlay = layer(
        br#"{
        "namespace": "hud",
        "root_panel/chat_stack": {
            "type": "panel", "size": ["100%", "100%"],
            "controls": [{"server_chat@hud.chat_panel": {
                "anchor_from": "top_left", "anchor_to": "top_left"
            }}]
        }
    }"#,
    );
    let moved = render(&layer_pack_catalog(&vanilla, &[overlay]));
    assert_eq!(
        text(&moved, "Chat line").dest,
        text(&base, "Chat line").dest
    );
    assert_eq!(
        moved
            .nodes
            .iter()
            .filter(|node| matches!(&node.draw,
        Draw::Text { text, .. } if text == "Chat line"))
            .count(),
        1
    );
}

#[test]
fn alternate_server_chat_factory_does_not_duplicate_chat_or_hide_other_widgets() {
    let Some(vanilla) =
        vanilla("alternate_server_chat_factory_does_not_duplicate_chat_or_hide_other_widgets")
    else {
        return;
    };
    let base = render(&layer_pack_catalog(&vanilla, &[]));
    let overlay = layer(
        br##"{
        "namespace":"hud",
        "root_panel":{"modifications":[{"array_name":"controls","operation":"insert_front","value":[
            {"alternate_chat":{"type":"stack_panel","size":[240,"100%c"],
                "anchor_from":"top_left","anchor_to":"top_left",
                "factory":{"name":"chat_item_factory","control_name":"hud.alternate_item"}}},
            {"server_widget":{"type":"panel","size":[80,12],
                "anchor_from":"top_right","anchor_to":"top_right",
                "factory":{"name":"chat_item_factory","control_name":"hud.notice",
                    "control_ids":{"chat_item":"hud.chat_grid_item"}}}},
            {"mixed_factory":{"type":"panel","size":[100,12],
                "anchor_from":"top_right","anchor_to":"top_right","offset":[0,15],
                "factory":{"name":"chat_item_factory","control_ids":{
                    "chat_item":"hud.chat_grid_item","notice":"hud.notice"}},
                "controls":[{"literal":{"type":"label","text":"Literal widget","size":[100,9]}}]}}
        ]}]},
        "alternate_item@hud.chat_grid_item":{},
        "notice":{"type":"label","text":"Server notice","size":[80,9]},
        "chat_label":{"visible":false}
    }"##,
    );
    let drawn = render(&layer_pack_catalog(&vanilla, &[overlay]));
    assert_eq!(
        text(&drawn, "Chat line").dest,
        text(&base, "Chat line").dest
    );
    assert_eq!(
        drawn
            .nodes
            .iter()
            .filter(|node| matches!(&node.draw,
        Draw::Text { text, .. } if text == "Chat line"))
            .count(),
        1
    );
    assert!(text(&drawn, "Server notice").dest.x > 480.0 / 2.0);
    assert!(text(&drawn, "Literal widget").dest.x > 480.0 / 2.0);
}

#[test]
fn builtin_chat_preserves_server_additional_content_and_main_hud() {
    let Some(vanilla) = vanilla("builtin_chat_preserves_server_additional_content_and_main_hud")
    else {
        return;
    };
    let overlay = layer(
        br#"{
        "namespace":"hud",
        "hud_screen":{"$additional_screen_content":"hud.server_extra"},
        "server_extra":{"type":"label","text":"Server overlay","size":[100,9],
            "anchor_from":"top_right","anchor_to":"top_right"}
    }"#,
    );
    let drawn = render(&layer_pack_catalog(&vanilla, &[overlay]));
    assert!(text(&drawn, "Server overlay").dest.x > 480.0 / 2.0);
    assert!(text(&drawn, "Chat line").dest.y > 270.0 / 2.0);
    assert!(text(&drawn, "Selected item").dest.y < hotbar_top(&drawn));
}

#[test]
fn fullscreen_server_sidebar_keeps_its_authored_top_right_position() {
    let Some(vanilla) = vanilla("fullscreen_server_sidebar_keeps_its_authored_top_right_position")
    else {
        return;
    };
    let overlay = vec![(
        "ui/scoreboards.json".to_owned(),
        br##"{
            "namespace": "scoreboard",
            "scoreboard_sidebar": {
                "type": "panel", "size": ["100%", "100%"],
                "controls": [{"entries": {
                    "type": "panel", "size": [100, 10],
                    "anchor_from": "top_right", "anchor_to": "top_right",
                    "offset": [-2, 5],
                    "controls": [{"level": {
                        "type": "label", "text": "Level 1/100",
                        "size": ["100%", 9], "offset": [1, 3],
                        "anchor_from": "top_left", "anchor_to": "top_left"
                    }}]
                }}]
            }
        }"##
        .to_vec(),
    )];
    let model = HudModel {
        sidebar: Some(Sidebar::default()),
        ..Default::default()
    };
    let server_on_vanilla = {
        let mut catalog = vanilla.clone();
        catalog.apply_pack(
            overlay
                .iter()
                .map(|(path, bytes)| (path.as_str(), bytes.as_slice())),
        );
        catalog
    };
    let layered = layer_pack_catalog(&vanilla, &[layer(CHAT_STYLE), overlay]);
    for viewport in [[480.0, 270.0], [960.0, 540.0]] {
        let native = render_model(&server_on_vanilla, &model, viewport);
        let styled = render_model(&layered, &model, viewport);
        let expected = text(&native, "Level 1/100");
        let actual = text(&styled, "Level 1/100");
        assert_eq!(actual.dest, expected.dest);
        assert_eq!(actual.dest.y, 8.0);
        assert_eq!(actual.dest.x + actual.dest.w, viewport[0] - 1.0);
    }
}

#[test]
fn builtin_sidebar_preserves_its_position_for_different_row_counts() {
    let Some(vanilla) = vanilla("builtin_sidebar_preserves_its_position_for_different_row_counts")
    else {
        return;
    };
    let catalog = layer_pack_catalog(&vanilla, &[]);
    for rows in [1, 3, 15] {
        let model = HudModel {
            sidebar: Some(Sidebar {
                title: "Objective".to_owned(),
                rows: (0..rows)
                    .map(|row| (format!("Player {row}"), row.to_string()))
                    .collect(),
                ..Default::default()
            }),
            ..Default::default()
        };
        for viewport in [[480.0, 270.0], [960.0, 540.0]] {
            let drawn = render_model(&catalog, &model, viewport);
            let last = text(&drawn, &format!("Player {}", rows - 1));
            let height = 10.0 + f64::from(rows) * 9.0;
            let expected_bottom = viewport[1] / 2.0 + height / 3.0 - 10.0 / 3.0;
            assert!((last.dest.y + last.dest.h - expected_bottom).abs() < 1e-6);
        }
    }
}
