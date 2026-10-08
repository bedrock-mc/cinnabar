use std::sync::Arc;

use super::{Draw, HudModel, Timed, layer, layer_pack_catalog, render_model, vanilla};
use crate::ui_runtime::{
    UiRuntime,
    presentation::forms::{ServerUiPack, pack_harness, snapshot},
};

#[test]
fn server_chat_visibility_filters_rows_without_starving_effect_factories() {
    let Some(vanilla) =
        vanilla("server_chat_visibility_filters_rows_without_starving_effect_factories")
    else {
        return;
    };
    let pack = layer(
        br##"{
            "namespace":"hud",
            "chat_label":{"modifications":[{
                "array_name":"bindings","operation":"insert_back","value":{
                    "binding_type":"view",
                    "source_property_name":"((#text < 'fx.') or (#text > 'fx/'))",
                    "target_property_name":"#visible"
                }
            }]},
            "root_panel":{"modifications":[{
                "array_name":"controls","operation":"insert_front","value":[{
                    "effects":{
                        "type":"panel","size":["100%","100%"],
                        "factory":{"name":"chat_item_factory",
                            "control_ids":{"chat_item":"effect@hud.effect"}}
                    }
                }]
            }]},
            "effect":{
                "type":"label","text":"Effect shown","size":[100,9],
                "bindings":[
                    {"binding_type":"collection","binding_collection_name":"chat_text_grid",
                        "binding_name":"#chat_text","binding_name_override":"#trigger",
                        "binding_condition":"once"},
                    {"binding_type":"view","source_property_name":"(#trigger = 'fx.flash')",
                        "target_property_name":"#visible"}
                ]
            }
        }"##,
    );
    let model = HudModel {
        chat_visible: true,
        chat_lifetime: 10.0,
        chat: ["Ordinary message", "fx.flash"]
            .map(|text| Timed {
                text: text.into(),
                born: 0.0,
            })
            .into(),
        ..Default::default()
    };
    let drawn = render_model(
        &layer_pack_catalog(&vanilla, &[pack.clone()]),
        &model,
        [480.0, 270.0],
    );
    let labels = drawn
        .nodes
        .iter()
        .filter_map(|node| match &node.draw {
            Draw::Text { text, .. } => Some(text.as_str()),
            _ => None,
        })
        .collect::<Vec<_>>();
    let published = published_chat(&pack);
    assert!(labels.contains(&"Ordinary message"));
    assert!(
        !labels.contains(&"fx.flash"),
        "HUD exposed an authored UI trigger"
    );
    assert_eq!(
        labels
            .iter()
            .filter(|text| **text == "Effect shown")
            .count(),
        1,
        "filtering history must preserve the factory's full message collection"
    );
    assert!(published.contains(&"Ordinary message".to_owned()));
    assert!(!published.contains(&"fx.flash".to_owned()));
    assert_eq!(
        published
            .iter()
            .filter(|text| *text == "Effect shown")
            .count(),
        1
    );
}

fn published_chat(pack: &[(String, Vec<u8>)]) -> Vec<String> {
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
    for text in ["Ordinary message", "fx.flash"] {
        runtime.push_local_chat_line(Arc::from(text), 0);
    }
    let input = presentation
        .build(
            &player,
            &runtime,
            100,
            [1280, 720],
            ui::DpiScale::new(1.0).unwrap(),
        )
        .unwrap();
    snapshot::write(&input, "chat-trigger");
    pack_harness::drawn_texts(&presentation.last_frame.as_ref().unwrap().nodes)
}
