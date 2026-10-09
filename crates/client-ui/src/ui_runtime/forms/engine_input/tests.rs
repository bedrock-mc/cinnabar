//! Screen cancel mappings survive rendering only a form's content subtree.

use super::*;
use crate::ui_runtime::presentation::{
    UiPresentationRuntime,
    forms::{ServerUiPack, pack_harness, tests::mini_engine_presentation},
};

#[test]
fn complete_large_menu_final_button_sends_its_original_wire_index() {
    for count in [257usize, 300] {
        let mut player_runtime = player_state::PlayerState::new(1);
        let labels: Vec<_> = (0..count).map(|index| format!("Choice {index}")).collect();
        let labels: Vec<_> = labels.iter().map(String::as_str).collect();
        let mut runtime = pack_harness::action_form(&mut player_runtime, "Menu", &labels);
        let mut presentation = mini_engine_presentation();
        presentation
            .build(
                &player_runtime,
                &runtime,
                0,
                [1280, 65_536],
                ui::DpiScale::new(1.0).unwrap(),
            )
            .unwrap();
        let identity = runtime.server_forms().active().unwrap().identity;
        let frame = presentation.form_engine_frame(identity).unwrap().clone();
        assert_eq!(frame.hits.len(), count);
        let last = frame
            .hits
            .iter()
            .find(|hit| hit.collection_index == Some(count - 1))
            .unwrap();
        let cursor = UiPoint::new(
            frame.origin[0] + (last.rect.x + 1.0) as f32 * frame.scale,
            frame.origin[1] + (last.rect.y + 1.0) as f32 * frame.scale,
        )
        .unwrap();
        assert!(last.contains(frame.to_virtual(cursor)));
        for down in [true, false] {
            drive(
                &mut runtime,
                &frame,
                EngineInput {
                    cursor: Some(cursor),
                    keys: &ButtonInput::default(),
                    pointer: PointerButtons {
                        pressed: down,
                        released: !down,
                        held: down,
                    },
                    pointer_edges: vec![down],
                    wheel: Vec::new(),
                    typed: Vec::new(),
                    now: if down { 0.0 } else { 0.01 },
                    animator: None,
                },
            );
        }
        let mut sent = Vec::new();
        assert!(
            super::super::flush_form_response(&mut runtime, |packet| {
                sent.push(packet);
                Ok(())
            })
            .unwrap()
        );
        assert_eq!(
            sent,
            vec![protocol::modal_form_submit_response(
                identity.form_id,
                protocol::ModalFormResponseSelection::ButtonIndex((count - 1) as u32)
            )]
        );
    }
}

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
    let mut player_runtime = player_state::PlayerState::new(1);

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
    let mut player_runtime = player_state::PlayerState::new(1);

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
    let mut player_runtime = player_state::PlayerState::new(1);
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

#[test]
fn review_ui_release_applies_the_final_control_drag_position() {
    let mut presentation = mini_engine_presentation();
    let mut player_runtime = player_state::PlayerState::new(1);
    let mut runtime = pack_harness::action_form(&mut player_runtime, "Drag", &["Move"]);
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
    let mut region = frame.hits[0].clone();
    region.kind = HitKind::Draggable;
    region.drag_axes = [true, false];
    region.input.mappings.clear();
    let key = region.key.clone();
    let start = [region.rect.x + 1.0, region.rect.y + 1.0];
    frame.hits = vec![region].into();
    for (point, down) in [(start, true), ([start[0] + 20.0, start[1] + 10.0], false)] {
        let cursor = UiPoint::new(
            frame.origin[0] + point[0] as f32 * frame.scale,
            frame.origin[1] + point[1] as f32 * frame.scale,
        )
        .unwrap();
        drive(
            &mut runtime,
            &frame,
            EngineInput {
                cursor: Some(cursor),
                keys: &ButtonInput::default(),
                pointer: PointerButtons {
                    pressed: down,
                    released: !down,
                    held: down,
                },
                pointer_edges: vec![down],
                wheel: Vec::new(),
                typed: Vec::new(),
                now: 0.0,
                animator: None,
            },
        );
    }
    let engine = runtime.server_forms().engine();
    assert_eq!(engine.view.drags.get(&key), Some(&[20.0, 0.0]));
    assert!(engine.drag.is_none());
}

#[test]
fn review_ui_release_applies_the_final_scrollbar_position() {
    let mut presentation = mini_engine_presentation();
    let mut player_runtime = player_state::PlayerState::new(1);
    let mut runtime = pack_harness::action_form(&mut player_runtime, "Scroll", &["Move"]);
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
    let key = "scroll".to_owned();
    frame.report.scrolls.insert(
        key.clone(),
        json_ui::ScrollMetrics {
            offset: 10.0,
            content: 200.0,
            viewport: 50.0,
            track: Some([0.0, 0.0, 10.0, 100.0]),
            ..Default::default()
        },
    );
    runtime.server_forms_mut().engine_mut().drag =
        Some(crate::ui_runtime::forms::values::FormDrag::ScrollBox {
            view: key.clone(),
            last: 20.0,
        });
    drive(
        &mut runtime,
        &frame,
        EngineInput {
            cursor: Some(
                UiPoint::new(frame.origin[0], frame.origin[1] + 30.0 * frame.scale).unwrap(),
            ),
            keys: &ButtonInput::default(),
            pointer: PointerButtons {
                released: true,
                ..Default::default()
            },
            pointer_edges: vec![false],
            wheel: Vec::new(),
            typed: Vec::new(),
            now: 0.0,
            animator: None,
        },
    );
    let engine = runtime.server_forms().engine();
    assert_eq!(engine.view.scroll.get(&key), Some(&30.0));
    assert!(engine.drag.is_none());
}

#[test]
fn multiselect_edits_use_the_enclosing_form_and_normalize_option_order() {
    let Some(mut presentation) = pack_harness::engine_presentation() else {
        return;
    };
    let mut player = player_state::PlayerState::new(1);
    let mut runtime = crate::ui_runtime::presentation::forms::compatibility_tests::replay(
        &mut player,
        r#"{"type":"custom_form","content":[
            {"type":"multiselect","text":"First","options":["A","B","C"],"default":[2,0,2,-1]},
            {"type":"multiselect","text":"Second","options":["D","E"],"default":[1]}
        ]}"#,
        false,
    );
    runtime
        .server_forms_mut()
        .engine_mut()
        .open_multiselects
        .extend([0, 1]);
    assert_eq!(
        runtime.server_forms().engine().submission()[0],
        protocol::CustomFormValue::MultiSelect(vec![2, 0, 2, -1].into())
    );
    pack_harness::render(&mut presentation, &runtime, [1280, 1440], 1.0);
    let identity = runtime.server_forms().active().unwrap().identity;
    let frame = presentation.form_engine_frame(identity).unwrap().clone();
    let model = runtime.server_forms().active().unwrap().model.clone();
    let option = frame
        .hits
        .iter()
        .find(|hit| {
            hit.control_name.as_deref() == Some("custom_multiselect_checkbox")
                && hit.collections == [("custom_form".into(), 0), ("custom_multiselect".into(), 1)]
        })
        .expect("first form option B");
    for checked in [true, false] {
        assert_eq!(
            controller(
                &mut runtime,
                &frame,
                &model,
                &ScreenEvent::Toggle {
                    name: "custom_multiselect_checkbox".into(),
                    key: option.key.clone(),
                    index: Some(1),
                    checked,
                    by_click: true,
                },
                None
            ),
            None
        );
        assert_eq!(
            runtime.server_forms().engine().submission()[0],
            protocol::CustomFormValue::MultiSelect(
                if checked { vec![0, 1, 2] } else { vec![0, 2] }.into()
            )
        );
        assert_eq!(
            runtime.server_forms().engine().submission()[1],
            protocol::CustomFormValue::MultiSelect(vec![1].into())
        );
        assert!(
            runtime
                .server_forms()
                .engine()
                .open_multiselects
                .contains(&0)
        );
    }
}

#[test]
fn directional_slider_edits_advance_when_the_fraction_stays_on_the_same_grid_point() {
    let mut player = player_state::PlayerState::new(1);
    let mut runtime = crate::ui_runtime::presentation::forms::compatibility_tests::replay(
        &mut player,
        crate::ui_runtime::presentation::forms::compatibility_tests::CONTROLS,
        false,
    );
    let model = runtime.server_forms().active().unwrap().model.clone();
    assert_eq!(
        set_slider(&mut runtime, &model, 4, 0.41, None, true),
        Some(f64::from(0.6_f32))
    );
    assert_eq!(
        runtime.server_forms().engine().values[4],
        FormValue::Slider(6.0)
    );
}

#[test]
fn the_first_direction_after_pointer_tracking_only_snaps_the_value() {
    let mut player = player_state::PlayerState::new(1);
    let mut runtime = crate::ui_runtime::presentation::forms::compatibility_tests::replay(
        &mut player,
        crate::ui_runtime::presentation::forms::compatibility_tests::CONTROLS,
        false,
    );
    let model = runtime.server_forms().active().unwrap().model.clone();
    for (pointer, expected) in [(true, 4.0), (false, 4.0), (false, 6.0)] {
        let _ = set_slider(&mut runtime, &model, 4, 0.41, None, !pointer);
        assert_eq!(
            runtime.server_forms().engine().values[4],
            FormValue::Slider(expected)
        );
    }
}

#[test]
fn repeated_directional_input_uses_the_snapped_slider_position() {
    let Some(mut presentation) = pack_harness::engine_presentation() else {
        return;
    };
    let mut player = player_state::PlayerState::new(1);
    let mut runtime = crate::ui_runtime::presentation::forms::compatibility_tests::replay(
        &mut player,
        crate::ui_runtime::presentation::forms::compatibility_tests::CONTROLS,
        false,
    );
    pack_harness::render(&mut presentation, &runtime, [1280, 1440], 1.0);
    let identity = runtime.server_forms().active().unwrap().identity;
    let frame = presentation.form_engine_frame(identity).unwrap().clone();
    let model = runtime.server_forms().active().unwrap().model.clone();
    let slider = frame
        .hits
        .iter()
        .find(|hit| hit.kind == HitKind::Slider && hit.collection_index == Some(4))
        .unwrap();
    {
        let engine = runtime.server_forms_mut().engine_mut();
        engine.view.focused = Some(slider.key.clone());
        let dispatch = engine.dispatcher.button(
            &frame.hits,
            &mut engine.view,
            EngineButton {
                id: "button.menu_ok",
                down: true,
                point: None,
                mode: InputMode::Gamepad,
                now: -0.1,
            },
        );
        assert_eq!(
            engine.view.components.selected(),
            Some(slider.key.as_str()),
            "dispatch: {dispatch:?}"
        );
    }
    for (now, expected) in [(0.0, 6.0), (0.25, 8.0)] {
        let events = {
            let engine = runtime.server_forms_mut().engine_mut();
            engine
                .dispatcher
                .direction(&frame.hits, &mut engine.view, [1.0, 0.0], now)
                .events
        };
        assert!(events.iter().any(|event| matches!(
            event,
            ScreenEvent::Slider {
                directional: true,
                ..
            }
        )));
        for event in events {
            controller(&mut runtime, &frame, &model, &event, None);
        }
        assert_eq!(
            runtime.server_forms().engine().values[4],
            FormValue::Slider(expected)
        );
    }
}

#[test]
fn dropdown_edits_answer_and_close_only_the_enclosing_form_element() {
    let Some(mut presentation) = pack_harness::engine_presentation() else {
        return;
    };
    let mut player = player_state::PlayerState::new(1);
    let mut runtime = crate::ui_runtime::presentation::forms::compatibility_tests::replay(
        &mut player,
        r#"{"type":"custom_form","content":[
            {"type":"dropdown","text":"First","options":["A","B"],"default":0},
            {"type":"dropdown","text":"Second","options":["C","D"],"default":1}
        ]}"#,
        false,
    );
    runtime
        .server_forms_mut()
        .engine_mut()
        .open_dropdowns
        .extend([0, 1]);
    pack_harness::render(&mut presentation, &runtime, [1280, 1440], 1.0);
    let identity = runtime.server_forms().active().unwrap().identity;
    let frame = presentation.form_engine_frame(identity).unwrap().clone();
    let model = runtime.server_forms().active().unwrap().model.clone();
    let option = frame
        .hits
        .iter()
        .find(|hit| {
            hit.widget.toggle.as_ref().is_some_and(|toggle| {
                toggle.name.as_deref() == Some("custom_dropdown_radio_toggle")
            }) && hit.collections == [("custom_form".into(), 0), ("custom_dropdown".into(), 1)]
        })
        .expect("first dropdown option B");
    controller(
        &mut runtime,
        &frame,
        &model,
        &ScreenEvent::Toggle {
            name: "custom_dropdown_radio_toggle".into(),
            key: option.key.clone(),
            index: Some(1),
            checked: true,
            by_click: true,
        },
        None,
    );
    assert_eq!(
        runtime.server_forms().engine().submission().as_ref(),
        [
            protocol::CustomFormValue::Dropdown(1),
            protocol::CustomFormValue::Dropdown(1)
        ]
    );
    assert_eq!(
        runtime
            .server_forms()
            .engine()
            .open_dropdowns
            .iter()
            .copied()
            .collect::<Vec<_>>(),
        [1]
    );
}

#[test]
fn unchecked_dropdown_options_leave_toggle_answers_unchanged() {
    let mut player = player_state::PlayerState::new(1);
    let mut runtime = crate::ui_runtime::presentation::forms::compatibility_tests::replay(
        &mut player,
        r#"{"type":"custom_form","content":[
            {"type":"toggle","text":"Enabled","default":true},
            {"type":"dropdown","text":"Mode","options":["A","B"],"default":1}
        ]}"#,
        false,
    );
    let model = runtime.server_forms().active().unwrap().model.clone();
    let frame = EngineFrame {
        identity: Some(runtime.server_forms().active().unwrap().identity),
        hits: std::sync::Arc::from([]),
        report: Default::default(),
        cancel_target: None,
        origin: [0.0; 2],
        scale: 1.0,
        panel: None,
        edit_texts: Vec::new(),
    };
    controller(
        &mut runtime,
        &frame,
        &model,
        &ScreenEvent::Toggle {
            name: "custom_dropdown_radio_toggle".into(),
            key: "dropdown/option".into(),
            index: Some(0),
            checked: false,
            by_click: false,
        },
        None,
    );
    assert_eq!(
        runtime.server_forms().engine().submission().as_ref(),
        [
            protocol::CustomFormValue::Toggle(true),
            protocol::CustomFormValue::Dropdown(1)
        ]
    );
}
