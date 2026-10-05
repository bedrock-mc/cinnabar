use super::super::tests::mini_engine_presentation;
use super::tests::{frame, panel, point};
use super::*;

fn opened() -> UiPresentationRuntime {
    let mut presentation = mini_engine_presentation();
    presentation.set_mod_panel(Some(&panel())).unwrap();
    presentation.set_mod_panel_open(true);
    frame(&mut presentation, [1280, 720]);
    presentation
}

#[test]
fn dropdown_exposes_each_option_and_dismisses_without_changing_value() {
    let mut presentation = opened();
    let choice = point(&presentation, "mod.control:2", 0.5);
    assert!(presentation.mod_panel_events(choice, true, true).is_empty());
    assert!(presentation.mod_panel_editing());
    frame(&mut presentation, [1280, 720]);
    let first = point(&presentation, "mod.option:0", 0.5);
    let second = point(&presentation, "mod.option:1", 0.5);
    assert!(first[1] < second[1]);
    assert!(
        presentation
            .mod_panel_events([0., 0.], true, true)
            .is_empty()
    );
    assert!(!presentation.mod_panel_editing());
    frame(&mut presentation, [1280, 720]);
    let choice = point(&presentation, "mod.control:2", 0.5);
    presentation.mod_panel_events(choice, true, true);
    frame(&mut presentation, [1280, 720]);
    let second = point(&presentation, "mod.option:1", 0.5);
    assert_eq!(
        presentation.mod_panel_events(second, true, true),
        vec![Event {
            id: "mode".into(),
            value: 1.
        }]
    );
    assert!(!presentation.mod_panel_editing());
}

#[test]
fn dropdown_keyboard_navigation_is_explicit_and_escape_cancels() {
    let mut presentation = opened();
    let choice = point(&presentation, "mod.control:2", 0.5);
    presentation.mod_panel_events(choice, true, true);
    assert!(presentation.mod_panel_key("ArrowDown", None).is_empty());
    assert!(presentation.mod_panel_key("Escape", None).is_empty());
    assert!(presentation.mod_panel_open());
    frame(&mut presentation, [1280, 720]);
    let choice = point(&presentation, "mod.control:2", 0.5);
    presentation.mod_panel_events(choice, true, true);
    presentation.mod_panel_key("End", None);
    assert_eq!(
        presentation.mod_panel_key("Enter", None),
        vec![Event {
            id: "mode".into(),
            value: 1.
        }]
    );
}

#[test]
fn number_input_commits_only_valid_bounded_values_and_respects_step() {
    let mut presentation = opened();
    let mut spec = panel();
    if let Control::Slider { step, .. } = &mut spec.controls[1] {
        *step = 0.5;
    }
    presentation.set_mod_panel(Some(&spec)).unwrap();
    frame(&mut presentation, [1280, 720]);
    let value = point(&presentation, "mod.edit:1", 0.5);
    assert!(presentation.mod_panel_events(value, true, true).is_empty());
    presentation.mod_panel_key("Digit9", Some("999"));
    assert!(presentation.mod_panel_key("Enter", None).is_empty());
    assert!(presentation.mod_panel_editing());
    frame(&mut presentation, [1280, 720]);
    presentation.mod_panel_key("SelectAll", None);
    presentation.mod_panel_key("Digit4", Some("42.3"));
    assert_eq!(
        presentation.mod_panel_key("Enter", None),
        vec![Event {
            id: "strength".into(),
            value: 42.5
        }]
    );
    assert!(!presentation.mod_panel_editing());
    frame(&mut presentation, [1280, 720]);
    let value = point(&presentation, "mod.edit:1", 0.5);
    presentation.mod_panel_events(value, true, true);
    presentation.mod_panel_key("Backspace", None);
    assert!(presentation.mod_panel_key("Enter", None).is_empty());
    assert!(presentation.mod_panel_editing());
    presentation.mod_panel_key("Escape", None);
    assert!(presentation.mod_panel_open());
}

#[test]
fn number_input_handles_physical_digits_caret_deletion_and_focus_cancellation() {
    let mut presentation = opened();
    let value = point(&presentation, "mod.edit:1", 0.5);
    presentation.mod_panel_events(value, true, true);
    presentation.mod_panel_key("Digit4", None);
    presentation.mod_panel_key("Numpad2", None);
    presentation.mod_panel_key("Home", None);
    presentation.mod_panel_key("Delete", None);
    assert_eq!(
        presentation.mod_panel_key("NumpadEnter", None),
        vec![Event {
            id: "strength".into(),
            value: 2.
        }]
    );
    frame(&mut presentation, [1280, 720]);
    let value = point(&presentation, "mod.edit:1", 0.5);
    presentation.mod_panel_events(value, true, true);
    presentation.mod_panel_key("Digit8", None);
    presentation.set_mod_panel_open(false);
    assert!(!presentation.mod_panel_editing());
    assert!(presentation.mod_panel_key("Enter", None).is_empty());
    presentation.set_mod_panel_open(true);
    frame(&mut presentation, [1280, 720]);
    assert!(!presentation.mod_panel_editing());
}

#[test]
fn guest_key_capture_prevents_opening_a_competing_text_editor() {
    let mut presentation = opened();
    let mut spec = panel();
    spec.capture_key = true;
    presentation.set_mod_panel(Some(&spec)).unwrap();
    frame(&mut presentation, [1280, 720]);
    let value = point(&presentation, "mod.edit:1", 0.5);
    presentation.mod_panel_events(value, true, true);
    assert!(!presentation.mod_panel_editing());
    let choice = point(&presentation, "mod.control:2", 0.5);
    presentation.mod_panel_events(choice, true, true);
    assert!(!presentation.mod_panel_editing());
}

#[test]
fn number_open_and_enter_preserves_sub_cent_precision() {
    let mut presentation = opened();
    let mut spec = panel();
    if let Control::Slider {
        value, max, step, ..
    } = &mut spec.controls[1]
    {
        *value = 0.123;
        *max = 1.;
        *step = 0.001;
    }
    presentation.set_mod_panel(Some(&spec)).unwrap();
    frame(&mut presentation, [1280, 720]);
    let value = point(&presentation, "mod.edit:1", 0.5);
    presentation.mod_panel_events(value, true, true);
    let events = presentation.mod_panel_key("Enter", None);
    assert_eq!(events.len(), 1);
    assert!((events[0].value - 0.123).abs() < 0.000001);
}

#[test]
fn eight_dropdown_options_remain_inside_small_viewport_and_route_individually() {
    let mut presentation = opened();
    let mut spec = panel();
    spec.controls = vec![Control::Choice {
        id: "mode".into(),
        label: "Mode".into(),
        index: 0,
        options: (0..8).map(|i| format!("Mode {i}")).collect(),
    }];
    presentation.set_mod_panel(Some(&spec)).unwrap();
    frame(&mut presentation, [320, 320]);
    let choice = point(&presentation, "mod.control:0", 0.5);
    presentation.mod_panel_events(choice, true, true);
    frame(&mut presentation, [320, 320]);
    let mut previous = 0.;
    for option in 0..8 {
        let position = point(&presentation, &format!("mod.option:{option}"), 0.5);
        assert!(position[0] >= 0. && position[0] < 320.);
        assert!(position[1] > previous && position[1] < 320.);
        previous = position[1];
    }
    let last = point(&presentation, "mod.option:7", 0.5);
    assert_eq!(
        presentation.mod_panel_events(last, true, true),
        vec![Event {
            id: "mode".into(),
            value: 7.
        }]
    );
}

#[test]
fn long_numeric_draft_keeps_caret_visible_at_both_ends() {
    let text = "123456789012345678901234";
    assert_eq!(edit::visible_draft(text, false, 0, 42.), "|123456");
    assert_eq!(edit::visible_draft(text, false, 24, 42.), "901234|");
}

#[test]
fn same_shape_guest_updates_keep_draft_but_shape_changes_cancel_it() {
    let mut presentation = opened();
    let value = point(&presentation, "mod.edit:1", 0.5);
    presentation.mod_panel_events(value, true, true);
    presentation.mod_panel_key("Digit4", Some("44"));
    let mut spec = panel();
    if let Control::Slider { value, .. } = &mut spec.controls[1] {
        *value = 60.;
    }
    presentation.set_mod_panel(Some(&spec)).unwrap();
    frame(&mut presentation, [1280, 720]);
    assert!(presentation.mod_panel_editing());
    assert_eq!(
        presentation.mod_panel_key("Enter", None),
        vec![Event {
            id: "strength".into(),
            value: 44.
        }]
    );
    assert!(presentation.mod_panel_key("Enter", None).is_empty());
    frame(&mut presentation, [1280, 720]);
    let choice = point(&presentation, "mod.control:2", 0.5);
    presentation.mod_panel_events(choice, true, true);
    presentation.mod_panel_key("End", None);
    presentation.set_mod_panel(Some(&spec)).unwrap();
    frame(&mut presentation, [1280, 720]);
    assert_eq!(
        presentation.mod_panel_key("Enter", None),
        vec![Event {
            id: "mode".into(),
            value: 1.
        }]
    );
    frame(&mut presentation, [1280, 720]);
    let value = point(&presentation, "mod.edit:1", 0.5);
    presentation.mod_panel_events(value, true, true);
    if let Control::Slider { max, .. } = &mut spec.controls[1] {
        *max = 90.;
    }
    presentation.set_mod_panel(Some(&spec)).unwrap();
    assert!(!presentation.mod_panel_editing());
    assert!(presentation.mod_panel_key("Enter", None).is_empty());
}
