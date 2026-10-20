use mod_ui::PanelUiOwner;
use mod_ui::test_support::{card_dimensions, card_origin};
use ui::mod_panel::{Control, Event};

use super::super::super::snapshot;
use super::mod_hud_editor::{click, panel, point};
use super::mod_widgets::{content, frame, presentation};
use crate::ui_runtime::{UiRuntime, presentation::UiPresentationRuntime};

/// Opens the existing generic editor with incremental persistence explicitly enabled.
fn autosave_editor(p: &mut UiPresentationRuntime) {
    let mut hud = content();
    hud.autosave = true;
    hud.cards[0].position = Some([0.2, 0.2]);
    p.mod_ui_mut()
        .set_mod_panel(PanelUiOwner::Environment(0), Some(&panel()))
        .unwrap();
    p.mod_ui_mut().set_mod_panel_open(true);
    p.mod_ui_mut().open_mod_hud_editor(&hud).unwrap();
    frame(p, &UiRuntime::new(1), [1280, 720], 1.);
}

/// Autosave chrome offers a close action instead of promising to undo completed saves.
#[test]
fn autosave_toolbar_closes_without_exposing_a_cancel_action() {
    let mut p = presentation(false);
    autosave_editor(&mut p);
    let hits = &p
        .mod_ui()
        .mod_hud_editor_state()
        .unwrap()
        .frame
        .unwrap()
        .hits;
    assert!(
        hits.iter()
            .any(|hit| hit.pressed.as_deref() == Some("hud.close"))
    );
    assert!(
        !hits
            .iter()
            .any(|hit| matches!(hit.pressed.as_deref(), Some("hud.cancel" | "hud.save")))
    );
    click(&mut p, "hud.close");
    assert!(!p.mod_ui().mod_hud_editor_open() && !p.mod_ui().mod_panel_open());
}

#[test]
fn completed_drag_autosaves_without_closing_and_interrupted_drag_rolls_back() {
    let mut p = presentation(false);
    autosave_editor(&mut p);
    let at = point(&p, "hud.card:0");
    p.mod_ui_mut().mod_panel_events(at, true, true);
    p.mod_ui_mut()
        .mod_panel_events([at[0] + 140., at[1] + 90.], false, true);
    assert!(
        p.mod_ui_mut().take_mod_hud_editor_result().is_none(),
        "held gestures remain drafts"
    );
    p.mod_ui_mut()
        .mod_panel_events([at[0] + 140., at[1] + 90.], false, false);
    let saved = p.mod_ui_mut().take_mod_hud_editor_result().unwrap();
    assert!(saved.saved);
    assert_ne!(saved.placements[0].position, Some([0.2, 0.2]));
    assert!(p.mod_ui().mod_hud_editor_open() && p.mod_ui().mod_panel_open());

    frame(&mut p, &UiRuntime::new(1), [1280, 720], 1.);
    let at = point(&p, "hud.card:0");
    p.mod_ui_mut().mod_panel_events(at, true, true);
    p.mod_ui_mut()
        .mod_panel_events([at[0] + 120., at[1] + 70.], false, true);
    p.mod_ui_mut().cancel_mod_panel_pointer_input();
    let card = &p.mod_ui().mod_hud_editor_state().unwrap().draft.cards[0];
    assert_eq!(card.position, None);
    assert_eq!(card.anchor, saved.placements[0].anchor);
    assert_eq!(card.offset, saved.placements[0].offset);
    assert!(p.mod_ui_mut().take_mod_hud_editor_result().is_none());
    p.mod_ui_mut().mod_panel_key("Escape", None);
    assert!(!p.mod_ui().mod_hud_editor_open() && !p.mod_ui().mod_panel_open());
    assert_eq!(p.mod_ui_mut().take_mod_hud_editor_result(), Some(saved));
}

#[test]
fn autosave_nudge_and_reset_each_keep_the_editor_live() {
    let mut p = presentation(false);
    autosave_editor(&mut p);
    click(&mut p, "hud.card:0");
    assert!(
        p.mod_ui_mut().take_mod_hud_editor_result().is_none(),
        "selection never moves a card"
    );
    p.mod_ui_mut().mod_panel_key("ArrowRight", None);
    let nudged = p.mod_ui_mut().take_mod_hud_editor_result().unwrap();
    assert!(nudged.saved && !nudged.reset);
    assert!(nudged.placements[0].position.unwrap()[0] > 0.2);
    assert!(p.mod_ui().mod_hud_editor_open());
    click(&mut p, "hud.reset");
    let reset = p.mod_ui_mut().take_mod_hud_editor_result().unwrap();
    assert!(reset.saved && reset.reset);
    assert_eq!(reset.placements[0].anchor, ui::mod_hud::Anchor::TopLeft);
    assert_eq!(reset.placements[0].offset, [6., 6.]);
    assert!(reset.placements[0].position.is_some());
    assert!(p.mod_ui().mod_hud_editor_open());
    p.mod_ui_mut().mod_panel_key("ArrowRight", None);
    assert!(
        !p.mod_ui_mut().take_mod_hud_editor_result().unwrap().reset,
        "subsequent saves do not repeat a completed reset"
    );
}

#[test]
fn reset_before_dismissal_survives_until_consumed_without_sticking_to_later_results() {
    for reset_consumed in [false, true] {
        let mut p = presentation(false);
        autosave_editor(&mut p);
        click(&mut p, "hud.reset");
        if reset_consumed {
            assert!(p.mod_ui_mut().take_mod_hud_editor_result().unwrap().reset);
        }
        p.mod_ui_mut().mod_panel_key("Escape", None);
        let result = p.mod_ui_mut().take_mod_hud_editor_result().unwrap();
        assert!(result.saved);
        assert_eq!(result.reset, !reset_consumed);
        assert_eq!(result.placements[0].anchor, ui::mod_hud::Anchor::TopLeft);
        assert_eq!(result.placements[0].offset, [6., 6.]);
        assert!(result.placements[0].position.is_some());
    }
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
    p.mod_ui_mut()
        .set_mod_panel(PanelUiOwner::Environment(0), Some(&panel))
        .unwrap();
    p.mod_ui_mut().set_mod_panel_open(true);
    p.mod_ui_mut().open_mod_hud_editor(&hud).unwrap();
    frame(&mut p, &UiRuntime::new(1), [1280, 720], 1.);
    let editor = p.mod_ui().mod_hud_editor_state().unwrap();
    assert!(
        !editor
            .frame
            .unwrap()
            .hits
            .iter()
            .any(|hit| hit.pressed.as_deref() == Some("hud.save")),
        "supplied chrome replaces built-in toolbar"
    );
    let invalid = point(&p, "hud.done:1");
    assert!(
        p.mod_ui_mut()
            .mod_panel_events(invalid, true, true)
            .is_empty()
    );
    assert!(
        p.mod_ui().mod_hud_editor_open(),
        "a toggle cannot be forwarded as a button"
    );
    let at = point(&p, "hud.done:0");
    assert_eq!(
        p.mod_ui_mut().mod_panel_events(at, true, true),
        vec![Event {
            id: "continue".into(),
            value: 1.
        }]
    );
    assert!(!p.mod_ui().mod_hud_editor_open() && p.mod_ui().mod_panel_open());
    assert!(p.mod_ui_mut().take_mod_hud_editor_result().unwrap().saved);

    p.mod_ui_mut().open_mod_hud_editor(&hud).unwrap();
    frame(&mut p, &UiRuntime::new(1), [1280, 720], 1.);
    let at = point(&p, "hud.card:0");
    p.mod_ui_mut().mod_panel_events(at, true, true);
    p.mod_ui_mut()
        .mod_panel_events([at[0] + 60., at[1] + 40.], false, false);
    let committed = p.mod_ui_mut().take_mod_hud_editor_result().unwrap();
    frame(&mut p, &UiRuntime::new(1), [1280, 720], 1.);
    let at = point(&p, "hud.card:0");
    p.mod_ui_mut().mod_panel_events(at, true, true);
    p.mod_ui_mut()
        .mod_panel_events([at[0] + 40., at[1] + 20.], false, true);
    let close = point(&p, "hud.close");
    p.mod_ui_mut().mod_panel_events(close, true, true);
    assert!(!p.mod_ui().mod_hud_editor_open() && !p.mod_ui().mod_panel_open());
    let final_result = p.mod_ui_mut().take_mod_hud_editor_result().unwrap();
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
    p.mod_ui_mut().mod_panel_key("ArrowRight", None);
    p.mod_ui_mut().mod_panel_key("Escape", None);
    assert!(!p.mod_ui().mod_hud_editor_open() && !p.mod_ui().mod_panel_open());
    let result = p.mod_ui_mut().take_mod_hud_editor_result().unwrap();
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
        collections: Default::default(),
        previews: Default::default(),
        transitions: Default::default(),
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
    p.mod_ui_mut()
        .set_mod_panel(PanelUiOwner::Environment(0), Some(&panel()))
        .unwrap();
    p.mod_ui_mut().set_mod_panel_open(true);
    p.mod_ui_mut().open_mod_hud_editor(&hud).unwrap();
    frame(&mut p, &UiRuntime::new(1), [1280, 720], 1.);
    let at = point(&p, "hud.grid");
    let pixel = |p: &mut UiPresentationRuntime| {
        let rendered = frame(p, &UiRuntime::new(1), [1280, 720], 1.);
        snapshot::rasterize(&rendered)
            .get_pixel(at[0] as u32, at[1] as u32)
            .0
    };
    assert_eq!(pixel(&mut p), [0, 0, 255, 255]);
    assert!(p.mod_ui_mut().mod_panel_events(at, false, false).is_empty());
    assert_eq!(pixel(&mut p), [0, 255, 0, 255]);
    assert!(p.mod_ui_mut().take_mod_hud_editor_result().is_none());
    assert!(p.mod_ui_mut().mod_panel_events(at, true, true).is_empty());
    assert_eq!(pixel(&mut p), [255, 0, 0, 255]);
    assert!(p.mod_ui().mod_hud_editor_state().unwrap().snap);

    p.mod_ui_mut().mod_panel_events([-50.; 2], false, true);
    assert_eq!(
        pixel(&mut p),
        [255, 0, 0, 255],
        "press remains captured outside"
    );
    p.mod_ui_mut().mod_panel_events([-50.; 2], false, false);
    assert_eq!(pixel(&mut p), [0, 0, 255, 255]);
    p.mod_ui_mut().mod_panel_events(at, false, false);
    assert_eq!(pixel(&mut p), [0, 255, 0, 255]);
    p.mod_ui_mut().mod_panel_events(at, true, true);
    p.mod_ui_mut().cancel_mod_panel_pointer_input();
    assert_eq!(pixel(&mut p), [0, 0, 255, 255]);
    p.mod_ui_mut().mod_panel_events(at, false, false);
    p.mod_ui_mut()
        .mod_panel_events([f32::NAN, at[1]], false, true);
    assert_eq!(pixel(&mut p), [0, 0, 255, 255]);

    let card = point(&p, "hud.card:0");
    p.mod_ui_mut().mod_panel_events(card, true, true);
    p.mod_ui_mut()
        .mod_panel_events([card[0] + 40., card[1] + 30.], false, true);
    assert!(p.mod_ui_mut().take_mod_hud_editor_result().is_none());
    p.mod_ui_mut()
        .mod_panel_events([card[0] + 40., card[1] + 30.], false, false);
    let saved = p.mod_ui_mut().take_mod_hud_editor_result().unwrap();
    assert!(saved.saved && saved.placements[0].position != Some([0.2, 0.2]));
    assert!(p.mod_ui().mod_hud_editor_open());
    pixel(&mut p);
    let card = point(&p, "hud.card:0");
    p.mod_ui_mut().mod_panel_events(card, true, true);
    p.mod_ui_mut()
        .mod_panel_events([card[0] + 40., card[1] + 30.], false, true);
    p.mod_ui_mut().cancel_mod_panel_pointer_input();
    let editor = p.mod_ui().mod_hud_editor_state().unwrap();
    assert_eq!(editor.draft.cards[0].position, None);
    assert_eq!(editor.draft.cards[0].anchor, saved.placements[0].anchor);
    assert_eq!(editor.draft.cards[0].offset, saved.placements[0].offset);
    assert!(editor.view.hovered.is_none() && editor.view.pressed.is_none());
    assert!(p.mod_ui_mut().take_mod_hud_editor_result().is_none());
}

#[test]
fn empty_editor_stays_open_and_closes_without_placements() {
    let mut p = presentation(false);
    let mut hud = content();
    hud.cards.clear();
    assert!(p.mod_ui_mut().open_mod_hud_editor(&hud).is_err());
    p.mod_ui_mut()
        .set_mod_panel(PanelUiOwner::Environment(0), Some(&panel()))
        .unwrap();
    p.mod_ui_mut().set_mod_panel_open(true);
    p.mod_ui_mut().open_mod_hud_editor(&hud).unwrap();
    frame(&mut p, &UiRuntime::new(1), [1280, 720], 1.);
    assert!(p.mod_ui().mod_hud_editor_open());
    p.mod_ui_mut().mod_panel_key("Escape", None);
    assert!(!p.mod_ui().mod_hud_editor_open());
    assert!(
        p.mod_ui_mut()
            .take_mod_hud_editor_result()
            .unwrap()
            .placements
            .is_empty()
    );
}

#[test]
fn dragging_near_viewport_axes_snaps_and_releases_guides() {
    for fraction in [0., 0.5, 1.] {
        let mut p = presentation(false);
        autosave_editor(&mut p);
        let at = point(&p, "hud.card:0");
        p.mod_ui_mut().mod_panel_events(at, true, true);
        let editor = p.mod_ui().mod_hud_editor_state().unwrap();
        let origin = card_origin(&editor.draft.cards[0], editor.viewport);
        let size = card_dimensions(&editor.draft.cards[0]);
        let scale = f64::from(editor.frame.unwrap().scale);
        let destination = std::array::from_fn(|axis| {
            let target = (editor.viewport[axis] - size[axis]).max(0.) * fraction;
            (f64::from(at[axis]) + (target + 3. - origin[axis]) * scale) as f32
        });
        p.mod_ui_mut().mod_panel_events(destination, false, true);
        let editor = p.mod_ui().mod_hud_editor_state().unwrap();
        assert_eq!(editor.draft.cards[0].position, Some([fraction as f32; 2]));
        assert!(editor.drag.unwrap().iter().all(Option::is_some));
        p.mod_ui_mut().mod_panel_events(destination, false, false);
        assert!(p.mod_ui().mod_hud_editor_state().unwrap().drag.is_none());
        assert_eq!(
            p.mod_ui_mut()
                .take_mod_hud_editor_result()
                .unwrap()
                .placements[0]
                .position,
            Some([fraction as f32; 2])
        );
    }
}

#[test]
fn corners_resize_uniformly_autosave_and_restore_interrupted_drafts() {
    for corner in 0..4 {
        let mut p = presentation(false);
        let mut hud = content();
        hud.autosave = true;
        hud.resizable = true;
        hud.cards[0].position = Some([0.5, 0.5]);
        hud.cards[0].reset_scale = Some(1.);
        p.mod_ui_mut()
            .set_mod_panel(PanelUiOwner::Environment(0), Some(&panel()))
            .unwrap();
        p.mod_ui_mut().set_mod_panel_open(true);
        p.mod_ui_mut().open_mod_hud_editor(&hud).unwrap();
        frame(&mut p, &UiRuntime::new(1), [1280, 720], 1.);
        let editor = p.mod_ui().mod_hud_editor_state().unwrap();
        let original = card_origin(&editor.draft.cards[0], editor.viewport);
        let size = card_dimensions(&editor.draft.cards[0]);
        let px = editor.frame.unwrap().scale;
        let fixed: [f64; 2] = std::array::from_fn(|axis| {
            original[axis]
                + if corner & (1 << axis) == 0 {
                    size[axis]
                } else {
                    0.
                }
        });
        let at = point(&p, &format!("hud.resize:0:{corner}"));
        let destination: [f32; 2] = std::array::from_fn(|axis| {
            at[axis]
                + size[axis] as f32 * px * 0.25 * if corner & (1 << axis) == 0 { -1. } else { 1. }
        });
        p.mod_ui_mut().mod_panel_events(at, true, true);
        p.mod_ui_mut().mod_panel_events(destination, false, true);
        assert!(p.mod_ui_mut().take_mod_hud_editor_result().is_none());
        frame(&mut p, &UiRuntime::new(1), [1280, 720], 1.);
        let editor = p.mod_ui().mod_hud_editor_state().unwrap();
        assert!((editor.draft.cards[0].scale - 1.25).abs() < 0.001);
        let origin = card_origin(&editor.draft.cards[0], editor.viewport);
        let resized = card_dimensions(&editor.draft.cards[0]);
        for axis in 0..2 {
            let opposite = origin[axis]
                + if corner & (1 << axis) == 0 {
                    resized[axis]
                } else {
                    0.
                };
            assert!((opposite - fixed[axis]).abs() < 0.001);
        }
        p.mod_ui_mut().mod_panel_events(destination, false, false);
        let saved = p.mod_ui_mut().take_mod_hud_editor_result().unwrap();
        assert!((saved.placements[0].scale - 1.25).abs() < 0.001);
        frame(&mut p, &UiRuntime::new(1), [1280, 720], 1.);
        let at = point(&p, &format!("hud.resize:0:{corner}"));
        p.mod_ui_mut().mod_panel_events(at, true, true);
        p.mod_ui_mut()
            .mod_panel_events([at[0] + 30., at[1] + 20.], false, true);
        p.mod_ui_mut().cancel_mod_panel_pointer_input();
        assert!(
            (p.mod_ui().mod_hud_editor_state().unwrap().draft.cards[0].scale - 1.25).abs() < 0.001
        );
        assert!(p.mod_ui_mut().take_mod_hud_editor_result().is_none());
        frame(&mut p, &UiRuntime::new(1), [1280, 720], 1.);
        click(&mut p, "hud.reset");
        assert_eq!(
            p.mod_ui_mut()
                .take_mod_hud_editor_result()
                .unwrap()
                .placements[0]
                .scale,
            1.
        );
        for direction in [1., -1.] {
            frame(&mut p, &UiRuntime::new(1), [1280, 720], 1.);
            let at = point(&p, &format!("hud.resize:0:{corner}"));
            let destination: [f32; 2] = std::array::from_fn(|axis| {
                at[axis] + direction * 10000. * if corner & (1 << axis) == 0 { -1. } else { 1. }
            });
            p.mod_ui_mut().mod_panel_events(at, true, true);
            p.mod_ui_mut().mod_panel_events(destination, false, false);
            let saved = p.mod_ui_mut().take_mod_hud_editor_result().unwrap();
            let scale = saved.placements[0].scale;
            if direction < 0. {
                assert_eq!(scale, ui::mod_hud::MIN_CARD_SCALE);
            } else {
                assert!(scale > 1. && scale <= ui::mod_hud::MAX_CARD_SCALE);
            }
            let editor = p.mod_ui().mod_hud_editor_state().unwrap();
            let origin = card_origin(&editor.draft.cards[0], editor.viewport);
            let size = card_dimensions(&editor.draft.cards[0]);
            if direction > 0. {
                assert!(
                    scale == ui::mod_hud::MAX_CARD_SCALE
                        || (0..2).any(|axis| {
                            if corner & (1 << axis) == 0 {
                                origin[axis].abs() < 0.001
                            } else {
                                (origin[axis] + size[axis] - editor.viewport[axis]).abs() < 0.001
                            }
                        }),
                    "growth stops at a dragged screen edge or the published maximum"
                );
            }
            for axis in 0..2 {
                assert!(
                    origin[axis] >= 0.
                        && origin[axis] + size[axis] <= editor.viewport[axis] + 0.001
                );
            }
        }
    }
}

#[test]
fn guides_align_both_edges_and_center_above_below_and_beside_center_axes() {
    for axis in 0..2 {
        for fraction in [0., 0.5, 1.] {
            let mut p = presentation(false);
            autosave_editor(&mut p);
            let editor = p.mod_ui().mod_hud_editor_state().unwrap();
            let origin = card_origin(&editor.draft.cards[0], editor.viewport);
            let size = card_dimensions(&editor.draft.cards[0]);
            let guide = editor.viewport[axis] * 0.5;
            let px = editor.frame.unwrap().scale;
            let target = guide - size[axis] * fraction;
            let at = point(&p, "hud.card:0");
            let mut destination = at;
            destination[axis] += (target + 2. - origin[axis]) as f32 * px;
            p.mod_ui_mut().mod_panel_events(at, true, true);
            p.mod_ui_mut().mod_panel_events(destination, false, true);
            let editor = p.mod_ui().mod_hud_editor_state().unwrap();
            assert!(
                (card_origin(&editor.draft.cards[0], editor.viewport)[axis] - target).abs() < 0.001
            );
            assert_eq!(editor.drag.unwrap()[axis], Some(guide));
            p.mod_ui_mut().mod_panel_events(destination, false, false);
            assert!(p.mod_ui_mut().take_mod_hud_editor_result().unwrap().saved);
        }
    }
}

/// Native input and retained painting share snapping feedback without rebuilding the catalog.
#[test]
fn snapping_drag_feedback_retains_catalog_and_shift_bypasses_all_snaps() {
    let mut p = super::mod_hud_anchors::presentation();
    let runtime = UiRuntime::new(1);
    let hud: ui::mod_hud::Hud = serde_json::from_value(serde_json::json!({"autosave":true,"cards":[
        {"id":"fps","font":"sans","editor_label":"Frame rate","width":100,"offset":[40,80],"rows":[{"label":"FPS","value":"144"}]},
        {"id":"clock","font":"sans","editor_label":"Clock","width":120,"offset":[317,220],"rows":[{"label":"Time","value":"12:30"}]}
    ]})).unwrap();
    p.mod_ui_mut()
        .set_mod_panel(PanelUiOwner::Environment(0), Some(&panel()))
        .unwrap();
    p.mod_ui_mut().set_mod_panel_open(true);
    p.mod_ui_mut().open_mod_hud_editor(&hud).unwrap();
    p.mod_ui_mut()
        .set_mod_hud_editor_keep_out(&[[549., 650., 182., 60.]]);
    frame(&mut p, &runtime, [1280, 720], 1.);
    let e = p.mod_ui().mod_hud_editor_state().unwrap();
    let catalog = std::sync::Arc::clone(e.catalog.unwrap());
    let original = card_origin(&e.draft.cards[0], e.viewport);
    let px = e.frame.unwrap().scale;
    let at = point(&p, "hud.card:0");
    let destination = [
        at[0] + ((320. - original[0]) as f32 * px),
        at[1] + ((140. - original[1]) as f32 * px),
    ];
    p.mod_ui_mut().mod_panel_events(at, true, true);
    let before = frame(&mut p, &runtime, [1280, 720], 1.);
    snapshot::write(&before, "hud-snapping-before");
    let e = p.mod_ui().mod_hud_editor_state().unwrap();
    for (axis, distance) in original.into_iter().enumerate() {
        let line = e
            .nodes
            .iter()
            .find(|hit| hit.name == format!("measurement_{axis}"))
            .expect("visible distance line");
        let length = if axis == 0 { line.dest.w } else { line.dest.h };
        assert!(
            (length - distance).abs() < 0.001,
            "measurement {axis} must span the actual gap"
        );
    }
    p.mod_ui_mut().mod_panel_events(destination, false, true);
    let after = frame(&mut p, &runtime, [1280, 720], 1.);
    snapshot::write(&after, "hud-snapping-after");
    let e = p.mod_ui().mod_hud_editor_state().unwrap();
    assert!((card_origin(&e.draft.cards[0], e.viewport)[0] - 317.).abs() < 0.001);
    assert_eq!(e.drag.unwrap()[0], Some(317.));
    assert!(std::sync::Arc::ptr_eq(&catalog, e.catalog.unwrap()));
    // Let hover feedback catch up with the card's newly painted hit bounds.
    p.mod_ui_mut().mod_panel_events(destination, false, true);
    frame(&mut p, &runtime, [1280, 720], 1.);
    let passes = p.mod_ui().mod_hud_editor_state().unwrap().passes;
    // An unchanged captured pointer must not regenerate bindings or rerun layout.
    p.mod_ui_mut().mod_panel_events(destination, false, true);
    frame(&mut p, &runtime, [1280, 720], 1.);
    let e = p.mod_ui().mod_hud_editor_state().unwrap();
    assert_eq!(e.passes, passes);
    assert!(std::sync::Arc::ptr_eq(&catalog, e.catalog.unwrap()));
    p.mod_ui_mut().set_mod_hud_editor_shift(true);
    p.mod_ui_mut().mod_panel_events(destination, false, true);
    let e = p.mod_ui().mod_hud_editor_state().unwrap();
    assert!((card_origin(&e.draft.cards[0], e.viewport)[0] - 320.).abs() < 0.001);
    assert_eq!(e.drag, Some([None; 2]));
    p.mod_ui_mut().set_mod_hud_editor_shift(false);
    p.mod_ui_mut().mod_panel_events(destination, false, true);
    let e = p.mod_ui().mod_hud_editor_state().unwrap();
    assert!((card_origin(&e.draft.cards[0], e.viewport)[0] - 317.).abs() < 0.001);
    p.mod_ui_mut().mod_panel_events(destination, false, false);
    assert!(p.mod_ui_mut().take_mod_hud_editor_result().unwrap().saved);
}
