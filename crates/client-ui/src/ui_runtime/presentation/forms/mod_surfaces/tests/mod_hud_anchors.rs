//! Window-free placement captures using original text and the shipped open font.

use super::super::super::ServerUiPack;
use super::super::super::snapshot;
use super::mod_hud_editor::{panel, point};
use super::mod_widgets::frame;
use crate::ui_runtime::{UiRuntime, presentation::UiPresentationRuntime};
use mod_ui::{
    PanelUiOwner,
    test_support::{card_dimensions, card_origin},
};
use std::sync::Arc;
use ui::mod_hud::{Anchor, Hud};

/// Uses the shipped open font for every label without requiring game carriers.
pub(super) fn presentation() -> UiPresentationRuntime {
    let fonts = mod_ui::fonts::install(ui_engine::test_support::fixture_font()).unwrap();
    let font = Arc::new(fonts.font_named(ui::mod_panel::SANS_FONT_NAME).clone());
    let mut p = UiPresentationRuntime::new(font).unwrap();
    p.enable_json_ui(crate::test_support::mini_carrier())
        .unwrap();
    p.set_server_ui_pack(&ServerUiPack {
        ui_layers: vec![vec![
            ("ui/_ui_defs.json".into(), br#"{"ui_defs":["ui/hud_screen.json"]}"#.to_vec()),
            ("ui/hud_screen.json".into(), br#"{"namespace":"hud","hud_screen":{"type":"screen","controls":[{"caption":{"type":"label","text":"HUD layout","size":[100,12],"anchor_from":"bottom_middle","anchor_to":"bottom_middle"}}]}}"#.to_vec()),
        ]],
        ..Default::default()
    });
    p.set_gui_scale_preference(Some(1));
    p
}

#[test]
fn anchored_layout_resize_snapshot_preserves_edge_distance_after_a_real_drop() {
    let mut p = presentation();
    let runtime = UiRuntime::new(1);
    let hud: Hud = serde_json::from_value(serde_json::json!({"cards":[{
        "id":"frame_rate", "editor_label":"Frame rate", "font":"sans",
        "width":100, "offset":[12,40], "rows":[{"label":"FPS","value":"144"}]
    }]}))
    .unwrap();
    p.mod_ui_mut()
        .set_mod_panel(PanelUiOwner::Environment(0), Some(&panel()))
        .unwrap();
    p.mod_ui_mut().set_mod_panel_open(true);
    p.mod_ui_mut().open_mod_hud_editor(&hud).unwrap();
    frame(&mut p, &runtime, [1280, 720], 1.);
    let e = p.mod_ui().mod_hud_editor_state().unwrap();
    let size = card_dimensions(&e.draft.cards[0]);
    let original = card_origin(&e.draft.cards[0], e.viewport);
    let target = [e.viewport[0] - size[0] - 32., 40.];
    let px = e.frame.unwrap().scale;
    let at = point(&p, "hud.card:0");
    let destination =
        std::array::from_fn(|axis| at[axis] + ((target[axis] - original[axis]) as f32 * px));
    p.mod_ui_mut().mod_panel_events(at, true, true);
    p.mod_ui_mut().mod_panel_events(destination, false, false);
    let mut anchored = p.mod_ui().mod_hud_editor_state().unwrap().draft.clone();
    assert_eq!(anchored.cards[0].anchor, Anchor::TopRight);
    for (actual, expected) in anchored.cards[0].offset.into_iter().zip([32., 40.]) {
        assert!((actual - expected).abs() < 0.001);
    }
    p.mod_ui_mut().mod_panel_key("Enter", None);
    let saved = p.mod_ui_mut().take_mod_hud_editor_result().unwrap();
    p.mod_ui_mut().set_mod_panel_open(false);
    let mut legacy = anchored.clone();
    legacy.cards[0].position = saved.placements[0].position;
    p.set_mod_hud(Some(&legacy)).unwrap();
    let before = frame(&mut p, &runtime, [1920, 1080], 1.);
    snapshot::write(&before, "hud-anchors-before");
    let state = p.mod_ui().mod_widgets_state().unwrap();
    let origin = card_origin(&legacy.cards[0], state.viewport);
    assert!((state.viewport[0] - origin[0] - size[0] - 32.).abs() > 1.);
    anchored.cards[0].position = None;
    p.set_mod_hud(Some(&anchored)).unwrap();
    let after = frame(&mut p, &runtime, [1920, 1080], 1.);
    snapshot::write(&after, "hud-anchors-after");
    let state = p.mod_ui().mod_widgets_state().unwrap();
    let origin = card_origin(&anchored.cards[0], state.viewport);
    assert!((state.viewport[0] - origin[0] - size[0] - 32.).abs() < 0.001);
    assert_ne!(snapshot::rasterize(&before), snapshot::rasterize(&after));
}
