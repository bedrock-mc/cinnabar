use std::sync::Arc;

use json_ui::HudTitle;

use super::{Draw, HudModel, Timed, layer, layer_pack_catalog, render_model, vanilla};
use crate::ui_runtime::{
    UiRuntime,
    presentation::forms::{ServerUiPack, pack_harness, snapshot},
};

const AUTHOR_VISIBILITY: &[u8] = br##"{
    "namespace":"hud",
    "hud_title_text":{"modifications":[{
        "array_name":"bindings","operation":"insert_back","value":[
            {"binding_name":"#hud_title_text_string","binding_name_override":"#current_title"},
            {"binding_type":"view","source_property_name":"((#current_title < 'fx.') or (#current_title > 'fx/'))","target_property_name":"#visible"}
        ]
    }]},
    "hud_actionbar_text":{
        "$custom_actionbar_text":"$actionbar_text", "$visible|default":true,
        "variables":[{"requires":"(($custom_actionbar_text > 'fx.') and ($custom_actionbar_text < 'fx/'))","$visible":false}],
        "visible":"$visible"
    },
    "root_panel":{"modifications":[{"array_name":"controls","operation":"insert_front","value":[
        {"effect":{"type":"label","text":"Effect displayed","size":[120,9],"bindings":[
            {"binding_name":"#hud_title_text_string","binding_name_override":"#trigger"},
            {"binding_type":"view","source_property_name":"(#trigger = 'fx.flash')","target_property_name":"#visible"}
        ]}}
    ]}]}
}"##;

fn texts(frame: &json_ui::ScreenRender) -> Vec<&str> {
    frame
        .nodes
        .iter()
        .filter_map(|node| match &node.draw {
            Draw::Text { text, .. } => Some(text.as_str()),
            _ => None,
        })
        .collect()
}

#[test]
fn server_title_and_actionbar_visibility_preserves_effect_inputs() {
    let Some(vanilla) = vanilla("server_title_and_actionbar_visibility_preserves_effect_inputs")
    else {
        return;
    };
    let pack = layer(AUTHOR_VISIBILITY);
    let catalog = layer_pack_catalog(&vanilla, std::slice::from_ref(&pack));
    let model = HudModel {
        title: Some(HudTitle {
            title: "fx.flash".into(),
            subtitle: "Hidden subtitle".into(),
            stay: 3.0,
            ..Default::default()
        }),
        actionbar: Some(Timed {
            text: "fx.light".into(),
            born: 0.0,
        }),
        ..Default::default()
    };
    let hidden = render_model(&catalog, &model, [480.0, 270.0]);
    let labels = texts(&hidden);
    let published = published_frame(
        &pack,
        "fx.flash",
        "Hidden subtitle",
        "fx.light",
        "hud-triggers",
    );
    assert!(
        !labels.contains(&"fx.flash"),
        "authored title visibility was bypassed"
    );
    assert!(!labels.contains(&"Hidden subtitle"));
    assert!(
        !labels.contains(&"fx.light"),
        "authored actionbar visibility was bypassed"
    );
    assert_eq!(
        labels
            .iter()
            .filter(|text| **text == "Effect displayed")
            .count(),
        1
    );
    assert!(!published.contains(&"fx.flash".to_owned()));
    assert!(!published.contains(&"fx.light".to_owned()));
    assert_eq!(
        published
            .iter()
            .filter(|text| *text == "Effect displayed")
            .count(),
        1
    );
    let normal = render_model(
        &catalog,
        &HudModel {
            title: Some(HudTitle {
                title: "Ordinary title".into(),
                subtitle: "Ordinary subtitle".into(),
                stay: 3.0,
                ..Default::default()
            }),
            actionbar: Some(Timed {
                text: "Ordinary actionbar".into(),
                born: 0.0,
            }),
            ..Default::default()
        },
        [480.0, 270.0],
    );
    for expected in ["Ordinary title", "Ordinary subtitle", "Ordinary actionbar"] {
        assert_eq!(
            texts(&normal)
                .iter()
                .filter(|text| **text == expected)
                .count(),
            1
        );
    }
}

#[test]
fn server_custom_title_replaces_builtin_title_without_duplicate_draws() {
    let Some(vanilla) =
        vanilla("server_custom_title_replaces_builtin_title_without_duplicate_draws")
    else {
        return;
    };
    let pack = layer(br##"{
        "namespace":"hud",
        "hud_title_text":{"visible":false},
        "root_panel":{"modifications":[{"array_name":"controls","operation":"insert_front","value":[
            {"server_title":{"type":"panel","size":["100%","100%"],"factory":{
                "name":"hud_title_text_factory","control_ids":{"hud_title_text":"title@hud.authored_title"}
            }}}
        ]}]},
        "authored_title":{"type":"panel","size":["100%","100%"],"controls":[
            {"title":{"type":"label","text":"#text","size":[120,9],"offset":[0,-30],"bindings":[
                {"binding_name":"#hud_title_text_string","binding_name_override":"#text"}
            ]}},
            {"subtitle":{"type":"label","text":"#text","size":[120,9],"offset":[0,-12],"bindings":[
                {"binding_name":"#hud_subtitle_text_string","binding_name_override":"#text"}
            ]}}
        ]}
    }"##);
    let frame = render_model(
        &layer_pack_catalog(&vanilla, std::slice::from_ref(&pack)),
        &HudModel {
            title: Some(HudTitle {
                title: "Round finished".into(),
                subtitle: "Spectating".into(),
                stay: 3.0,
                ..Default::default()
            }),
            ..Default::default()
        },
        [480.0, 270.0],
    );
    let published = published_frame(&pack, "Round finished", "Spectating", "", "server-title");
    for expected in ["Round finished", "Spectating"] {
        assert_eq!(
            texts(&frame)
                .iter()
                .filter(|text| **text == expected)
                .count(),
            1,
            "built-in title also drew"
        );
        assert_eq!(published.iter().filter(|text| *text == expected).count(), 1);
    }
}

#[test]
fn admitted_hud_hides_its_authored_title_and_actionbar_triggers() {
    let Some(path) = std::env::var_os("CINNABAR_TEST_HUD_UI_DIR") else {
        eprintln!(
            "skipping admitted_hud_hides_its_authored_title_and_actionbar_triggers: missing CINNABAR_TEST_HUD_UI_DIR fixture"
        );
        return;
    };
    let path = std::path::PathBuf::from(path).join("hud_screen.json");
    let Ok(bytes) = std::fs::read(&path) else {
        eprintln!(
            "skipping admitted_hud_hides_its_authored_title_and_actionbar_triggers: missing {}",
            path.display()
        );
        return;
    };
    let Some(vanilla) = vanilla("admitted_hud_hides_its_authored_title_and_actionbar_triggers")
    else {
        return;
    };
    let pack = layer(&bytes);
    let frame = render_model(
        &layer_pack_catalog(&vanilla, std::slice::from_ref(&pack)),
        &HudModel {
            title: Some(HudTitle {
                title: "ui.halloween.flash".into(),
                subtitle: "Hidden subtitle".into(),
                stay: 3.0,
                ..Default::default()
            }),
            actionbar: Some(Timed {
                text: "ui.halloween.light".into(),
                born: 0.0,
            }),
            ..Default::default()
        },
        [480.0, 270.0],
    );
    let published = published_frame(
        &pack,
        "ui.halloween.flash",
        "Hidden subtitle",
        "ui.halloween.light",
        "admitted-hud-triggers",
    );
    for expected in [
        "ui.halloween.flash",
        "Hidden subtitle",
        "ui.halloween.light",
    ] {
        assert!(
            !texts(&frame).contains(&expected),
            "admitted HUD exposed {expected}"
        );
        assert!(
            !published.contains(&expected.to_owned()),
            "published admitted HUD exposed {expected}"
        );
    }
}

pub(super) fn published_frame(
    pack: &[(String, Vec<u8>)],
    title: &str,
    subtitle: &str,
    actionbar: &str,
    name: &str,
) -> Vec<String> {
    let mut presentation = crate::test_support::engine_presentation_with(pack_harness::font())
        .expect("installed UI carrier already loaded by this fixture");
    let mut player = player_state::PlayerState::new(1);
    player
        .facts
        .publish_player_game_mode(protocol::PlayerGameMode::Survival);
    let mut runtime = UiRuntime::new(1);
    runtime.set_server_ui(Some(Arc::new(ServerUiPack {
        ui_layers: vec![pack.to_vec()],
        ..Default::default()
    })));
    runtime.hud.set_title(Arc::from(title), 1, 0);
    runtime.hud.set_subtitle(Arc::from(subtitle), 2, 0);
    if !actionbar.is_empty() {
        runtime.hud.set_actionbar(Arc::from(actionbar), 3, 0);
    }
    let input = presentation
        .build(
            &player,
            &runtime,
            1000,
            [1280, 720],
            ui::DpiScale::new(1.0).unwrap(),
        )
        .unwrap();
    snapshot::write(&input, name);
    pack_harness::drawn_texts(&presentation.last_frame.as_ref().unwrap().nodes)
}
