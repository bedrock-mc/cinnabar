//! Studio actions, history and inspector feedback use the real input boundary.

use super::*;
use ui::mod_panel::{Control, FieldModifiers};

mod amendment;
mod slider;

struct Art;
impl json_ui::TextMeasure for Art {
    /// Geometry tests need no glyph metrics.
    fn extent(&self, _: &str) -> [f64; 2] {
        [0.; 2]
    }
}
impl json_ui::TextureSource for Art {
    /// Geometry tests have no external artwork.
    fn texture(&self, _: &str) -> Option<json_ui::TextureMeta> {
        None
    }
}

/// Builds actual scaled card hit regions and retains one action for a focused input test.
fn frame(editor: &mut HudEditor, action: &str) -> [f32; 2] {
    let catalog = template::catalog(&editor.draft, editor.viewport).unwrap();
    let root = json_ui::resolve(&catalog, SCREEN, &Context::default())
        .control
        .unwrap();
    let bound = json_ui::bind(&root, &editor.data(), &json_ui::EmptyLibrary);
    let (laid, report) = json_ui::layout_with(
        &bound,
        editor.viewport,
        &json_ui::LayoutEnv {
            text: &Art,
            textures: &Art,
        },
        &editor.view,
    );
    let mut hit = json_ui::hit_regions(&laid)
        .into_iter()
        .find(|hit| hit.pressed.as_deref() == Some("hud.card:0"))
        .unwrap();
    let point = [
        (hit.rect.x + hit.rect.w * 0.5) as f32,
        (hit.rect.y + hit.rect.h * 0.5) as f32,
    ];
    hit.pressed = Some(action.into());
    editor.frame = Some(EngineFrame {
        hits: vec![hit].into(),
        report,
        cancel_target: None,
        origin: [0.; 2],
        scale: 1.,
        panel: None,
        edit_texts: Vec::new(),
        top: Vec::new(),
    });
    point
}

/// Applies a Studio action through a painted hit's pressed route.
fn press(
    editor: &mut HudEditor,
    action: &str,
    controls: &mut [Control],
) -> Vec<ui::mod_panel::Event> {
    let point = frame(editor, action);
    editor.pointer(point, true, true, controls)
}

#[test]
fn studio_history_covers_move_resize_nudge_anchor_scale_visibility_reset_and_redo() {
    for autosave in [false, true] {
        let mut editor = editor(autosave);
        editor.draft.resizable = true;
        let mut states = vec![editor.snapshot(true)];
        // One complete pointer gesture is one step, regardless of intermediate samples.
        let point = frame(&mut editor, "hud.card:0");
        editor.pointer(point, true, true, &mut []);
        for delta in [20., 40., 60.] {
            editor.pointer([point[0] + delta, point[1] + delta], false, true, &mut []);
        }
        editor.pointer([point[0] + 60., point[1] + 60.], false, false, &mut []);
        states.push(editor.snapshot(true));
        let point = frame(&mut editor, "hud.resize:0:3");
        editor.pointer(point, true, true, &mut []);
        editor.pointer([point[0] + 20., point[1] + 20.], false, false, &mut []);
        states.push(editor.snapshot(true));
        editor.key("ArrowRight");
        states.push(editor.snapshot(true));
        for action in [
            "hud.anchor:8",
            "hud.scale_up",
            "hud.visibility:0",
            "hud.reset",
        ] {
            // Hidden cards have no screen hits, but remain actionable from the guest's drawer.
            if action == "hud.reset" {
                editor.reset_layout();
            } else {
                editor.action(action, &mut []).unwrap();
            }
            states.push(editor.snapshot(true));
        }
        for pair in states.windows(2) {
            assert!(
                pair[0].placements != pair[1].placements
                    || pair[0].hidden_cards != pair[1].hidden_cards,
                "every tested action must create an edit"
            );
        }
        for expected in states.iter().rev().skip(1) {
            editor.step_history(false);
            let actual = editor.snapshot(true);
            assert_eq!(actual.placements, expected.placements);
            assert_eq!(actual.hidden_cards, expected.hidden_cards);
        }
        assert_eq!(editor.history.available(), [false, true]);
        for expected in states.iter().skip(1) {
            editor.step_history(true);
            let actual = editor.snapshot(true);
            assert_eq!(actual.placements, expected.placements);
            assert_eq!(actual.hidden_cards, expected.hidden_cards);
        }
        assert_eq!(editor.history.available(), [true, false]);
        if autosave {
            assert!(editor.result.take().unwrap().saved);
        }
    }
}

#[test]
fn studio_history_truncates_old_steps_and_branches_only_on_a_real_edit() {
    let mut editor = editor(false);
    editor.selected = Some(0);
    for _ in 0..100 {
        editor.key("ArrowRight");
    }
    let end = cards::origin(&editor.draft.cards[0], editor.viewport);
    let mut steps = 0;
    while editor.history.available()[0] {
        editor.step_history(false);
        steps += 1;
    }
    assert!((50..100).contains(&steps));
    assert!(
        (end[0] - cards::origin(&editor.draft.cards[0], editor.viewport)[0] - f64::from(steps))
            .abs()
            < 0.01
    );
    editor.action("hud.select:0", &mut []);
    assert!(editor.history.available()[1]);
    editor.key("ArrowDown");
    assert_eq!(editor.history.available(), [true, false]);
    let before = editor.snapshot(true);
    editor.step_history(true);
    assert_eq!(editor.snapshot(true), before);
}

#[test]
fn studio_keyboard_shortcuts_keep_the_physical_modifier_snapshot() {
    let mut ui = ModUi::default();
    let mut editor = editor(false);
    editor.selected = Some(0);
    editor.key("ArrowRight");
    let moved = editor.draft.cards[0].clone();
    ui.mod_hud_editor = Some(editor);
    for command in [false, true] {
        let modifiers = FieldModifiers {
            control: !command,
            super_key: command,
            ..Default::default()
        };
        ui.mod_panel_field_key("KeyZ", None, FieldModifiers::default());
        assert_eq!(ui.mod_hud_editor.as_ref().unwrap().draft.cards[0], moved);
        ui.mod_panel_field_key("KeyZ", None, modifiers);
        assert_ne!(ui.mod_hud_editor.as_ref().unwrap().draft.cards[0], moved);
        ui.mod_panel_field_key(
            "KeyZ",
            None,
            FieldModifiers {
                shift: true,
                ..modifiers
            },
        );
        assert_eq!(ui.mod_hud_editor.as_ref().unwrap().draft.cards[0], moved);
    }
    ui.mod_panel_field_key(
        "KeyZ",
        None,
        FieldModifiers {
            control: true,
            ..Default::default()
        },
    );
    ui.mod_panel_field_key(
        "KeyY",
        None,
        FieldModifiers {
            control: true,
            ..Default::default()
        },
    );
    assert_eq!(ui.mod_hud_editor.as_ref().unwrap().draft.cards[0], moved);
}

#[test]
fn studio_hide_result_and_bindings_keep_the_card_listed_and_clear_the_toast_by_now() {
    let mut editor = editor(false);
    assert_eq!(
        editor.data().global_value("#hud_selected"),
        Some(&Scalar::Num(-1.))
    );
    editor.action("hud.select:0", &mut []);
    editor.action("hud.scale_up", &mut []);
    let data = editor.data();
    assert_eq!(data.global_value("#hud_selected"), Some(&Scalar::Num(0.)));
    assert_eq!(
        data.global_value("#hud_selected_scale_text"),
        Some(&Scalar::Text("105%".into()))
    );
    assert_eq!(
        data.global_value("#hud_selected_anchor"),
        Some(&Scalar::Num(0.))
    );
    assert_eq!(
        data.global_value("#hud_selected_w"),
        Some(&Scalar::Num(cards::dimensions(&editor.draft.cards[0])[0]))
    );
    let point = frame(&mut editor, "hud.card:0");
    editor.pointer(point, true, true, &mut []);
    assert_eq!(
        editor.data().global_value("#hud_dragging"),
        Some(&Scalar::Bool(true))
    );
    editor.pointer(point, false, false, &mut []);
    assert_eq!(
        editor.data().global_value("#hud_dragging"),
        Some(&Scalar::Bool(false))
    );
    editor.action("hud.visibility:0", &mut []);
    let data = editor.data();
    assert_eq!(
        data.global_value("#hud_card_0_visible"),
        Some(&Scalar::Bool(false))
    );
    assert_eq!(
        data.global_value("#hud_card_0_name"),
        Some(&Scalar::Text("fixture".into()))
    );
    assert_eq!(
        data.global_value("#hud_toast"),
        Some(&Scalar::Text("fixture hidden".into()))
    );
    assert_eq!(editor.snapshot(true).hidden_cards, ["fixture"]);
    editor.advance_feedback(100., true);
    editor.advance_feedback(103.99, true);
    assert!(!editor.toast.text.is_empty());
    editor.advance_feedback(104., true);
    assert!(editor.toast.text.is_empty());
    editor.action("hud.visibility:0", &mut []);
    editor.action("hud.visibility:0", &mut []);
    editor.step_history(false);
    assert!(editor.toast.text.is_empty());
    assert!(editor.hidden.is_empty());
    editor.finish(false);
    assert_eq!(editor.result, Some(EditorResult::default()));
}

/// Resetting the layout restores placement without re-enabling hidden modules.
#[test]
fn studio_reset_keeps_hidden_cards_hidden() {
    let mut editor = editor(true);
    editor.action("hud.visibility:0", &mut []).unwrap();
    editor.reset_layout();
    assert!(editor.hidden.contains("fixture"));
    assert_eq!(editor.snapshot(true).hidden_cards, ["fixture"]);
    assert_eq!(
        editor.data().global_value("#hud_card_0_visible"),
        Some(&Scalar::Bool(false))
    );
}

#[test]
fn studio_anchor_picker_uses_all_nine_margins_and_caller_clock_easing() {
    for animate in [true, false] {
        let mut editor = editor(false);
        editor.selected = Some(0);
        for (index, anchor) in ui::mod_hud::Anchor::ALL.into_iter().enumerate() {
            let from = editor.card_origin(0);
            editor.action(&format!("hud.anchor:{index}"), &mut []);
            let target = anchor.origin(
                anchor.editor_offset(snapping::GRID as f32),
                cards::dimensions(&editor.draft.cards[0]),
                editor.viewport,
            );
            editor.advance_feedback(10., animate);
            assert_eq!(editor.card_origin(0), if animate { from } else { target });
            editor.advance_feedback(10.15, animate);
            let at = editor.card_origin(0);
            for axis in 0..2 {
                assert!(
                    (at[axis]
                        - (if animate {
                            from[axis] + (target[axis] - from[axis]) * 0.875
                        } else {
                            target[axis]
                        }))
                    .abs()
                        < 0.001
                );
            }
            assert_eq!(
                editor.data().global_value("#hud_selected_x"),
                Some(&Scalar::Num(at[0]))
            );
            editor.advance_feedback(10.31, animate);
            assert_eq!(editor.card_origin(0), target);
            assert_eq!(
                editor.data().global_value("#hud_selected_anchor"),
                Some(&Scalar::Num(index as f64))
            );
        }
    }
}

#[test]
fn studio_control_fires_button_and_toggle_without_finishing_or_ending_history() {
    let mut editor = editor(false);
    let mut controls = crate::test_support::sample_panel().controls;
    let button = controls
        .iter()
        .position(|c| matches!(c, Control::Button { .. }))
        .unwrap();
    let toggle = controls
        .iter()
        .position(|c| matches!(c, Control::Toggle { .. }))
        .unwrap();
    editor.selected = Some(0);
    editor.key("ArrowRight");
    for (index, expected) in [(button, 1.), (toggle, 1.), (toggle, 0.)] {
        let events = press(&mut editor, &format!("hud.control:{index}"), &mut controls);
        assert_eq!(
            events,
            [ui::mod_panel::Event {
                id: controls[index].id().into(),
                value: expected
            }]
        );
        assert!(editor.open && editor.result.is_none());
        assert_eq!(editor.selected, Some(0));
        assert_eq!(editor.history.available(), [true, false]);
    }
    let mut published = editor.draft.cards.clone();
    published[0].title = "Fresh content".into();
    published[0].offset = [77.; 2];
    let placement = editor.snapshot(true).placements;
    editor.republish_cards(&published, true);
    assert_eq!(editor.snapshot(true).placements, placement);
    assert_eq!(editor.selected, Some(0));
    editor.step_history(false);
    assert_eq!(editor.draft.cards[0].title, "Fresh content");
    editor.step_history(true);
    assert_eq!(editor.snapshot(true).placements, placement);
}

#[test]
fn studio_scale_actions_clamp_without_creating_noop_history() {
    let mut editor = editor(false);
    editor.selected = Some(0);
    for _ in 0..100 {
        editor.action("hud.scale_down", &mut []);
    }
    assert_eq!(editor.draft.cards[0].scale, ui::mod_hud::MIN_CARD_SCALE);
    editor.step_history(false);
    assert!(editor.draft.cards[0].scale > ui::mod_hud::MIN_CARD_SCALE);
    for _ in 0..200 {
        editor.action("hud.scale_up", &mut []);
    }
    assert_eq!(editor.draft.cards[0].scale, ui::mod_hud::MAX_CARD_SCALE);
    editor.step_history(false);
    assert!(editor.draft.cards[0].scale < ui::mod_hud::MAX_CARD_SCALE);
}

#[test]
fn studio_hidden_selection_accepts_inspector_edits_and_history_bindings_follow_steps() {
    let mut editor = editor(false);
    editor.action("hud.visibility:0", &mut []);
    editor.action("hud.select:0", &mut []);
    editor.action("hud.scale_up", &mut []);
    editor.action("hud.anchor:6", &mut []);
    assert_eq!(editor.draft.cards[0].anchor, Anchor::BottomLeft);
    assert!((editor.draft.cards[0].scale - 1.05).abs() < 0.0001);
    assert_eq!(editor.snapshot(true).hidden_cards, ["fixture"]);
    assert_eq!(
        editor.data().global_value("#hud_can_undo"),
        Some(&Scalar::Bool(true))
    );
    assert_eq!(
        editor.data().global_value("#hud_can_redo"),
        Some(&Scalar::Bool(false))
    );
    editor.step_history(false);
    assert_eq!(
        editor.data().global_value("#hud_can_redo"),
        Some(&Scalar::Bool(true))
    );
    assert_eq!(editor.draft.cards[0].anchor, Anchor::TopLeft);
    editor.advance_feedback(1., false);
    editor.data_dirty = false;
    let (_, allocation) = test_alloc::measure(|| editor.advance_feedback(2., false));
    assert_eq!(allocation.calls, 0);
    assert!(!editor.data_dirty);
}

#[test]
fn studio_full_preview_reorders_by_id_and_cancelling_a_new_card_drag_keeps_fresh_content() {
    let mut editor = editor(true);
    editor.selected = Some(0);
    editor.key("ArrowRight");
    let mut preview = editor.draft.clone();
    preview.cards.insert(
        0,
        Card {
            id: "new".into(),
            ..Default::default()
        },
    );
    preview.cards[1].title = "Fresh".into();
    editor.republish_cards(&preview.cards, true);
    assert_eq!(editor.selected, Some(1));
    editor.step_history(false);
    assert_eq!(editor.draft.cards[1].offset, [0.; 2]);
    assert_eq!(editor.draft.cards[1].title, "Fresh");
    let point = frame(&mut editor, "hud.card:0");
    editor.pointer(point, true, true, &mut []);
    editor.pointer([point[0] + 20., point[1] + 20.], false, true, &mut []);
    editor.cancel_pointer_input();
    assert_eq!(editor.draft.cards[0].id, "new");
    assert_eq!(editor.draft.cards[0].offset, preview.cards[0].offset);
}

#[test]
fn studio_undo_cancels_an_unfinished_first_gesture_in_both_save_modes() {
    for autosave in [false, true] {
        let mut editor = editor(autosave);
        let original = editor.draft.cards[0].clone();
        let point = frame(&mut editor, "hud.card:0");
        editor.pointer(point, true, true, &mut []);
        editor.pointer([point[0] + 20., point[1] + 20.], false, true, &mut []);
        assert_ne!(editor.draft.cards[0], original);
        editor.step_history(false);
        assert_eq!(editor.draft.cards[0], original);
        assert!(editor.drag.is_none());
        assert_eq!(editor.history.available(), [false, false]);
        assert!(editor.result.is_none());
    }
}

#[test]
fn studio_republish_keeps_capture_and_the_completed_history_baseline() {
    for reorder in [false, true] {
        let mut editor = editor(true);
        let original = editor.snapshot(true).placements[0].clone();
        let point = frame(&mut editor, "hud.card:0");
        editor.pointer(point, true, true, &mut []);
        editor.pointer([point[0] + 20., point[1] + 20.], false, true, &mut []);
        let mut published = editor.draft.cards.clone();
        published[0].background_opacity = 0.4;
        if reorder {
            published.insert(
                0,
                Card {
                    id: "new".into(),
                    ..Default::default()
                },
            );
        }
        editor.republish_cards(&published, true);
        let index = usize::from(reorder);
        assert!(
            editor.drag.is_some(),
            "content-only publication retains pointer capture"
        );
        assert_eq!(editor.selected, Some(index));
        editor.pointer([point[0] + 40., point[1] + 40.], false, false, &mut []);
        assert_ne!(editor.snapshot(true).placements[index], original);
        assert!(editor.result.take().unwrap().saved);
        editor.step_history(false);
        assert_eq!(editor.snapshot(true).placements[index], original);
        assert_eq!(editor.draft.cards[index].background_opacity, 0.4);
        if reorder {
            assert_eq!(editor.draft.cards[0].offset, [0.; 2]);
        }
    }
}

#[test]
fn studio_republish_added_cards_remain_undoable_after_restoring_an_older_layout() {
    let mut editor = editor(false);
    editor.selected = Some(0);
    editor.key("ArrowRight");
    let mut published = editor.draft.cards.clone();
    published.push(Card {
        id: "new".into(),
        ..Default::default()
    });
    editor.republish_cards(&published, true);
    editor.step_history(false);
    editor.action("hud.select:1", &mut []);
    editor.action("hud.scale_up", &mut []);
    editor.step_history(false);
    assert_eq!(editor.draft.cards[1].scale, 1.);
    editor.step_history(true);
    assert!((editor.draft.cards[1].scale - 1.05).abs() < 0.0001);
    editor.action("hud.visibility:1", &mut []);
    editor.step_history(false);
    assert!(!editor.hidden.contains("new"));
}

#[test]
fn studio_republish_keeps_anchor_motion_running() {
    let mut editor = editor(false);
    editor.selected = Some(0);
    editor.action("hud.anchor:8", &mut []);
    editor.advance_feedback(10., true);
    let from = editor.card_origin(0);
    let mut published = editor.draft.cards.clone();
    published[0].background_opacity = 0.4;
    editor.republish_cards(&published, true);
    let target = cards::origin(&editor.draft.cards[0], editor.viewport);
    editor.advance_feedback(10.15, true);
    let at = editor.card_origin(0);
    for axis in 0..2 {
        assert!((at[axis] - (from[axis] + (target[axis] - from[axis]) * 0.875)).abs() < 0.001);
    }
}

#[test]
fn studio_native_empty_preview_keeps_its_placeholder_across_gameplay_publication() {
    let mut editor = editor(false);
    editor.draft.cards[0].title = "Armor".into();
    editor.draft.cards[0].editor_label = "Armor".into();
    editor.draft.cards[0].source = Some(ui::mod_hud::RowSource {
        kind: ui::mod_hud::NativeRows::Armor {
            held: false,
            percent: true,
            empty: false,
        },
        limit: 4,
        color: [1.; 4],
        brackets: false,
    });
    let mut published = editor.draft.cards.clone();
    published[0].title.clear();
    published[0].source = None;
    editor.republish_cards(&published, false);
    assert_eq!(editor.draft.cards[0].title, "Armor");
    assert!(cards::dimensions(&editor.draft.cards[0])[1] > 4.);
}

#[test]
fn studio_pressing_an_animated_card_captures_its_displayed_origin_for_move_and_resize() {
    for action in ["hud.card:0", "hud.resize:0:3"] {
        let mut editor = editor(true);
        editor.draft.resizable = true;
        editor.selected = Some(0);
        editor.action("hud.anchor:8", &mut []);
        editor.advance_feedback(10., true);
        editor.advance_feedback(10.15, true);
        let displayed = editor.card_origin(0);
        let point = frame(&mut editor, action);
        editor.pointer(point, true, true, &mut []);
        let captured = cards::origin(&editor.draft.cards[0], editor.viewport);
        for axis in 0..2 {
            assert!((captured[axis] - displayed[axis]).abs() < 0.001);
        }
        editor.pointer([point[0] + 10., point[1] + 10.], false, false, &mut []);
        let moved = cards::origin(&editor.draft.cards[0], editor.viewport);
        for axis in 0..2 {
            let expected = displayed[axis] + if action == "hud.card:0" { 10. } else { 0. };
            assert!((moved[axis] - expected).abs() < 0.01);
        }
        let saved = editor.result.take().unwrap();
        assert!(saved.saved);
        saved.validate().unwrap();
    }
}

#[test]
fn studio_pointer_cancellation_rolls_back_unfinished_gestures_in_both_save_modes() {
    for autosave in [false, true] {
        let mut editor = editor(autosave);
        let original = editor.draft.cards[0].clone();
        let point = frame(&mut editor, "hud.card:0");
        editor.pointer(point, true, true, &mut []);
        editor.pointer([point[0] + 20., point[1] + 20.], false, true, &mut []);
        editor.cancel_pointer_input();
        assert_eq!(editor.draft.cards[0], original);
        assert_eq!(editor.history.available(), [false, false]);
        editor.step_history(false);
        assert_eq!(editor.draft.cards[0], original);
    }
}

#[test]
fn studio_reset_metadata_round_trips_through_undo_redo_and_save() {
    for move_first in [false, true] {
        let mut editor = editor(false);
        editor.selected = Some(0);
        if move_first {
            editor.key("ArrowRight");
        }
        editor.reset_layout();
        assert!(editor.snapshot(true).reset);
        editor.step_history(false);
        assert!(!editor.snapshot(true).reset);
        editor.step_history(true);
        editor.finish(true);
        assert!(editor.result.take().unwrap().reset);
    }
}

#[test]
fn studio_reset_metadata_autosave_undo_replaces_unread_reset_intent() {
    let mut editor = editor(true);
    editor.selected = Some(0);
    editor.key("ArrowRight");
    editor.reset_layout();
    assert!(editor.result.as_ref().unwrap().reset);
    editor.step_history(false);
    assert!(!editor.result.as_ref().unwrap().reset);
    editor.step_history(true);
    assert!(editor.result.as_ref().unwrap().reset);
    editor.step_history(false);
    editor.key("ArrowDown");
    let restored = editor.result.take().unwrap();
    assert!(restored.saved && !restored.reset);
    restored.validate().unwrap();
    assert!(restored.replace_pending_reset);
    editor.key("ArrowRight");
    assert!(!editor.result.take().unwrap().replace_pending_reset);
}
