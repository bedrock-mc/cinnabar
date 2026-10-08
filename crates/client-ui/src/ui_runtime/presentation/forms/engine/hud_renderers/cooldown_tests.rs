use super::*;
use crate::test_support::{fixture_font, mini_carrier};
use crate::ui_runtime::presentation::forms::engine::{
    EngineInputs, EngineOutput, FormEngine, ScreenArt,
};
use crate::ui_runtime::presentation::{FONT_DESIGN_PIXEL_TEXELS, TextMetrics};
use json_ui::{Catalog, Context, ViewState};
use ui::{DpiScale, SafeArea, TextLayoutCache, UiNode};

fn draw(progress: f32, index: usize, opacity: f32) -> (Vec<UiNode>, f32) {
    let mut catalog = Catalog::default();
    let (namespace, name) = json_ui::CROSSHAIR_SCREEN.split_once('.').unwrap();
    catalog.overlay_text(
        "ui/cooldown.json",
        &serde_json::json!({
            "namespace": namespace,
            (name): {"type": "screen", "controls": [{
                "hotbar": {"type": "grid", "collection_name": "hotbar_items",
                    "grid_dimensions": [index + 1, 1],
                    "size": [20 * (index + 1), 22],
                    "anchor_from": "top_left", "anchor_to": "top_left",
                    "grid_item_template": format!("{namespace}.cooldown")}
            }]},
            "cooldown": {
                    "type": "custom", "renderer": "hotbar_cooldown_renderer",
                    "anchor_from": "top_left", "anchor_to": "top_left",
                    "size": [20, 22], "alpha": opacity,
                    "bindings": [{"binding_type": "collection_details",
                        "binding_collection_name": "hotbar_items"}]
            }
        })
        .to_string(),
    );
    let engine = FormEngine::new(mini_carrier(), catalog, 2);
    let font = fixture_font();
    let metrics = TextMetrics::for_viewport([1280, 720], DpiScale::new(1.0).unwrap(), None);
    let px = metrics.scale.get() * FONT_DESIGN_PIXEL_TEXELS as f32;
    let mut hud = HudPaint::default();
    hud.hotbar_cooldowns[1] = progress;
    let mut layouts = TextLayoutCache::new(32, 1024 * 1024);
    let mut nodes = Vec::new();
    let mut next = 1;
    engine
        .render_screen(
            json_ui::CROSSHAIR_SCREEN,
            &json_ui::hud_data_source(&json_ui::HudModel {
                hotbar: vec![json_ui::HudSlot::default(); index + 1],
                ..Default::default()
            }),
            &Context::default(),
            &ViewState::default(),
            ScreenArt {
                hud: Some(&hud),
                ..Default::default()
            },
            EngineInputs {
                layouts: &mut layouts,
                font: &font,
                metrics,
                solid_page: 0,
                safe_area: SafeArea::ZERO,
                content: [1280.0, 720.0],
                translate: &|_| None,
                language: [0; 3],
            },
            EngineOutput {
                nodes: &mut nodes,
                next: &mut next,
                overlay: &[],
            },
        )
        .unwrap()
        .expect("cooldown screen resolves");
    (nodes, px)
}

#[test]
fn only_the_cooling_slot_draws_and_expired_or_invalid_progress_is_empty() {
    for (progress, slot) in [(0.5, 0), (0.0, 1), (-1.0, 1), (f32::NAN, 1)] {
        let (nodes, _) = draw(progress, slot, 1.0);
        assert!(
            !nodes
                .iter()
                .any(|node| matches!(node.visual(), UiVisual::Solid { .. }))
        );
    }
    let (nodes, _) = draw(0.5, 1, 1.0);
    let solids: Vec<_> = nodes
        .iter()
        .filter(|node| matches!(node.visual(), UiVisual::Solid { .. }))
        .collect();
    assert_eq!(solids.len(), 1);
    assert!(matches!(
        solids[0].visual(),
        UiVisual::Solid {
            color: [255, 255, 255, 127],
            ..
        }
    ));
}

#[test]
fn cooldown_inherits_the_hud_opacity() {
    let (nodes, _) = draw(0.5, 1, 0.5);
    assert!(nodes.iter().any(|node| matches!(
        node.visual(),
        UiVisual::Solid {
            color: [255, 255, 255, 64],
            ..
        }
    )));
}
