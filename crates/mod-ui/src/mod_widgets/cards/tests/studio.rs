//! Native measurement refresh must respect hidden editor selections.

use super::*;

#[test]
fn studio_hidden_measurements_stay_hidden_after_a_sibling_text_plate_changes() {
    let mut hud: Hud = serde_json::from_value(json!({"cards":[
        {"id":"card_0","width":40,"offset":[40,40],"rows":[],"cells":[{"rect":[0,0,40,20]}]},
        {"id":"plate","offset":[250,100],"rows":[{"label":"","value":"1"}],"text_plate":{}}
    ]}))
    .unwrap();
    hud.surface = Some(Surface {
        screen:"studio.root".into(), document:json!({"namespace":"studio","root":{"type":"screen","size":["100%","100%"],"controls":[{
            "hide":{"type":"button","size":[50,20],"offset":[200,50],"anchor_from":"top_left","anchor_to":"top_left",
                "button_mappings":[{"from_button_id":"button.menu_select","to_button_id":"hud.visibility:0","mapping_type":"pressed"}]}
        }]}}).to_string(), bindings:Default::default(), transitions:Default::default(), collections:Default::default(), previews:Default::default(),
    });
    let mut ui = installed(&hud, &panel(false));
    ui.open_mod_hud_editor(&hud).unwrap();
    let mut harness = Harness::new();
    harness.paint(&mut ui, true);
    studio_click(&mut ui, "hud.card:0");
    harness.paint(&mut ui, true);
    assert!(ui.mod_hud_editor_state().unwrap().nodes.iter().any(
        |node| matches!(&node.draw, json_ui::Draw::Text { text, .. } if text.ends_with(" px"))
    ));
    studio_click(&mut ui, "hud.visibility:0");
    hud.cards[1].rows[0].value = "999999".into();
    ui.set_mod_hud_sources([(OWNER, 2, &hud)].into_iter())
        .unwrap();
    harness.paint(&mut ui, true);
    assert!(!ui.mod_hud_editor_state().unwrap().nodes.iter().any(
        |node| matches!(&node.draw, json_ui::Draw::Text { text, .. } if text.ends_with(" px"))
    ));
}

/// Paints editor animation from an explicit caller clock through the production painter.
fn paint_at(harness: &mut Harness, ui: &mut ModUi, now: f64) {
    harness.paint_with(|engine, frame| {
        frame.now = now;
        let icons = ui.mod_hud_editor_icons(|_, _| None).unwrap();
        ui.paint_mod_hud_editor(Some(engine), frame, &icons);
    });
}

#[test]
fn studio_deselected_arrow_settles_the_animated_card_in_retained_bindings() {
    let mut hud = hud(1);
    hud.surface = Some(Surface {
        screen:"studio.root".into(), document:json!({"namespace":"studio","root":{"type":"screen","size":["100%","100%"],"controls":[{
            "anchor":{"type":"button","size":[50,20],"offset":[100,50],"anchor_from":"top_left","anchor_to":"top_left",
                "button_mappings":[{"from_button_id":"button.menu_select","to_button_id":"hud.anchor:8","mapping_type":"pressed"}]}
        }]}}).to_string(), bindings:Default::default(), transitions:Default::default(), collections:Default::default(), previews:Default::default(),
    });
    let mut ui = installed(&hud, &panel(false));
    ui.open_mod_hud_editor(&hud).unwrap();
    let mut harness = Harness::new();
    paint_at(&mut harness, &mut ui, 0.);
    studio_click(&mut ui, "hud.card:0");
    paint_at(&mut harness, &mut ui, 0.);
    studio_click(&mut ui, "hud.anchor:8");
    paint_at(&mut harness, &mut ui, 0.);
    paint_at(&mut harness, &mut ui, 0.15);
    ui.mod_panel_events([300., 300.], true, false);
    paint_at(&mut harness, &mut ui, 0.15);
    ui.mod_panel_key("ArrowRight", None);
    paint_at(&mut harness, &mut ui, 0.15);
    let state = ui.mod_hud_editor_state().unwrap();
    let target = crate::test_support::card_origin(&state.draft.cards[0], state.viewport);
    let hit = state
        .frame
        .unwrap()
        .hits
        .iter()
        .find(|hit| hit.pressed.as_deref() == Some("hud.card:0"))
        .unwrap();
    assert!((hit.rect.x - target[0]).abs() < 0.001);
    assert!((hit.rect.y - target[1]).abs() < 0.001);
}
