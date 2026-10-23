//! Input ownership and presentation lifetime of an authored HUD editor surface.

use super::*;
use crate::PanelUiOwner;
use std::collections::BTreeMap;
use ui::mod_panel::{Surface, SurfaceTransition, SurfaceValue};

/// Opens an empty autosaving editor whose chrome has a deterministic linear fade.
fn editor() -> (ModUi, Hud) {
    let mut ui = ModUi::default();
    ui.set_mod_panel(
        PanelUiOwner::Environment(0),
        Some(&crate::test_support::sample_panel()),
    )
    .unwrap();
    ui.set_mod_panel_open(true);
    let hud = Hud {
        autosave: true,
        surface: Some(Surface {
            collections: Default::default(),
            previews: Default::default(),
            screen: "fixture.root".into(),
            document: serde_json::json!({"namespace":"fixture","root":{
                "type":"screen","size":["100%","100%"],"controls":[]
            }})
            .to_string(),
            bindings: BTreeMap::from([("#alpha".into(), SurfaceValue::Number(1.))]),
            transitions: BTreeMap::from([(
                "#alpha".into(),
                SurfaceTransition {
                    duration: 0.2,
                    easing: serde_json::from_str("\"linear\"").unwrap(),
                    from: Some(SurfaceValue::Number(0.)),
                    exit: Some(SurfaceValue::Number(0.)),
                },
            )]),
        }),
        ..Default::default()
    };
    ui.open_mod_hud_editor(&hud).unwrap();
    advance(&mut ui, 1.);
    advance(&mut ui, 1.3);
    (ui, hud)
}

/// Samples motion exactly as a painted frame does, without a graphics device.
fn advance(ui: &mut ModUi, now: f64) {
    ui.mod_hud_editor
        .as_mut()
        .unwrap()
        .advance_transitions(now, true);
}

/// Reads the actual eased binding consumed by the authored chrome.
fn alpha(ui: &ModUi) -> f64 {
    let mut data = DataSource::new();
    ui.mod_hud_editor
        .as_ref()
        .unwrap()
        .transitions
        .write(&mut data);
    match data.global_value("#alpha").unwrap() {
        Scalar::Num(value) => *value,
        _ => panic!("alpha is numeric"),
    }
}

#[test]
fn hud_editor_exit_releases_input_delivers_once_and_keeps_painting_until_settled() {
    let (mut ui, _) = editor();
    ui.set_mod_panel_open(false);
    assert!(!ui.mod_panel_open() && !ui.mod_hud_editor_open());
    assert!(ui.mod_panel_painting() && ui.mod_hud_editor_painting());
    assert!(ui.mod_panel_motion_active());
    assert!(ui.take_mod_hud_editor_result().is_some());
    assert!(ui.take_mod_hud_editor_result().is_none());
    assert!(ui.mod_panel_events([10., 10.], true, true).is_empty());
    advance(&mut ui, 1.3);
    advance(&mut ui, 1.4);
    assert!((alpha(&ui) - 0.5).abs() < 0.000001);
    assert!(ui.mod_panel_motion_active());
    advance(&mut ui, 1.6);
    assert!(!ui.mod_panel_painting() && !ui.mod_panel_motion_active());
    assert!(ui.take_mod_hud_editor_result().is_none());
}

#[test]
fn hud_editor_reopening_resumes_displayed_alpha_without_reusing_a_draft() {
    let (mut ui, mut preview) = editor();
    ui.set_mod_panel_open(false);
    let _ = ui.take_mod_hud_editor_result();
    advance(&mut ui, 1.3);
    advance(&mut ui, 1.4);
    let displayed = alpha(&ui);
    ui.set_mod_panel_open(true);
    preview.hide_editor_labels = true;
    ui.open_mod_hud_editor(&preview).unwrap();
    advance(&mut ui, 1.4);
    assert!((alpha(&ui) - displayed).abs() < 0.000001);
    assert!(ui.mod_hud_editor.as_ref().unwrap().draft.hide_editor_labels);
    advance(&mut ui, 1.5);
    assert!(alpha(&ui) > displayed && alpha(&ui) < 1.);
    assert!(ui.take_mod_hud_editor_result().is_none());
}

#[test]
fn hud_editor_cancellation_owner_change_and_reduced_motion_remove_an_exit_immediately() {
    for boundary in 0..3 {
        let (mut ui, _) = editor();
        ui.set_mod_panel_open(false);
        let _ = ui.take_mod_hud_editor_result();
        assert!(ui.mod_hud_editor_painting());
        match boundary {
            0 => ui.cancel_mod_hud_editor(),
            1 => ui.configure_motion(false),
            _ => ui
                .set_mod_panel(
                    PanelUiOwner::Environment(1),
                    Some(&crate::test_support::sample_panel()),
                )
                .unwrap(),
        }
        assert!(!ui.mod_hud_editor_painting());
        assert!(!ui.mod_panel_motion_active());
    }
    let (mut ui, _) = editor();
    ui.configure_motion(false);
    ui.set_mod_panel_open(false);
    assert!(!ui.mod_hud_editor_painting());
}

#[test]
fn hud_editor_navigation_finishes_directly_and_autosave_enter_closes_the_underlying_panel() {
    let (mut ui, _) = editor();
    ui.mod_hud_editor.as_mut().unwrap().finish(true);
    assert!(ui.mod_panel_open() && !ui.mod_hud_editor_painting());
    assert!(ui.take_mod_hud_editor_result().unwrap().saved);
    for key in ["Enter", "NumpadEnter"] {
        let (mut ui, _) = editor();
        ui.mod_panel_key(key, None);
        assert!(!ui.mod_panel_open() && !ui.mod_hud_editor_open());
        assert!(ui.mod_hud_editor_painting());
        assert!(ui.take_mod_hud_editor_result().unwrap().saved);
    }
}

#[test]
fn closing_before_first_paint_does_not_save_unmeasured_anchors() {
    for key in ["Escape", "Enter", "NumpadEnter", "panel"] {
        let (mut ui, mut preview) = editor();
        preview.cards = vec![ui::mod_hud::Card {
            id: "untouched".into(),
            position: Some([0.5, 0.5]),
            ..Default::default()
        }];
        ui.open_mod_hud_editor(&preview).unwrap();
        assert_eq!(ui.mod_hud_editor.as_ref().unwrap().viewport, [0.; 2]);
        if key == "panel" {
            ui.set_mod_panel_open(false);
        } else {
            ui.mod_panel_key(key, None);
        }
        assert!(!ui.mod_panel_open() && !ui.mod_hud_editor_open());
        assert_eq!(
            ui.take_mod_hud_editor_result(),
            Some(EditorResult::default()),
            "{key}"
        );
    }
}

/// Empty live observations cannot replace the editor's explicitly authored sample artwork.
#[test]
fn hud_editor_keeps_mock_rows_until_an_explicit_preview_update() {
    use ui::mod_hud::Card;
    let (mut ui, mut hud) = editor();
    hud.cards.push(Card {
        id: "armor".into(),
        sample: true,
        preview_only: true,
        rows: vec![
            serde_json::from_value(
                serde_json::json!({"label":"","value":"", "item":"minecraft:diamond_helmet"}),
            )
            .unwrap(),
        ],
        ..Default::default()
    });
    ui.update_mod_hud_editor(&hud).unwrap();
    let live = Hud {
        cards: vec![Card {
            id: "armor".into(),
            ..Default::default()
        }],
        ..Default::default()
    };
    ui.set_mod_hud_sources(std::iter::once((PanelUiOwner::Environment(0), 1, &live)))
        .unwrap();
    let card = &ui.mod_hud_editor.as_ref().unwrap().draft.cards[0];
    assert!(card.sample);
    assert_eq!(
        card.rows[0].item.as_deref(),
        Some("minecraft:diamond_helmet")
    );
    hud.cards[0].rows[0].item = Some("minecraft:diamond_boots".into());
    ui.update_mod_hud_editor(&hud).unwrap();
    assert_eq!(
        ui.mod_hud_editor.as_ref().unwrap().draft.cards[0].rows[0]
            .item
            .as_deref(),
        Some("minecraft:diamond_boots")
    );
}
