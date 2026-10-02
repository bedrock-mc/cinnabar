//! Screen cancel mappings survive rendering only a form's content subtree.

use super::*;
use crate::ui_runtime::presentation::forms::pack_harness;

#[test]
fn review_escape_dispatches_the_vanilla_form_screen_cancel() {
    let mut presentation = pack_harness::engine_presentation().expect("real vanilla carrier");
    let mut runtime = pack_harness::action_form("Shop", &["Buy"]);
    presentation
        .build(&runtime, 0, [1280, 720], ui::DpiScale::new(1.0).unwrap())
        .unwrap();
    let identity = runtime.server_forms().active().unwrap().identity;
    let frame = presentation.form_engine_frame(identity).unwrap().clone();
    assert_eq!(frame.cancel_target.as_deref(), Some("button.menu_exit"));
    let events = keyboard(&mut runtime, &frame, KeyCode::Escape, None, false, 0.0);
    assert!(events.iter().any(|event| matches!(event,
        ScreenEvent::Button(button) if mapped_action(&runtime.server_forms().active().unwrap().model, button) == Some(LocalFormAction::Dismiss)
            && button.down && button.interacted
    )), "{events:?}");
}

#[test]
fn screen_cancel_ignores_unmapped_any_events_but_respects_a_consuming_control() {
    let mut presentation = pack_harness::engine_presentation().expect("real vanilla carrier");
    let mut runtime = pack_harness::action_form("Shop", &["Buy"]);
    presentation
        .build(&runtime, 0, [1280, 720], ui::DpiScale::new(1.0).unwrap())
        .unwrap();
    let identity = runtime.server_forms().active().unwrap().identity;
    let mut frame = presentation.form_engine_frame(identity).unwrap().clone();
    let mut region = frame
        .hits
        .iter()
        .find(|region| !region.input.mappings.is_empty())
        .unwrap()
        .clone();
    let mut mapping = region.input.mappings[0].clone();
    region.input.mappings.clear();
    region.input.any = Some(json_ui::MappingScope::Global);
    frame.hits = vec![region].into();
    let events = keyboard(&mut runtime, &frame, KeyCode::Escape, None, false, 0.0);
    assert!(events.iter().any(
        |event| matches!(event, ScreenEvent::Button(button) if button.id == "button.menu_exit")
    ));
    mapping.from = "button.menu_cancel".into();
    mapping.to = "button.dropdown_exit".into();
    mapping.kind = json_ui::MappingType::Global;
    mapping.consume_event = true;
    let region = &mut std::sync::Arc::make_mut(&mut frame.hits)[0];
    region.widget.consume = true;
    region.input.mappings = vec![mapping];
    let events = keyboard(&mut runtime, &frame, KeyCode::Escape, None, false, 1.0);
    assert!(events.iter().any(|event| matches!(event, ScreenEvent::Button(button) if button.id == "button.dropdown_exit" && button.down)));
    assert!(!events.iter().any(
        |event| matches!(event, ScreenEvent::Button(button) if button.id == "button.menu_exit")
    ));
}
