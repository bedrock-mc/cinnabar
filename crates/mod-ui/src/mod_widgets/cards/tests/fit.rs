//! Measured previews use production card preparation, text measurement and fragment replay.

use super::*;
use serde_json::Value;

/// Adds authored preview properties without changing the shared production test harness.
fn fitted_panel(properties: Value) -> ui::mod_panel::Panel {
    let mut panel = panel(false);
    let surface = panel.surface.as_mut().unwrap();
    let mut document: Value = serde_json::from_str(&surface.document).unwrap();
    document["screen"]["controls"][0]["stage"]
        .as_object_mut()
        .unwrap()
        .extend(properties.as_object().unwrap().clone());
    surface.document = document.to_string();
    panel
}

/// Admits a single owner card through the same path as guest publications.
fn single_card(card: Value) -> Hud {
    let hud: Hud = serde_json::from_value(json!({"cards":[card]})).unwrap();
    hud.validate().unwrap();
    hud
}

/// Compares physical artwork with its node, including alignment and the binding-axis fill.
fn assert_fit(harness: &Harness, rect: [f32; 4], fit: f32, alignment: [f32; 2]) {
    let bounds = harness.drawn().painted_bounds().unwrap();
    let size = [bounds.width(), bounds.height()];
    let node = [rect[2] - rect[0], rect[3] - rect[1]];
    assert!(((size[0] / node[0]).max(size[1] / node[1]) - fit).abs() < 0.001);
    for (axis, min) in [bounds.min().x(), bounds.min().y()].into_iter().enumerate() {
        let expected = rect[axis] + (node[axis] - size[axis]) * alignment[axis];
        assert!(
            (min - expected).abs() < 0.001,
            "{bounds:?}, expected {expected}"
        );
    }
}

#[test]
fn hud_card_preview_fit_measures_short_long_text_and_live_text_plates() {
    for text_plate in [false, true] {
        let mut card = json!({"id":"card_0","width":240,"background_opacity":0,
            "rows":[{"label":"","value":"0000"}]});
        if text_plate {
            card["text_plate"] = json!({"brackets":true,"background":[0,0,0,0.8]});
        }
        let mut hud = single_card(card);
        let mut ui = installed(&hud, &fitted_panel(json!({"fit":0.78})));
        let mut harness = Harness::new();
        harness.paint(&mut ui, true);
        assert_fit(&harness, [40., 60., 200., 180.], 0.78, [0.5; 2]);
        let source = ui.hud_sources.get_mut(OWNER).unwrap();
        let first = source.cards[0].bounds.unwrap().width();
        let catalog = Arc::clone(&source.catalog);
        hud.cards[0].rows[0].value = "00000000000000000000".into();
        ui.set_mod_hud_sources([(OWNER, 2, &hud)].into_iter())
            .unwrap();
        harness.paint(&mut ui, true);
        assert_fit(&harness, [40., 60., 200., 180.], 0.78, [0.5; 2]);
        let source = ui.hud_sources.get_mut(OWNER).unwrap();
        assert!(source.cards[0].bounds.unwrap().width() > first);
        assert!(Arc::ptr_eq(&catalog, &source.catalog));
    }
}

#[test]
fn hud_card_preview_fit_handles_multi_row_and_oversized_scaled_sources() {
    for scale in [1., ui::mod_hud::MAX_CARD_SCALE] {
        let hud = single_card(json!({"id":"card_0","width":240,"scale":scale,
            "rows":(0..16).map(|_| json!({"label":"Row","value":"123"})).collect::<Vec<_>>()}));
        let mut ui = installed(&hud, &fitted_panel(json!({"fit":0.78})));
        let mut harness = Harness::new();
        harness.paint(&mut ui, true);
        assert_fit(&harness, [40., 60., 200., 180.], 0.78, [0.5; 2]);
        if scale == ui::mod_hud::MAX_CARD_SCALE {
            let source = ui.hud_sources.get_mut(OWNER).unwrap();
            let output = harness.drawn().painted_bounds().unwrap().height();
            assert!(
                output / source.cards[0].bounds.unwrap().height()
                    < ui::mod_panel::hud_card::MIN_SCALE
            );
        }
    }
}

#[test]
fn hud_card_preview_fit_aligns_ink_with_transparent_padding() {
    let hud = single_card(
        json!({"id":"card_0","width":200,"background_opacity":0,"rows":[],
        "cells":[{"rect":[50,25,40,20],"label":"0","background":[1,0,0,1]}]}),
    );
    for alignment in [[0., 0.], [0.5, 0.5], [1., 1.], [0., 1.]] {
        let mut ui = installed(
            &hud,
            &fitted_panel(json!({"fit":0.78,"alignment":alignment})),
        );
        let mut harness = Harness::new();
        harness.paint(&mut ui, true);
        assert_fit(&harness, [40., 60., 200., 180.], 0.78, alignment);
    }
}

#[test]
fn hud_card_preview_fit_caps_tile_and_sheet_instances_independently() {
    let mut panel = fitted_panel(json!({"fit":0.78,"max_scale":2.4}));
    let surface = panel.surface.as_mut().unwrap();
    let mut document: Value = serde_json::from_str(&surface.document).unwrap();
    document["screen"]["controls"]
        .as_array_mut()
        .unwrap()
        .push(json!({"sheet":{
            "type":"custom","renderer":ui::mod_panel::hud_card::RENDERER,"card_id":"card_0",
            "fit":0.78,"max_scale":4,"size":[240,180],"offset":[200,30],
            "anchor_from":"top_left","anchor_to":"top_left"
        }}));
    surface.document = document.to_string();
    let mut ui = installed(&hud(1), &panel);
    let mut harness = Harness::new();
    harness.paint(&mut ui, true);
    let source = ui.hud_sources.get_mut(OWNER).unwrap();
    assert_eq!(
        source.cards[0].paints, 1,
        "both previews share one prepared card"
    );
    let ink = source.cards[0].bounds.unwrap();
    let drawn = harness.drawn();
    for (sheet, cap, width) in [(false, 2.4_f32, 160_f32), (true, 4., 480.)] {
        let vertices: Vec<_> = drawn
            .vertices
            .iter()
            .filter(|vertex| (vertex.position[0] > 300.) == sheet)
            .collect();
        assert!(!vertices.is_empty());
        let min = vertices
            .iter()
            .map(|v| v.position[0])
            .fold(f32::INFINITY, f32::min);
        let max = vertices
            .iter()
            .map(|v| v.position[0])
            .fold(f32::NEG_INFINITY, f32::max);
        let expected = (width * 0.78).min(ink.width() * cap);
        assert!(
            (max - min - expected).abs() < 0.001,
            "{min}..{max}, expected {expected}"
        );
    }
}

#[test]
fn hud_card_preview_fit_bindings_update_parameters_and_reject_invalid_values() {
    let mut panel = fitted_panel(json!({"fit":0.78,"max_scale":2.4,"bindings":[
        {"binding_name":"#selected","binding_name_override":"#card_id"},
        {"binding_name":"#fraction","binding_name_override":"#fit"},
        {"binding_name":"#cap","binding_name_override":"#max_scale"}
    ]}));
    let mut ui = installed(&hud(1), &panel);
    let mut harness = Harness::new();
    for (fraction, cap, visible) in [
        (0.5, 2.4, true),
        (1., 5., true),
        (0., 2.4, false),
        (1.1, 2.4, false),
        (0.78, 0.1, false),
        (0.78, 6., false),
    ] {
        let surface = panel.surface.as_mut().unwrap();
        surface
            .bindings
            .insert("#fraction".into(), SurfaceValue::Number(fraction));
        surface
            .bindings
            .insert("#cap".into(), SurfaceValue::Number(cap));
        ui.set_mod_panel(OWNER, Some(&panel)).unwrap();
        harness.paint(&mut ui, true);
        if visible {
            assert_fit(&harness, [40., 60., 200., 180.], fraction as f32, [0.5; 2]);
        } else {
            assert!(harness.drawn().vertices.is_empty());
        }
    }
}

#[test]
fn hud_card_preview_fit_stable_frames_reuse_preparation_without_allocations() {
    let mut ui = installed(&hud(1), &fitted_panel(json!({"fit":0.78,"max_scale":2.4})));
    let mut harness = Harness::new();
    for _ in 0..4 {
        harness.paint(&mut ui, true);
    }
    let source = ui.hud_sources.get_mut(OWNER).unwrap();
    let shared = Arc::clone(&source.cards[0].nodes);
    let paints = source.cards[0].paints;
    let passes = ui.mod_panel_state().unwrap().passes;
    let (_, allocation) = test_alloc::measure(|| harness.paint(&mut ui, true));
    assert_eq!(allocation.calls, 0, "{allocation:?}");
    let source = ui.hud_sources.get_mut(OWNER).unwrap();
    assert!(Arc::ptr_eq(&shared, &source.cards[0].nodes));
    assert_eq!(source.cards[0].paints, paints);
    assert_eq!(ui.mod_panel_state().unwrap().passes, passes);
    harness.gui_scale = 3;
    harness.paint(&mut ui, true);
    assert_fit(&harness, [40., 60., 200., 180.], 0.78, [0.5; 2]);
}

#[test]
fn hud_card_preview_fit_absent_preserves_manual_scale_and_layout_alignment() {
    let hud = single_card(
        json!({"id":"card_0","width":200,"background_opacity":0,"rows":[],
        "cells":[{"rect":[50,25,40,20],"label":"0","background":[1,0,0,1]}]}),
    );
    let mut harness = Harness::new();
    let mut ui = installed(&hud, &fitted_panel(json!({"scale":0.5})));
    harness.paint(&mut ui, true);
    let before = harness.drawn();
    let source = ui.hud_sources.get_mut(OWNER).unwrap();
    let bounds = source.cards[0].bounds.unwrap();
    let geometry = template::dimensions(&source.content.cards[0]);
    let painted = before.painted_bounds().unwrap();
    assert_eq!(painted.width(), bounds.width() * 0.5);
    assert_eq!(
        painted.min().x(),
        40. + (160. - geometry[0] as f32) * 0.5
            + (bounds.min().x() + source.cards[0].preview_offset[0]) * 0.5
    );
    ui.set_mod_panel(
        OWNER,
        Some(&fitted_panel(json!({"scale":0.5,"max_scale":0.25}))),
    )
    .unwrap();
    harness.paint(&mut ui, true);
    assert_eq!(before.vertices, harness.drawn().vertices);
}
