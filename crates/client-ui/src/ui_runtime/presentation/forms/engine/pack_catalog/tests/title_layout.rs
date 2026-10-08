use json_ui::{Draw, HudModel, HudTitle};

use super::{layer, layer_pack_catalog, render_model, text, vanilla};

const TITLE_STYLE: &[u8] = br#"{
    "namespace": "hud",
    "hud_title_text/title_frame/title": {"font_size": "large", "shadow": true},
    "hud_title_text/subtitle_frame/subtitle": {"font_size": "medium", "shadow": true}
}"#;

#[test]
fn partial_server_title_sizes_inherit_the_native_stack_without_builtin_scaling() {
    let Some(base) =
        vanilla("partial_server_title_sizes_inherit_the_native_stack_without_builtin_scaling")
    else {
        return;
    };
    let layer = layer(TITLE_STYLE);
    let mut native = base.clone();
    native.apply_pack(
        layer
            .iter()
            .map(|(path, bytes)| (path.as_str(), bytes.as_slice())),
    );
    let model = HudModel {
        title: Some(HudTitle {
            title: "VICTORY".into(),
            subtitle: "YOU WON THE GAME!".into(),
            stay: 3.0,
            ..Default::default()
        }),
        ..Default::default()
    };
    let expected = render_model(&native, &model, [480.0, 270.0]);
    let actual = render_model(&layer_pack_catalog(&base, &[layer]), &model, [480.0, 270.0]);
    for (payload, expected_scale) in [("VICTORY", 2.0), ("YOU WON THE GAME!", 1.0)] {
        let expected = text(&expected, payload);
        let actual = text(&actual, payload);
        let Draw::Text { scale, .. } = actual.draw else {
            panic!("title label was not text");
        };
        assert_eq!(
            scale, expected_scale,
            "server title scale was multiplied by built-in styling"
        );
        assert_eq!(
            actual.dest, expected.dest,
            "server title did not use its native stack placement"
        );
    }
    let title = text(&actual, "VICTORY");
    let subtitle = text(&actual, "YOU WON THE GAME!");
    assert!(
        title.dest.y + title.dest.h < subtitle.dest.y,
        "title overlaps its subtitle"
    );
}

#[test]
fn later_title_style_preserves_an_earlier_server_factory() {
    let Some(base) = vanilla("later_title_style_preserves_an_earlier_server_factory") else {
        return;
    };
    let factory = layer(br##"{
        "namespace":"hud",
        "root_panel":{"modifications":[{
            "array_name":"controls", "operation":"replace", "control_name":"hud_title_text_area",
            "value":[{"hud_title_text_area":{"type":"panel","factory":{
                "name":"hud_title_text_factory", "control_ids":{"hud_title_text":"server@hud.authored_title"}
            }}}]
        }]},
        "authored_title":{"type":"label","text":"PACK VICTORY","size":[120,9],"offset":[0,-20]}
    }"##);
    let model = HudModel {
        title: Some(HudTitle {
            title: "VICTORY".into(),
            stay: 3.0,
            ..Default::default()
        }),
        ..Default::default()
    };
    let frame = render_model(
        &layer_pack_catalog(&base, &[factory, layer(TITLE_STYLE)]),
        &model,
        [480.0, 270.0],
    );
    let labels = frame
        .nodes
        .iter()
        .filter_map(|node| match &node.draw {
            Draw::Text { text, .. } => Some(text.as_str()),
            _ => None,
        })
        .collect::<Vec<_>>();
    assert_eq!(labels, ["PACK VICTORY"]);
}

#[test]
fn later_partial_title_patch_keeps_earlier_native_label_edits() {
    let Some(base) = vanilla("later_partial_title_patch_keeps_earlier_native_label_edits") else {
        return;
    };
    let first = layer(br#"{"namespace":"hud","hud_title_text/title_frame/title":{"font_size":"large","offset":[0,2]}}"#);
    let second = layer(br#"{"namespace":"hud","hud_title_text/subtitle_frame/subtitle":{"font_size":"medium","offset":[0,14]}}"#);
    let mut native = base.clone();
    for files in [&first, &second] {
        native.apply_pack(
            files
                .iter()
                .map(|(path, bytes)| (path.as_str(), bytes.as_slice())),
        );
    }
    let model = HudModel {
        title: Some(HudTitle {
            title: "VICTORY".into(),
            subtitle: "YOU WON THE GAME!".into(),
            stay: 3.0,
            ..Default::default()
        }),
        ..Default::default()
    };
    let expected = render_model(&native, &model, [480.0, 270.0]);
    let actual = render_model(
        &layer_pack_catalog(&base, &[first, second]),
        &model,
        [480.0, 270.0],
    );
    for payload in ["VICTORY", "YOU WON THE GAME!"] {
        assert_eq!(text(&actual, payload).dest, text(&expected, payload).dest);
        assert_eq!(text(&actual, payload).draw, text(&expected, payload).draw);
    }
}
