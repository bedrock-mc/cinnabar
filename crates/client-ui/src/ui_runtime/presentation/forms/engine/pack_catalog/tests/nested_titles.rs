use json_ui::HudTitle;

use super::{Draw, HudModel, layer, layer_pack_catalog, render_model, vanilla};

fn model() -> HudModel {
    HudModel {
        title: Some(HudTitle {
            title: "Round finished".into(),
            subtitle: "Spectating".into(),
            stay: 3.0,
            ..Default::default()
        }),
        ..Default::default()
    }
}

#[test]
fn native_nested_title_overrides_hide_each_authored_label() {
    let Some(vanilla) = vanilla("native_nested_title_overrides_hide_each_authored_label") else {
        return;
    };
    let pack = layer(
        br##"{
        "namespace":"hud",
        "hud_title_text/title_frame/title":{"visible":false},
        "hud_title_text/subtitle_frame/subtitle":{"visible":false}
    }"##,
    );
    let frame = render_model(
        &layer_pack_catalog(&vanilla, &[pack.clone()]),
        &model(),
        [480.0, 270.0],
    );
    assert!(
        !frame.nodes.iter().any(|node| matches!(&node.draw,
            Draw::Text { text, .. } if text == "Round finished" || text == "Spectating")),
        "nested server overrides did not suppress the inherited title labels"
    );
}

#[test]
fn native_title_frame_replacement_draws_only_server_labels() {
    let Some(vanilla) = vanilla("native_title_frame_replacement_draws_only_server_labels") else {
        return;
    };
    let pack = layer(br##"{
        "namespace":"hud",
        "hud_title_text":{"modifications":[
            {"array_name":"controls","operation":"remove","control_name":"title_frame"},
            {"array_name":"controls","operation":"remove","control_name":"subtitle_frame"},
            {"array_name":"controls","operation":"insert_back","value":[
                {"server_title":{"type":"label","text":"#text","size":[120,9],"font_scale_factor":1.0,"offset":[-100,-30],"bindings":[
                    {"binding_name":"#hud_title_text_string","binding_name_override":"#text"}
                ]}},
                {"server_subtitle":{"type":"label","text":"#text","size":[120,9],"font_scale_factor":1.0,"offset":[-100,-15],"bindings":[
                    {"binding_name":"#hud_subtitle_text_string","binding_name_override":"#text"}
                ]}}
            ]}
        ]}
    }"##);
    let frame = render_model(
        &layer_pack_catalog(&vanilla, &[pack.clone()]),
        &model(),
        [480.0, 270.0],
    );
    let published = super::titles::published_frame(
        &pack,
        "Round finished",
        "Spectating",
        "",
        "native-title-frame-replacement",
    );
    for expected in ["Round finished", "Spectating"] {
        let labels = frame
            .nodes
            .iter()
            .filter(|node| {
                matches!(&node.draw,
            Draw::Text { text, .. } if text == expected)
            })
            .collect::<Vec<_>>();
        assert_eq!(
            labels.len(),
            1,
            "title frame replacement left duplicate {expected}"
        );
        assert!(
            labels[0].dest.x < 240.0,
            "server label lost its authored position"
        );
        assert_eq!(
            published
                .iter()
                .filter(|text| text.as_str() == expected)
                .count(),
            1
        );
    }
}
