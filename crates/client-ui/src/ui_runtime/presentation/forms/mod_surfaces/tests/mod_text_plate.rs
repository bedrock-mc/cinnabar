//! Text plates use the live font and exactly the same bounds during gameplay and editing.

use super::mod_widgets::{frame, presentation};
use crate::ui_runtime::UiRuntime;
use mod_ui::{PanelUiOwner, test_support::card_dimensions};
use ui::mod_hud::Hud;

/// Creates a fitting plate with colors distinct from the ordinary gameplay HUD.
fn content() -> Hud {
    serde_json::from_str(r#"{"resizable":true,"cards":[{"id":"clock","anchor":"top_right","offset":[-8,8],"rows":[{"label":"","value":"12:05 AM","color":[1,0,0,1]}],"text_plate":{"brackets":true,"bracket_color":[0,1,0,1],"background":[0,0,1,1],"border_color":[1,1,0,1],"border_width":0.5}}]}"#).unwrap()
}

/// Editor hit regions and drag dimensions follow native glyph measurements at every tested DPI.
#[test]
fn text_plate_editor_bounds_match_gameplay_font_measurements() {
    for (size, dpi) in [([800, 500], 1.), ([1920, 1080], 1.5), ([2560, 1440], 2.)] {
        let mut p = presentation(false);
        let hud = content();
        p.set_mod_hud(Some(&hud)).unwrap();
        let runtime = UiRuntime::new(1);
        frame(&mut p, &runtime, size, dpi);
        let card = &p.mod_ui().mod_widgets_state().unwrap().content.cards[0];
        assert!(card.text_plate.as_ref().unwrap().layout.is_some());
        let expected = card_dimensions(card);
        let panel = mod_ui::test_support::sample_panel();
        p.mod_ui_mut()
            .set_mod_panel(PanelUiOwner::Environment(0), Some(&panel))
            .unwrap();
        p.mod_ui_mut().set_mod_panel_open(true);
        p.mod_ui_mut().open_mod_hud_editor(&hud).unwrap();
        frame(&mut p, &runtime, size, dpi);
        let editor = p.mod_ui().mod_hud_editor_state().unwrap();
        let hit = editor
            .frame
            .unwrap()
            .hits
            .iter()
            .find(|hit| hit.pressed.as_deref() == Some("hud.card:0"))
            .unwrap();
        assert_eq!([hit.rect.w, hit.rect.h], expected);
        assert_eq!(card_dimensions(&editor.draft.cards[0]), expected);
    }
}

/// Changing text resizes its background and repositions a right-anchored plate without rebuilding its template.
#[test]
fn text_plate_updates_retain_catalog_and_resize_with_real_fonts() {
    let mut p = presentation(false);
    let runtime = UiRuntime::new(1);
    let mut hud = content();
    p.set_mod_hud(Some(&hud)).unwrap();
    let before = frame(&mut p, &runtime, [800, 500], 1.);
    let state = p.mod_ui().mod_widgets_state().unwrap();
    let catalog = std::sync::Arc::clone(state.catalog);
    let width = card_dimensions(&state.content.cards[0])[0];
    p.set_mod_hud(Some(&hud)).unwrap();
    assert!(
        p.mod_ui().mod_widgets_state().unwrap().content.cards[0]
            .text_plate
            .as_ref()
            .unwrap()
            .layout
            .is_some(),
        "authored equality must preserve native measurements"
    );
    hud.cards[0].rows[0].value = "1:05".into();
    p.set_mod_hud(Some(&hud)).unwrap();
    let after = frame(&mut p, &runtime, [800, 500], 1.);
    let state = p.mod_ui().mod_widgets_state().unwrap();
    assert!(std::sync::Arc::ptr_eq(&catalog, state.catalog));
    assert!(card_dimensions(&state.content.cards[0])[0] < width);
    assert!(
        super::super::super::snapshot::rasterize(&before)
            != super::super::super::snapshot::rasterize(&after)
    );
}

/// Complete glyph runs and a visible fill must reach paint, rather than merely measured bounds.
#[test]
fn text_plate_paints_full_values_and_measured_backgrounds() {
    for (size, dpi) in [([800, 500], 1.), ([1920, 1080], 1.5), ([2560, 1440], 2.)] {
        let mut p = presentation(false);
        let runtime = UiRuntime::new(1);
        let hud = content();
        p.set_mod_hud(Some(&hud)).unwrap();
        frame(&mut p, &runtime, size, dpi);
        let nodes = &p.last_frame.as_ref().unwrap().nodes;
        let value = nodes
            .iter()
            .find_map(|node| match node.visual() {
                ui::UiVisual::Text {
                    color: [255, 0, 0, 255],
                    layout,
                    ..
                } => Some(layout),
                _ => None,
            })
            .expect("painted text plate value");
        assert!(
            !value.ellipsized(),
            "measured values must fit their paint box"
        );
        for (index, character) in "12:05 AM"
            .char_indices()
            .filter(|(_, c)| !c.is_whitespace())
        {
            assert!(
                value.glyph_source_indices().contains(&Some(index)),
                "missing glyph {character} at byte {index}: {:?}",
                value.glyph_source_indices()
            );
        }
        assert!(
            nodes.iter().any(|node| match node.visual() {
                ui::UiVisual::Mesh(mesh) =>
                    mesh.vertices()
                        .iter()
                        .any(|vertex| vertex.color == [0, 0, 255, 255])
                        && node.bounds().max().x() - node.bounds().min().x() > 10.,
                _ => false,
            }),
            "measured plate fill must occupy more than a placeholder pixel"
        );
    }
}
