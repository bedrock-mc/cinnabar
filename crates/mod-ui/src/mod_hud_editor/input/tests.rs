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

/// Covers negative and zero travel on either axis, both axes, and before the first frame.
fn collapsed_viewports(card: &Card) -> [[f64; 2]; 7] {
    let [width, height] = cards::dimensions(card);
    [
        [width * 0.75, height + 100.],
        [width, height + 100.],
        [width + 100., height * 0.5],
        [width + 100., height],
        [width * 0.75, height * 0.5],
        [width, height],
        [0., 0.],
    ]
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
            for (resolved, expected) in cards::origin(&legacy, editor.viewport).into_iter().zip(at)
            {
                assert!((resolved - expected).abs() < 0.001);
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
fn snapshot_preserves_explicit_positions_with_collapsed_travel() {
    let mut editor = editor(false);
    let card = &mut editor.draft.cards[0];
    card.width = 280.;
    card.scale = 2.;
    card.position = Some([0.5, 0.75]);
    let authored = card.clone();
    for viewport in collapsed_viewports(&authored) {
        editor.viewport = viewport;
        let result = editor.snapshot(true);
        result.validate().unwrap();
        let placement = &result.placements[0];
        assert_eq!(
            placement.position, authored.position,
            "viewport {viewport:?}"
        );
        assert_eq!(editor.draft.cards[0], authored);
        let restored = Card {
            position: placement.position,
            ..authored.clone()
        };
        assert_eq!(
            cards::origin(&restored, [1200., 900.]),
            cards::origin(&authored, [1200., 900.])
        );
    }
}

#[test]
fn snapshot_preserves_clamped_anchors_and_reset_offsets() {
    let mut editor = editor(false);
    for anchor in [Anchor::TopLeft, Anchor::Center, Anchor::BottomRight] {
        for offset in [[37., 41.], [-37., -41.]] {
            let card = &mut editor.draft.cards[0];
            card.width = 280.;
            card.scale = 2.;
            card.anchor = anchor;
            card.offset = offset;
            card.position = None;
            let authored = card.clone();
            for viewport in collapsed_viewports(&authored) {
                editor.viewport = viewport;
                let result = editor.snapshot(true);
                let placement = &result.placements[0];
                assert_eq!(placement.anchor, anchor);
                assert_eq!(placement.offset, offset);
                assert_eq!(editor.draft.cards[0], authored);
                let restored = Card {
                    anchor: placement.anchor,
                    offset: placement.offset,
                    ..authored.clone()
                };
                assert_eq!(
                    cards::origin(&restored, [1200., 900.]),
                    cards::origin(&authored, [1200., 900.])
                );
            }
            let card = &mut editor.draft.cards[0];
            card.position = Some([0.5, 0.75]);
            card.reset_anchor = Some(anchor);
            card.reset_offset = Some(offset);
            card.reset_scale = Some(2.);
            editor.reset_layout();
            let result = editor.snapshot(true);
            assert!(result.reset);
            assert_eq!(result.placements[0].anchor, anchor);
            assert_eq!(result.placements[0].offset, offset);
            assert_eq!(editor.draft.cards[0].position, None);
        }
    }
}

#[test]
fn snapshot_preserves_untouched_cards_during_autosave_and_finish() {
    let mut editor = editor(true);
    editor.viewport = [400., 4.];
    let card = &mut editor.draft.cards[0];
    card.width = 280.;
    card.scale = 2.;
    card.position = Some([0.5, 0.75]);
    editor.draft.cards.extend([
        Card {
            id: "anchored".into(),
            anchor: Anchor::BottomRight,
            offset: [37., 41.],
            ..Default::default()
        },
        Card {
            id: "moved".into(),
            ..Default::default()
        },
    ]);
    editor.committed = editor.draft.clone();
    editor.move_card(2, [100., 0.]);
    editor.anchor_card(2);
    editor.checkpoint();
    let checkpoint = editor.result.take().unwrap();
    editor.finish(true);
    for result in [checkpoint, editor.result.take().unwrap()] {
        result.validate().unwrap();
        assert_eq!(result.placements[0].position, Some([0.5, 0.75]));
        assert_eq!(result.placements[1].anchor, Anchor::BottomRight);
        assert_eq!(result.placements[1].offset, [37., 41.]);
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
    for (actual, expected) in editor.draft.cards[0].offset.into_iter().zip([4000., 2200.]) {
        assert!((actual - expected).abs() < 0.001);
    }
}

#[test]
fn anchored_reset_restores_captured_fallbacks_and_explicit_guest_defaults() {
    let mut editor = editor(true);
    editor.move_card(0, [650., 450.]);
    editor.anchor_card(0);
    editor.checkpoint();
    editor.result.take();
    editor.reset_layout();
    let result = editor.result.take().unwrap();
    assert!(result.saved && result.reset);
    assert_eq!(result.placements[0].anchor, Anchor::TopLeft);
    assert_eq!(result.placements[0].offset, [0.; 2]);
    let card = &mut editor.draft.cards[0];
    card.reset_anchor = Some(Anchor::Right);
    card.reset_offset = Some([13., -17.]);
    card.reset_scale = Some(0.75);
    editor.reset_layout();
    let card = &editor.draft.cards[0];
    assert_eq!(card.anchor, Anchor::Right);
    assert_eq!(card.offset, [13., -17.]);
    assert_eq!(card.scale, 0.75);
    assert_eq!(card.position, None);
    editor.result.take().unwrap().validate().unwrap();
}
