//! Continuous scale and guest slider gestures through the real pointer boundary.

use super::*;
use ui::mod_panel::Event;

/// Paints one hit region for `action` and returns a mapper from fraction to its pointer position.
fn slider(editor: &mut HudEditor, action: &str) -> impl Fn(f64) -> [f32; 2] + use<> {
    frame(editor, action);
    let rect = editor.frame.as_ref().unwrap().hits[0].rect;
    move |fraction| {
        [
            (rect.x + rect.w * fraction) as f32,
            (rect.y + rect.h * 0.5) as f32,
        ]
    }
}

/// Places the scale slider at `scale` within the published slider range.
fn at_scale(scale: f64) -> f64 {
    let [lo, hi] = ui::mod_hud::EDITOR_SCALE_SLIDER_RANGE.map(f64::from);
    (scale - lo) / (hi - lo)
}

/// Opens a selected card with a retained catalog so invalidation is observable.
fn scale_editor(autosave: bool) -> HudEditor {
    let mut editor = editor(autosave);
    editor.selected = Some(0);
    editor.catalog = Some(Arc::new(
        template::catalog(&editor.draft, editor.viewport).unwrap(),
    ));
    editor
}

#[test]
fn scale_slider_steps_within_range_and_one_gesture_is_one_undo_step() {
    for autosave in [false, true] {
        let mut editor = scale_editor(autosave);
        let placement = (editor.draft.cards[0].anchor, editor.draft.cards[0].offset);
        let at = slider(&mut editor, "hud.scale");
        assert!(
            editor
                .pointer(at(at_scale(1.)), true, true, &mut [])
                .is_empty()
        );
        assert!(editor.drag.is_none() && editor.catalog.is_some());
        // Moving within one 0.05 step neither changes scale nor rebuilds the catalog.
        editor.pointer(at(at_scale(1.02)), false, true, &mut []);
        assert_eq!(editor.draft.cards[0].scale, 1.);
        assert!(editor.catalog.is_some());
        for (target, expected) in [(1.13, 1.15), (0., 0.5), (9., 2.), (1.5, 1.5)] {
            editor.pointer(at(at_scale(target).clamp(0., 1.)), false, true, &mut []);
            assert!((editor.draft.cards[0].scale - expected).abs() < 1e-6);
            assert!(editor.catalog.is_none());
            editor.catalog = Some(Arc::new(
                template::catalog(&editor.draft, editor.viewport).unwrap(),
            ));
        }
        assert_eq!(editor.history.available(), [false, false]);
        editor.pointer(at(at_scale(1.5)), false, false, &mut []);
        assert!(editor.slider.is_none() && editor.drag.is_none());
        assert_eq!(editor.history.available(), [true, false]);
        let card = &editor.draft.cards[0];
        assert_eq!((card.anchor, card.offset), placement);
        if autosave {
            assert_eq!(editor.result.take().unwrap().placements[0].scale, 1.5);
        }
        editor.step_history(false);
        assert_eq!(editor.draft.cards[0].scale, 1.);
        assert_eq!(editor.history.available(), [false, true]);
        editor.step_history(true);
        assert_eq!(editor.draft.cards[0].scale, 1.5);
    }
}

#[test]
fn scale_slider_gesture_ending_at_its_start_records_nothing() {
    for autosave in [false, true] {
        let mut editor = scale_editor(autosave);
        let at = slider(&mut editor, "hud.scale");
        editor.pointer(at(at_scale(1.)), true, true, &mut []);
        editor.pointer(at(at_scale(1.75)), false, true, &mut []);
        editor.pointer(at(at_scale(1.)), false, false, &mut []);
        assert_eq!(editor.draft.cards[0].scale, 1.);
        assert_eq!(editor.history.available(), [false, false]);
        assert!(editor.result.is_none());
    }
}

#[test]
fn scale_slider_cancellation_restores_the_pre_gesture_scale() {
    for cancel in ["pointer", "Escape", "undo", "ownership"] {
        let mut ui = ModUi::default();
        let mut editor = scale_editor(true);
        let at = slider(&mut editor, "hud.scale");
        editor.pointer(at(at_scale(1.)), true, true, &mut []);
        editor.pointer(at(at_scale(1.75)), false, true, &mut []);
        assert_eq!(editor.draft.cards[0].scale, 1.75);
        match cancel {
            "pointer" => editor.cancel_pointer_input(),
            "Escape" => editor.key("Escape"),
            "undo" => editor.step_history(false),
            _ => {
                ui.mod_hud_editor = Some(editor);
                ui.cancel_mod_hud_editor();
                editor = ui.mod_hud_editor.take().unwrap();
            }
        }
        assert_eq!(editor.draft.cards[0].scale, 1., "{cancel}");
        assert!(editor.slider.is_none());
        assert_eq!(editor.history.available(), [false, false]);
        assert!(!editor.result.as_ref().is_some_and(|result| result.saved));
    }
}

#[test]
fn scale_slider_without_a_visible_selection_changes_nothing_and_captures_no_card() {
    for hide in [false, true] {
        let mut editor = scale_editor(false);
        // Hidden cards have no bounds hit, so the slider region is painted first.
        let at = slider(&mut editor, "hud.scale");
        if hide {
            editor.action("hud.visibility:0", &mut []);
        } else {
            editor.selected = None;
        }
        let history = editor.history.available();
        editor.pointer(at(0.9), true, true, &mut []);
        editor.pointer(at(0.1), false, false, &mut []);
        assert_eq!(editor.draft.cards[0].scale, 1.);
        assert!(editor.drag.is_none() && editor.slider.is_none());
        assert_eq!(editor.selected, hide.then_some(0));
        assert_eq!(editor.history.available(), history);
    }
}

#[test]
fn scale_slider_bindings_report_fill_size_and_position() {
    let mut editor = editor(false);
    editor.draft.cards[0].offset = [37.4, 41.6];
    let data = editor.data();
    for (name, value) in [
        ("#hud_selected_scale_fill", Scalar::Num(0.)),
        ("#hud_selected_size_text", Scalar::Text(String::new())),
        ("#hud_selected_position_text", Scalar::Text(String::new())),
    ] {
        assert_eq!(data.global_value(name), Some(&value), "{name}");
    }
    editor.selected = Some(0);
    let at = slider(&mut editor, "hud.scale");
    editor.pointer(at(at_scale(1.5)), true, false, &mut []);
    let height = cards::dimensions(&editor.draft.cards[0])[1].round();
    let data = editor.data();
    assert_eq!(
        data.global_value("#hud_selected_scale_fill"),
        Some(&Scalar::Num(at_scale(1.5)))
    );
    assert_eq!(
        data.global_value("#hud_selected_size_text"),
        Some(&Scalar::Text(format!("150\u{d7}{height}")))
    );
    assert_eq!(
        data.global_value("#hud_selected_position_text"),
        Some(&Scalar::Text("x 37 \u{b7} y 42".into()))
    );
    editor.draft.cards[0].scale = 5.;
    assert_eq!(
        editor.data().global_value("#hud_selected_scale_fill"),
        Some(&Scalar::Num(1.))
    );
}

#[test]
fn guest_slider_emits_stepped_events_and_keeps_the_editor_open() {
    let mut editor = editor(false);
    editor.selected = Some(0);
    let mut controls = crate::test_support::sample_panel().controls;
    let index = controls
        .iter()
        .position(|c| matches!(c, Control::Slider { .. }))
        .unwrap();
    let id = controls[index].id().to_owned();
    let event = |value| {
        vec![Event {
            id: id.clone(),
            value,
        }]
    };
    let at = slider(&mut editor, &format!("hud.control:{index}"));
    assert_eq!(
        editor.pointer(at(0.5), true, true, &mut controls),
        event(50.)
    );
    assert!(
        editor
            .pointer(at(0.501), false, true, &mut controls)
            .is_empty()
    );
    assert_eq!(
        editor.pointer(at(0.75), false, true, &mut controls),
        event(75.)
    );
    assert_eq!(
        editor.pointer(at(2.), false, true, &mut controls),
        event(100.)
    );
    assert!(
        editor
            .pointer(at(2.), false, false, &mut controls)
            .is_empty()
    );
    assert!(
        editor
            .pointer(at(0.), false, false, &mut controls)
            .is_empty()
    );
    assert_eq!(controls[index], {
        let mut expected = crate::test_support::sample_panel().controls[index].clone();
        if let Control::Slider { value, .. } = &mut expected {
            *value = 100.;
        }
        expected
    });
    assert!(editor.open && editor.result.is_none());
    assert!(editor.drag.is_none() && editor.slider.is_none());
    assert_eq!(editor.selected, Some(0));
    assert_eq!(editor.history.available(), [false, false]);
    let fill = format!("#row_{index}_fill");
    assert!(editor.sync_control_fills(&controls));
    assert_eq!(editor.data().global_value(&fill), Some(&Scalar::Num(1.)));
    assert!(!editor.sync_control_fills(&controls));
    // A guest republish moves the fill without a host gesture.
    if let Control::Slider { value, .. } = &mut controls[index] {
        *value = 25.;
    }
    assert!(editor.sync_control_fills(&controls));
    assert_eq!(editor.data().global_value(&fill), Some(&Scalar::Num(0.25)));
}
