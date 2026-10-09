use super::super::mod_widgets::tests::{content, frame, presentation};
use super::tests::{click, panel, point};
use super::*;
use ui::mod_panel::{Control, Event};

/// Opens the existing generic editor with incremental persistence explicitly enabled.
fn autosave_editor(p: &mut UiPresentationRuntime) {
    let mut hud = content();
    hud.autosave = true;
    hud.cards[0].position = Some([0.2, 0.2]);
    p.set_mod_panel(Some(&panel())).unwrap();
    p.set_mod_panel_open(true);
    p.open_mod_hud_editor(&hud).unwrap();
    frame(p, &UiRuntime::new(1), [1280, 720], 1.);
}

#[test]
fn completed_drag_autosaves_without_closing_and_interrupted_drag_rolls_back() {
    let mut p = presentation(false);
    autosave_editor(&mut p);
    let at = point(&p, "hud.card:0");
    p.mod_panel_events(at, true, true);
    p.mod_panel_events([at[0] + 140., at[1] + 90.], false, true);
    assert!(
        p.take_mod_hud_editor_result().is_none(),
        "held gestures remain drafts"
    );
    p.mod_panel_events([at[0] + 140., at[1] + 90.], false, false);
    let saved = p.take_mod_hud_editor_result().unwrap();
    assert!(saved.saved);
    assert_ne!(saved.placements[0].position, Some([0.2, 0.2]));
    assert!(p.mod_hud_editor_open() && p.mod_panel_open());

    frame(&mut p, &UiRuntime::new(1), [1280, 720], 1.);
    let at = point(&p, "hud.card:0");
    p.mod_panel_events(at, true, true);
    p.mod_panel_events([at[0] + 120., at[1] + 70.], false, true);
    p.cancel_mod_panel_pointer_input();
    assert_eq!(
        p.form_presentation
            .mod_hud_editor
            .as_ref()
            .unwrap()
            .draft
            .cards[0]
            .position,
        saved.placements[0].position
    );
    assert!(p.take_mod_hud_editor_result().is_none());
    p.mod_panel_key("Escape", None);
    assert!(!p.mod_hud_editor_open() && !p.mod_panel_open());
    assert_eq!(
        p.take_mod_hud_editor_result(),
        Some(EditorResult::default())
    );
}

#[test]
fn autosave_nudge_and_reset_each_keep_the_editor_live() {
    let mut p = presentation(false);
    autosave_editor(&mut p);
    click(&mut p, "hud.card:0");
    assert!(
        p.take_mod_hud_editor_result().is_none(),
        "selection never moves a card"
    );
    p.mod_panel_key("ArrowRight", None);
    let nudged = p.take_mod_hud_editor_result().unwrap();
    assert!(nudged.saved && !nudged.reset);
    assert!(nudged.placements[0].position.unwrap()[0] > 0.2);
    assert!(p.mod_hud_editor_open());
    click(&mut p, "hud.reset");
    let reset = p.take_mod_hud_editor_result().unwrap();
    assert!(reset.saved && reset.reset);
    assert_eq!(reset.placements[0].position, None);
    assert!(p.mod_hud_editor_open());
    p.mod_panel_key("ArrowRight", None);
    assert!(
        !p.take_mod_hud_editor_result().unwrap().reset,
        "subsequent saves do not repeat a completed reset"
    );
}

#[test]
fn supplied_editor_chrome_forwards_declared_buttons_and_omits_builtin_toolbar() {
    let mut p = presentation(false);
    let mut panel = panel();
    panel.controls = vec![
        Control::Button {
            id: "continue".into(),
            label: "Continue".into(),
        },
        Control::Toggle {
            id: "enabled".into(),
            label: "Enabled".into(),
            value: false,
        },
    ];
    let mut hud = content();
    hud.autosave = true;
    hud.surface = Some(
        serde_json::from_value(serde_json::json!({
            "screen":"fixture.overlay",
            "document":serde_json::json!({"namespace":"fixture","overlay":{
                "type":"panel","size":["100%","100%"],"controls":[
                    {"invalid":{"type":"button","size":[80,24],"offset":[100,100],
                        "anchor_from":"top_left","anchor_to":"top_left",
                        "button_mappings":[{"from_button_id":"button.menu_select",
                            "to_button_id":"hud.done:1","mapping_type":"pressed"}]}},
                    {"continue":{"type":"button","size":[80,24],"offset":[100,130],
                        "anchor_from":"top_left","anchor_to":"top_left",
                        "button_mappings":[{"from_button_id":"button.menu_select",
                            "to_button_id":"hud.done:0","mapping_type":"pressed"}]}},
                    {"close":{"type":"button","size":[80,24],"offset":[100,160],
                        "anchor_from":"top_left","anchor_to":"top_left",
                        "button_mappings":[{"from_button_id":"button.menu_select",
                            "to_button_id":"hud.close","mapping_type":"pressed"}]}}
                ]
            }}).to_string(),
            "bindings":{}
        }))
        .unwrap(),
    );
    p.set_mod_panel(Some(&panel)).unwrap();
    p.set_mod_panel_open(true);
    p.open_mod_hud_editor(&hud).unwrap();
    frame(&mut p, &UiRuntime::new(1), [1280, 720], 1.);
    let editor = p.form_presentation.mod_hud_editor.as_ref().unwrap();
    assert!(
        !editor
            .frame
            .as_ref()
            .unwrap()
            .hits
            .iter()
            .any(|hit| hit.pressed.as_deref() == Some("hud.save")),
        "supplied chrome replaces built-in toolbar"
    );
    let invalid = point(&p, "hud.done:1");
    assert!(p.mod_panel_events(invalid, true, true).is_empty());
    assert!(
        p.mod_hud_editor_open(),
        "a toggle cannot be forwarded as a button"
    );
    let at = point(&p, "hud.done:0");
    assert_eq!(
        p.mod_panel_events(at, true, true),
        vec![Event {
            id: "continue".into(),
            value: 1.
        }]
    );
    assert!(!p.mod_hud_editor_open() && p.mod_panel_open());
    assert!(p.take_mod_hud_editor_result().unwrap().saved);

    p.open_mod_hud_editor(&hud).unwrap();
    frame(&mut p, &UiRuntime::new(1), [1280, 720], 1.);
    let at = point(&p, "hud.card:0");
    p.mod_panel_events(at, true, true);
    p.mod_panel_events([at[0] + 60., at[1] + 40.], false, false);
    let committed = p.take_mod_hud_editor_result().unwrap();
    frame(&mut p, &UiRuntime::new(1), [1280, 720], 1.);
    let at = point(&p, "hud.card:0");
    p.mod_panel_events(at, true, true);
    p.mod_panel_events([at[0] + 40., at[1] + 20.], false, true);
    let close = point(&p, "hud.close");
    p.mod_panel_events(close, true, true);
    assert!(!p.mod_hud_editor_open() && !p.mod_panel_open());
    let final_result = p.take_mod_hud_editor_result().unwrap();
    assert!(final_result.saved);
    assert_eq!(
        final_result.placements, committed.placements,
        "explicit close preserves completed placements and rolls back unfinished dragging"
    );
}

#[test]
fn completed_autosave_nudge_survives_escape_in_the_same_input_batch() {
    let mut p = presentation(false);
    autosave_editor(&mut p);
    click(&mut p, "hud.card:0");
    p.mod_panel_key("ArrowRight", None);
    p.mod_panel_key("Escape", None);
    assert!(!p.mod_hud_editor_open() && !p.mod_panel_open());
    let result = p.take_mod_hud_editor_result().unwrap();
    assert!(result.saved && !result.reset);
    assert!(
        result.placements[0].position.unwrap()[0] > 0.2,
        "dismissing the editor must deliver an uncollected completed nudge"
    );
}

#[test]
fn supplied_editor_buttons_render_hover_and_captured_press_without_disrupting_autosave() {
    let mut p = presentation(false);
    let mut hud = content();
    hud.autosave = true;
    hud.cards[0].position = Some([0.2, 0.2]);
    let state = |color| {
        serde_json::json!({"type":"custom","renderer":"cinnabar_rounded_rectangle",
            "size":["100%","100%"],"radius":0,"color":color})
    };
    hud.surface = Some(ui::mod_panel::Surface {
        screen: "fixture.overlay".into(),
        document: serde_json::json!({"namespace":"fixture","overlay":{
            "type":"panel","size":["100%","100%"],"controls":[{"grid":{
                "type":"button","size":[70,24],"offset":[230,80],
                "anchor_from":"top_left","anchor_to":"top_left",
                "default_control":"normal","hover_control":"hover","pressed_control":"pressed",
                "button_mappings":[{"from_button_id":"button.menu_select",
                    "to_button_id":"hud.grid","mapping_type":"pressed"}],
                "controls":[{"normal":state([0,0,1,1])},{"hover":state([0,1,0,1])},
                    {"pressed":state([1,0,0,1])}]
            }}]
        }})
        .to_string(),
        bindings: Default::default(),
    });
    p.set_mod_panel(Some(&panel())).unwrap();
    p.set_mod_panel_open(true);
    p.open_mod_hud_editor(&hud).unwrap();
    frame(&mut p, &UiRuntime::new(1), [1280, 720], 1.);
    let at = point(&p, "hud.grid");
    let pixel = |p: &mut UiPresentationRuntime| {
        let rendered = frame(p, &UiRuntime::new(1), [1280, 720], 1.);
        super::super::snapshot::rasterize(&rendered)
            .get_pixel(at[0] as u32, at[1] as u32)
            .0
    };
    assert_eq!(pixel(&mut p), [0, 0, 255, 255]);
    assert!(p.mod_panel_events(at, false, false).is_empty());
    assert_eq!(pixel(&mut p), [0, 255, 0, 255]);
    assert!(p.take_mod_hud_editor_result().is_none());
    assert!(p.mod_panel_events(at, true, true).is_empty());
    assert_eq!(pixel(&mut p), [255, 0, 0, 255]);
    assert!(p.form_presentation.mod_hud_editor.as_ref().unwrap().snap);

    p.mod_panel_events([-50.; 2], false, true);
    assert_eq!(
        pixel(&mut p),
        [255, 0, 0, 255],
        "press remains captured outside"
    );
    p.mod_panel_events([-50.; 2], false, false);
    assert_eq!(pixel(&mut p), [0, 0, 255, 255]);
    p.mod_panel_events(at, false, false);
    assert_eq!(pixel(&mut p), [0, 255, 0, 255]);
    p.mod_panel_events(at, true, true);
    p.cancel_mod_panel_pointer_input();
    assert_eq!(pixel(&mut p), [0, 0, 255, 255]);
    p.mod_panel_events(at, false, false);
    p.mod_panel_events([f32::NAN, at[1]], false, true);
    assert_eq!(pixel(&mut p), [0, 0, 255, 255]);

    let card = point(&p, "hud.card:0");
    p.mod_panel_events(card, true, true);
    p.mod_panel_events([card[0] + 40., card[1] + 30.], false, true);
    assert!(p.take_mod_hud_editor_result().is_none());
    p.mod_panel_events([card[0] + 40., card[1] + 30.], false, false);
    let saved = p.take_mod_hud_editor_result().unwrap();
    assert!(saved.saved && saved.placements[0].position != Some([0.2, 0.2]));
    assert!(p.mod_hud_editor_open());
    pixel(&mut p);
    let card = point(&p, "hud.card:0");
    p.mod_panel_events(card, true, true);
    p.mod_panel_events([card[0] + 40., card[1] + 30.], false, true);
    p.cancel_mod_panel_pointer_input();
    let editor = p.form_presentation.mod_hud_editor.as_ref().unwrap();
    assert_eq!(editor.draft.cards[0].position, saved.placements[0].position);
    assert!(editor.view.hovered.is_none() && editor.view.pressed.is_none());
    assert!(p.take_mod_hud_editor_result().is_none());
}
