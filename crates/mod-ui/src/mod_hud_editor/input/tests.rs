//! Completed placement and rollback behavior without a renderer or local carriers.

use super::*;
use ui::mod_hud::{Anchor, Card};

/// Opens one small card in a known GUI viewport through the public editor entry point.
fn editor(autosave: bool) -> HudEditor {
    let mut ui = ModUi::default();
    ui.set_mod_panel(
        crate::PanelUiOwner::Environment(0),
        Some(&crate::test_support::sample_panel()),
    )
    .unwrap();
    ui.set_mod_panel_open(true);
    ui.open_mod_hud_editor(&Hud {
        autosave,
        cards: vec![Card {
            id: "fixture".into(),
            width: 100.,
            ..Default::default()
        }],
        ..Default::default()
    })
    .unwrap();
    let mut editor = ui.mod_hud_editor.take().unwrap();
    editor.viewport = [900., 600.];
    editor
}

#[test]
fn anchored_completed_moves_and_nudges_keep_their_origin_and_legacy_results() {
    let mut editor = editor(true);
    for y in 0..3 {
        for x in 0..3 {
            let target = [63.25 + f64::from(x) * 300., 41.75 + f64::from(y) * 200.];
            editor.move_card(0, target);
            editor.anchor_card(0);
            assert_eq!(editor.draft.cards[0].position, None);
            let at = cards::origin(&editor.draft.cards[0], editor.viewport);
            for axis in 0..2 {
                assert!((at[axis] - target[axis]).abs() < 0.001);
            }
            editor.checkpoint();
            let result = editor.result.take().unwrap();
            result.validate().unwrap();
            let p = &result.placements[0];
            let restored = Card {
                anchor: p.anchor,
                offset: p.offset,
                scale: p.scale,
                ..editor.draft.cards[0].clone()
            };
            assert_eq!(cards::origin(&restored, editor.viewport), at);
            let legacy = Card {
                position: p.position,
                ..restored
            };
            for axis in 0..2 {
                assert!((cards::origin(&legacy, editor.viewport)[axis] - at[axis]).abs() < 0.001);
            }
        }
    }
    editor.selected = Some(0);
    let before = cards::origin(&editor.draft.cards[0], editor.viewport);
    editor.key("ArrowLeft");
    assert_eq!(editor.draft.cards[0].position, None);
    let after = cards::origin(&editor.draft.cards[0], editor.viewport);
    assert!((before[0] - after[0] - 1.).abs() < 0.001);
    assert!(editor.result.take().unwrap().saved);
}

#[test]
fn anchored_resize_retains_opposite_corner_and_saves_current_normalized_position() {
    let mut editor = editor(true);
    editor.move_card(0, [650., 400.]);
    editor.anchor_card(0);
    let size = cards::dimensions(&editor.draft.cards[0]);
    let fixed = [650. + size[0], 400. + size[1]];
    editor.resize_card(
        0,
        Resize {
            corner: 0,
            scale: 1.,
            size,
            fixed,
        },
        [-50., -2.],
    );
    let before = cards::origin(&editor.draft.cards[0], editor.viewport);
    editor.anchor_card(0);
    assert_eq!(editor.draft.cards[0].anchor, Anchor::BottomRight);
    let after = cards::origin(&editor.draft.cards[0], editor.viewport);
    let size = cards::dimensions(&editor.draft.cards[0]);
    for axis in 0..2 {
        assert!((after[axis] - before[axis]).abs() < 0.001);
        assert!((after[axis] + size[axis] - fixed[axis]).abs() < 0.001);
    }
    editor.checkpoint();
    let placement = &editor.result.as_ref().unwrap().placements[0];
    assert_eq!(placement.anchor, Anchor::BottomRight);
    assert!(placement.scale > 1.);
    assert!(placement.position.is_some());
}

#[test]
fn anchored_snapshot_converts_untouched_legacy_input_without_mutating_the_draft() {
    let mut editor = editor(false);
    editor.draft.cards[0].position = Some([0.9, 0.8]);
    let before = editor.draft.clone();
    let result = editor.snapshot(true);
    assert_eq!(editor.draft, before);
    let p = &result.placements[0];
    assert_eq!(p.anchor, Anchor::BottomRight);
    assert_eq!(p.position, Some([0.9, 0.8]));
    let card = Card {
        anchor: p.anchor,
        offset: p.offset,
        position: None,
        ..before.cards[0].clone()
    };
    for axis in 0..2 {
        assert!(
            (cards::origin(&card, editor.viewport)[axis]
                - cards::origin(&before.cards[0], editor.viewport)[axis])
                .abs()
                < 0.001
        );
    }
}

#[test]
fn anchored_wide_viewport_placements_remain_valid_guest_input() {
    let mut editor = editor(false);
    editor.viewport = [16000., 9000.];
    editor.move_card(0, [4000., 2200.]);
    editor.anchor_card(0);
    let result = editor.snapshot(true);
    result.validate().unwrap();
    editor.draft.validate().unwrap();
    assert_eq!(editor.draft.cards[0].offset, [4000., 2200.]);
}
