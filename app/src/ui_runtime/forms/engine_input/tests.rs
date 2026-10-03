//! Screen cancel mappings survive rendering only a form's content subtree.

use super::*;
use crate::ui_runtime::presentation::{
    UiPresentationRuntime,
    forms::{ServerUiPack, pack_harness, tests::mini_engine_presentation},
};

/// Supplies a synthetic screen-level cancel mapping above the rendered content subtree.
/// The inherited screen and form buttons exercise the real JSON-UI presentation without assets.
fn cancel_presentation() -> UiPresentationRuntime {
    let mut presentation = mini_engine_presentation();
    presentation.set_server_ui_pack(&ServerUiPack {
        ui_layers: vec![vec![(
            "ui/server_form.json".into(),
            br#"{
                "namespace": "server_form",
                "cancel_screen": {
                    "type": "screen",
                    "button_mappings": [{
                        "from_button_id": "button.menu_cancel",
                        "to_button_id": "button.menu_exit",
                        "mapping_type": "global"
                    }]
                },
                "third_party_server_screen@server_form.cancel_screen": {
                    "$screen_content": "server_form.main_screen_content"
                }
            }"#
            .to_vec(),
        )]],
        ..Default::default()
    });
    presentation
}

#[test]
fn review_escape_dispatches_the_vanilla_form_screen_cancel() {
    let mut player_runtime = crate::player_runtime::PlayerRuntime::new(1);

    let mut presentation = cancel_presentation();
    let mut runtime = pack_harness::action_form(&mut player_runtime, "Shop", &["Buy"]);
    presentation
        .build(
            &player_runtime,
            &runtime,
            0,
            [1280, 720],
            ui::DpiScale::new(1.0).unwrap(),
        )
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
    let mut player_runtime = crate::player_runtime::PlayerRuntime::new(1);

    let mut presentation = cancel_presentation();
    let mut runtime = pack_harness::action_form(&mut player_runtime, "Shop", &["Buy"]);
    presentation
        .build(
            &player_runtime,
            &runtime,
            0,
            [1280, 720],
            ui::DpiScale::new(1.0).unwrap(),
        )
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

#[test]
fn idle_input_relays_pending_animation_end_events() {
    let anims = std::sync::Arc::new(json_ui::ControlAnims {
        key: "finished".into(),
        graph: json_ui::AnimGraph {
            heads: vec![0],
            nodes: vec![json_ui::AnimNode {
                kind: json_ui::AnimKind::Wait,
                duration: 0.01,
                easing: json_ui::Easing::Linear,
                from: [0.0; 4],
                to: [0.0; 4],
                from_expr: serde_json::Value::Null,
                to_expr: serde_json::Value::Null,
                play_event: None,
                reset_event: None,
                end_event: Some("button.menu_exit".into()),
                destroy_at_end: None,
                wait_until_rendered: false,
                resettable: false,
                scale_from_starting_alpha: false,
                fps: 1.0,
                frame_count: 1,
                reversible: false,
                vertical: false,
                looping: false,
                next: None,
            }],
        },
        rest_alpha: 1.0,
        rest_offset: [0.0; 2],
        rect: [0.0, 0.0, 10.0, 10.0],
        anchor: [0.0; 2],
        born: None,
        clock: None,
        disable_fast_forward: false,
        reset_name: None,
        has_sprite: false,
    });
    let mut animator = json_ui::Animator::starting_at(0.0);
    animator.sample(&anims, 0.0, None);
    animator.sample(&anims, 1.0, None);
    let mut events = Vec::new();
    animate(&mut animator, &mut events);
    assert!(events.iter().any(
        |event| matches!(event, ScreenEvent::Button(button) if button.id == "button.menu_exit")
    ));
}

#[test]
fn review_pointer_release_then_press_keeps_the_second_capture() {
    let mut presentation = mini_engine_presentation();
    let mut player_runtime = crate::player_runtime::PlayerRuntime::new(1);
    let mut runtime = pack_harness::action_form(&mut player_runtime, "Shop", &["Buy"]);
    presentation
        .build(
            &player_runtime,
            &runtime,
            0,
            [1280, 720],
            ui::DpiScale::new(1.0).unwrap(),
        )
        .unwrap();
    let identity = runtime.server_forms().active().unwrap().identity;
    let frame = presentation.form_engine_frame(identity).unwrap().clone();
    let hit = &frame.hits[0];
    let gui = [
        frame.origin[0] + (hit.rect.x + 1.0) as f32 * frame.scale,
        frame.origin[1] + (hit.rect.y + 1.0) as f32 * frame.scale,
    ];
    drive(
        &mut runtime,
        &frame,
        EngineInput {
            cursor: Some(UiPoint::new(gui[0], gui[1]).unwrap()),
            keys: &ButtonInput::default(),
            pointer: PointerButtons {
                pressed: true,
                released: true,
                held: true,
            },
            pointer_edges: vec![false, true],
            wheel: Vec::new(),
            typed: Vec::new(),
            now: 0.0,
            animator: None,
        },
    );
    assert!(runtime.server_forms().engine().view.pressed.is_some());
}
