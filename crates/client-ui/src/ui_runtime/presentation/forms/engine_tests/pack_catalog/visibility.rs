use std::sync::Arc;

use super::{Draw, HudModel, Timed, layer, layer_pack_catalog, render_model, vanilla};
use crate::ui_runtime::{
    SequencedUiEvent, UiRuntime,
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
        &layer_pack_catalog(&vanilla, std::slice::from_ref(&pack)),
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

#[test]
fn derived_chat_label_keeps_its_message_filter_when_builtin_chat_is_restored() {
    let Some(vanilla) =
        vanilla("derived_chat_label_keeps_its_message_filter_when_builtin_chat_is_restored")
    else {
        return;
    };
    let pack = synthetic_chat_pack();
    let model = HudModel {
        chat_visible: true,
        chat_lifetime: 10.0,
        chat: ["Ordinary message", "killf.Aster/Birch"]
            .map(|text| Timed {
                text: text.into(),
                born: 0.0,
            })
            .into(),
        ..Default::default()
    };
    let drawn = render_model(
        &layer_pack_catalog(&vanilla, &[pack]),
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
    assert!(labels.contains(&"Ordinary message"));
    assert!(
        !labels.contains(&"killf.Aster/Birch"),
        "restoring chat lost the derived label's filter"
    );
    assert_eq!(
        labels.iter().filter(|text| **text == "Styled feed").count(),
        1
    );
}

/// Builds an original pack with a derived-label filter, a translation and a colored glyph.
fn synthetic_chat_pack() -> Vec<(String, Vec<u8>)> {
    let mut files = layer(br##"{
        "namespace":"hud",
        "root_panel":{"modifications":[{
            "array_name":"controls","operation":"insert_front","value":[
                {"custom_chat":{"type":"stack_panel","size":[200,"100%c"],
                    "factory":{"name":"chat_item_factory","control_ids":{"chat_item":"hud.custom_row"}}}},
                {"effects":{"type":"panel","size":["100%","100%"],
                    "factory":{"name":"chat_item_factory","control_ids":{"chat_item":"hud.effect"}}}}
            ]
        }]},
        "custom_row@hud.chat_grid_item":{"controls":[{
            "message@hud.chat_label":{
                "$condition":"(not (('%.6s' * #text) = 'killf.'))",
                "bindings":[{
                    "binding_type":"view","source_property_name":"$condition",
                    "target_property_name":"#visible"
                }]
            }
        }]},
        "chat_label":{"visible":false},
        "effect":{"type":"label","text":"Styled feed","size":[100,9],
            "bindings":[{"binding_type":"view",
                "source_property_name":"(('%.6s' * #text) = 'killf.')","target_property_name":"#visible"}]}
    }"##);
    files.push((
        "texts/en_US.lang".into(),
        "killf.test=Aster \u{e101} Birch\n".as_bytes().to_vec(),
    ));
    let mut sheet = image::RgbaImage::new(128, 128);
    for y in 1..7 {
        for x in 9..15 {
            sheet.put_pixel(x, y, image::Rgba([255, 80, 30, 255]));
        }
    }
    let mut png = std::io::Cursor::new(Vec::new());
    sheet.write_to(&mut png, image::ImageFormat::Png).unwrap();
    files.push(("font/glyph_E1.png".into(), png.into_inner()));
    files.push((
        "ui/_global_variables.json".into(),
        br##"{
        "$test_chat_visibility":[{"binding_type":"view",
            "source_property_name":"$condition","target_property_name":"#visible"}]
    }"##
        .to_vec(),
    ));
    files.push((
        "ui/chat_screen.json".into(),
        br##"{
        "messages_text/text":{"bindings":"$test_chat_visibility",
            "$condition":"(not (('%.6s' * #text) = 'killf.'))"}
    }"##
        .to_vec(),
    ));
    files
}

#[test]
fn admitted_chat_pack_filters_open_chat_without_losing_translated_glyphs() {
    let Some(mut presentation) =
        crate::test_support::engine_presentation_with(pack_harness::font())
    else {
        eprintln!(
            "skipping admitted_chat_pack_filters_open_chat_without_losing_translated_glyphs: missing installed UI carrier; make assets"
        );
        return;
    };
    let files = synthetic_chat_pack();
    let view = resource_pack::test_support::pack_view(&files);
    let lang = view.read("texts/en_US.lang").unwrap();
    let image = image::load_from_memory(&view.read("font/glyph_E1.png").unwrap())
        .unwrap()
        .into_rgba8();
    let cells = assets::extract_cells(&assets::GlyphSheet {
        high_byte: 0xe1,
        width: image.width(),
        height: image.height(),
        rgba8: image.into_raw().into_boxed_slice(),
    });
    let mut player = player_state::PlayerState::new(1);
    player
        .facts
        .publish_player_game_mode(protocol::PlayerGameMode::Survival);
    let mut runtime = UiRuntime::new(1);
    runtime.set_server_lang(assets::ServerLangOverlay::read(lang.len(), |target| {
        target.copy_from_slice(&lang);
        true
    }));
    runtime.set_session_glyphs(Some(Arc::new(
        crate::ui_runtime::presentation::SessionGlyphSheets {
            cells,
            ..Default::default()
        },
    )));
    runtime.set_server_ui(Some(Arc::new(ServerUiPack {
        ui_layers: vec![files],
        view: Some(view),
        ..Default::default()
    })));
    for (index, (kind, needs_translation, message)) in [
        (protocol::TextKind::Raw, false, "killf.Aster/Birch"),
        (protocol::TextKind::Translation, true, "killf.test"),
    ]
    .into_iter()
    .enumerate()
    {
        runtime
            .apply(
                &mut player,
                SequencedUiEvent {
                    session_id: 1,
                    fifo_sequence: index as u64 + 1,
                    local_millis: 0,
                    server_tick: None,
                    event: protocol::UiEvent::Text(protocol::TextEvent {
                        category: protocol::TextCategory::Parameters,
                        kind,
                        needs_translation,
                        source: None,
                        message: Arc::from(message),
                        parameters: Arc::from([]),
                        xuid: Arc::from(""),
                        platform_chat_id: Arc::from(""),
                        filtered_message: None,
                    }),
                },
            )
            .unwrap();
    }
    runtime.open_chat(&mut player);
    let input = presentation
        .build(
            &player,
            &runtime,
            100,
            [1280, 720],
            ui::DpiScale::new(1.0).unwrap(),
        )
        .unwrap();
    let labels = pack_harness::drawn_texts(&presentation.last_frame.as_ref().unwrap().nodes);
    assert!(!labels.contains(&"killf.Aster/Birch".to_owned()));
    assert!(labels.contains(&"Aster \u{e101} Birch".to_owned()));
    assert!(
        input
            .batches
            .iter()
            .any(|batch| input.textures.pages()[batch.texture_page as usize]
                .pixels()
                .as_chunks::<4>()
                .0
                .iter()
                .any(|pixel| pixel == &[255, 80, 30, 255])),
        "the visible glyph must use the pack's colored sheet pixels"
    );
}
