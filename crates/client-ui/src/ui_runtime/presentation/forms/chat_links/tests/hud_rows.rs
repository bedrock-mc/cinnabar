//! Java HUD row geometry through the real model, authored pack, and bitmap label adapter.

use std::sync::Arc;

use json_ui::{Catalog, Context, HUD_SCREEN, HudModel, Timed, hud_data_source, render_screen};

use crate::ui_runtime::presentation::forms::engine::{
    EngineInputs, EngineOutput, FormEngine, ScreenArt,
};
use crate::ui_runtime::presentation::{TextMetrics, UiPresentationRuntime};

#[test]
fn builtin_java_hud_keeps_single_and_wrapped_chat_rows_nine_pixels_high() {
    let catalog = Catalog::from_files([
        ("ui/_global_variables.json", b"{}".as_slice()),
        (
            "ui/_ui_defs.json",
            br#"{"ui_defs":["ui/hud_screen.json"]}"#.as_slice(),
        ),
        (
            "ui/hud_screen.json",
            br##"{
                "namespace":"hud",
                "hud_screen": {
                    "type":"panel", "size":[32,"100%"],
                    "anchor_from":"top_left", "anchor_to":"top_left",
                    "controls":[{"history@hud.chat_panel":{
                        "$cinnabar_chat_anchor":"top_left"
                    }}]
                },
                "chat_label": {
                    "type":"label", "text":"#text", "localize":false,
                    "font_type":"$chat_font_type",
                    "font_scale_factor":"$chat_font_scale_factor",
                    "line_padding":"$chat_line_spacing",
                    "bindings":[{"binding_name":"#text"}]
                }
            }"##
            .as_slice(),
        ),
    ])
    .unwrap();
    let model = HudModel {
        chat_visible: true,
        chat_lifetime: 10.0,
        chat_background_opacity: 0.5,
        chat: ["0", "0 0 0", "0"]
            .map(|text| Timed {
                text: text.into(),
                born: 0.0,
            })
            .into(),
        ..Default::default()
    };
    let data = hud_data_source(&model);
    for (physical, gui_scale) in [([854, 459], 1.0), ([1280, 720], 2.0)] {
        let font = crate::test_support::fixture_font();
        let mut presentation = UiPresentationRuntime::new(Arc::clone(&font)).unwrap();
        let engine = FormEngine::new(crate::test_support::mini_carrier(), catalog.clone(), 0);
        let mut drawn = Vec::new();
        let mut painted = Vec::new();
        let mut next = 1;
        engine
            .draw(
                ScreenArt::default(),
                EngineInputs {
                    layouts: &mut presentation.layouts,
                    font: &font,
                    metrics: TextMetrics::for_viewport(
                        physical,
                        ui::DpiScale::new(1.0).unwrap(),
                        None,
                    ),
                    solid_page: presentation.solid_texture_page,
                    safe_area: ui::SafeArea::ZERO,
                    content: physical.map(|side| side as f32),
                    translate: &|_| None,
                    language: [0; 3],
                },
                EngineOutput {
                    nodes: &mut painted,
                    next: &mut next,
                    overlay: &[],
                },
                |env, root| {
                    let render = render_screen(
                        HUD_SCREEN,
                        engine.catalog(),
                        &Context::desktop(),
                        &data,
                        root,
                        env,
                        &Default::default(),
                    )?;
                    drawn = render.nodes.clone();
                    Some(render)
                },
            )
            .unwrap()
            .expect("Java chat fixture draws");
        let backgrounds = drawn
            .iter()
            .filter(|node| node.name == "chat_background")
            .map(|node| (node.dest.y, node.dest.h))
            .collect::<Vec<_>>();
        assert_eq!(backgrounds, [(0.0, 9.0), (9.0, 18.0), (27.0, 9.0)]);
        let labels = painted
            .iter()
            .filter_map(|node| match node.visual() {
                ui::UiVisual::Text { layout, .. } => {
                    Some((node.bounds().min().y() / gui_scale, layout.line_count()))
                }
                _ => None,
            })
            .collect::<Vec<_>>();
        assert_eq!(labels, [(0.0, 1), (9.0, 2), (27.0, 1)]);
    }
}
