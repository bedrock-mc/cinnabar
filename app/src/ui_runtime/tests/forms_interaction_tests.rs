use super::forms_fixture::retained;
use crate::{
    menu::{MenuClipboard, MenuRuntime, drive_menu_input},
    ui_runtime::{
        drive_chat_keyboard_input, drive_server_form_input, presentation::tests::fixture_font,
    },
};
use bevy::{
    ecs::schedule::{IntoSystemSet, NodeId, ScheduleGraph, Schedules, SystemSet, graph::DiGraph},
    input::{
        ButtonState,
        keyboard::{Key, KeyboardInput, NativeKey},
        mouse::{AccumulatedMouseMotion, MouseWheel},
    },
    prelude::*,
    time::Real,
    window::{CursorOptions, PrimaryWindow},
};
use client_ui::ui_runtime::{UiRuntime, flush_form_response, presentation::UiPresentationRuntime};

#[test]
fn production_committed_stream_poll_precedes_form_input_authority_without_moving_publication() {
    use crate::{
        app::{
            ClientFrameSet, configure_client_frame_schedule,
            configure_client_production_frame_systems,
        },
        runtime::world::{
            drain_committed_ui_before_authority, drive_world_stream,
            reconcile_world_stream_before_physics,
        },
    };
    let mut app = App::new();
    configure_client_frame_schedule(&mut app);
    configure_client_production_frame_systems(&mut app);
    let schedules = app.world().resource::<Schedules>();
    let graph = schedules.get(Update).unwrap().graph();
    let authority = NodeId::Set(
        graph
            .system_sets
            .get_key(ClientFrameSet::UiAuthority.intern())
            .unwrap(),
    );
    assert!(
        schedule_reaches(
            graph.dependency().graph(),
            production_system_node(graph, reconcile_world_stream_before_physics),
            authority,
        ),
        "the real stream commit must precede admission-frame form/cursor authority"
    );
    let drain = NodeId::Set(
        graph
            .system_sets
            .get_key(
                drain_committed_ui_before_authority
                    .into_system_set()
                    .intern(),
            )
            .unwrap(),
    );
    assert!(
        schedule_reaches(
            graph.dependency().graph(),
            production_system_node(graph, reconcile_world_stream_before_physics),
            drain,
        ),
        "the sole UI drain follows real committed stream reconciliation"
    );
    assert!(
        schedule_reaches(
            graph.dependency().graph(),
            production_system_node(graph, drain_committed_ui_before_authority),
            authority,
        ),
        "the actual sole committed UI consumer precedes form input"
    );
    let publication = NodeId::Set(
        graph
            .system_sets
            .get_key(ClientFrameSet::WorldPublication.intern())
            .unwrap(),
    );
    assert!(
        schedule_reaches(
            graph.hierarchy().graph(),
            publication,
            production_system_node(graph, drive_world_stream)
        ),
        "render/world publication remains in its existing later phase"
    );
}

/// Checks schedule ordering or set membership through any number of intermediate nodes.
fn schedule_reaches(graph: &DiGraph<NodeId>, from: NodeId, target: NodeId) -> bool {
    let mut pending = vec![from];
    let mut visited = Vec::new();
    while let Some(node) = pending.pop() {
        if node == target {
            return true;
        }
        if !visited.contains(&node) {
            visited.push(node);
            pending.extend(graph.neighbors(node));
        }
    }
    false
}

fn production_system_node<M>(graph: &ScheduleGraph, system: impl IntoSystemSet<M>) -> NodeId {
    let parent = NodeId::Set(
        graph
            .system_sets
            .get_key(system.into_system_set().intern())
            .unwrap(),
    );
    graph
        .systems
        .iter()
        .find_map(|(key, _, _)| {
            let child = NodeId::System(key);
            graph
                .hierarchy()
                .graph()
                .contains_edge(parent, child)
                .then_some(child)
        })
        .unwrap()
}

fn app(player_runtime: &mut crate::player_runtime::PlayerRuntime) -> (App, Entity) {
    app_for(player_runtime, false)
}
fn app_for(
    player_runtime: &mut crate::player_runtime::PlayerRuntime,
    unsupported: bool,
) -> (App, Entity) {
    let mut app = App::new();
    let mut menu = MenuRuntime::new(true, 2, "Test".into());
    menu.set_visible(false);
    let mut runtime = UiRuntime::new(1);
    let mut event = retained(7, 1);
    if unsupported && let protocol::UiEvent::Form(form) = &mut event.event {
        form.model = protocol::ServerFormModel::Unsupported(protocol::UnsupportedForm::Controls);
    }
    runtime.apply(player_runtime, event).unwrap();
    let mut presentation = UiPresentationRuntime::new(fixture_font()).unwrap();
    presentation
        .build(
            player_runtime,
            &runtime,
            0,
            [1280, 720],
            ui::DpiScale::new(1.0).unwrap(),
        )
        .unwrap();
    app.add_message::<KeyboardInput>()
        .add_message::<MouseWheel>()
        .init_resource::<Time<Real>>()
        .init_resource::<Touches>()
        .init_resource::<ButtonInput<KeyCode>>()
        .init_resource::<ButtonInput<MouseButton>>()
        .init_resource::<AccumulatedMouseMotion>()
        .init_resource::<MenuClipboard>()
        .insert_resource(runtime)
        .insert_resource(player_runtime.clone())
        .insert_resource(presentation)
        .insert_resource(menu)
        .add_systems(
            Update,
            (
                drive_server_form_input,
                drive_chat_keyboard_input,
                drive_menu_input,
            )
                .chain(),
        );
    let entity = app
        .world_mut()
        .spawn((
            Window {
                focused: true,
                ..Default::default()
            },
            CursorOptions::default(),
            PrimaryWindow,
        ))
        .id();
    (app, entity)
}
fn press(app: &mut App, entity: Entity, key: KeyCode) {
    app.world_mut()
        .resource_mut::<ButtonInput<KeyCode>>()
        .press(key);
    app.world_mut().write_message(KeyboardInput {
        key_code: key,
        logical_key: Key::Unidentified(NativeKey::Unidentified),
        state: ButtonState::Pressed,
        text: None,
        repeat: false,
        window: entity,
    });
}

#[test]
fn form_open_and_answer_frames_consume_gameplay_chat_inventory_and_pause_edges() {
    let mut player_runtime = crate::player_runtime::PlayerRuntime::new(1);

    for (action, unsupported) in [
        (KeyCode::Enter, false),
        (KeyCode::Escape, false),
        (KeyCode::Enter, true),
        (KeyCode::Escape, true),
    ] {
        let (mut app, entity) = app_for(&mut player_runtime, unsupported);
        for key in [KeyCode::KeyT, KeyCode::KeyE, KeyCode::KeyW, action] {
            press(&mut app, entity, key);
        }
        app.world_mut()
            .resource_mut::<ButtonInput<MouseButton>>()
            .press(MouseButton::Right);
        app.update();
        let runtime = app.world().resource::<UiRuntime>();
        assert!(
            runtime.ui_focused(&player_runtime),
            "transition-frame pending answer owns semantic context"
        );
        assert!(!runtime.chat_focused());
        assert!(!runtime.inventory_open());
        assert!(!app.world().resource::<MenuRuntime>().is_visible());
        assert!(
            app.world()
                .resource::<ButtonInput<KeyCode>>()
                .get_pressed()
                .next()
                .is_none()
        );
        assert!(
            app.world()
                .resource::<ButtonInput<MouseButton>>()
                .get_pressed()
                .next()
                .is_none()
        );
        let mut packets = Vec::new();
        flush_form_response(&mut app.world_mut().resource_mut::<UiRuntime>(), |packet| {
            packets.push(packet);
            Ok(())
        })
        .unwrap();
        assert_eq!(packets.len(), 1);
        app.update(); // returns cursor ownership without replaying old messages
        assert!(
            !app.world()
                .resource::<UiRuntime>()
                .ui_focused(&player_runtime)
        );
        assert!(!app.world().resource::<MenuRuntime>().is_visible());
        press(&mut app, entity, KeyCode::KeyT);
        app.update();
        assert!(
            app.world().resource::<UiRuntime>().chat_focused(),
            "normal controls resume"
        );
    }
}

#[test]
fn pointer_action_is_bound_to_rendered_revision_and_keyboard_focus_selects_index() {
    let mut player_runtime = crate::player_runtime::PlayerRuntime::new(1);

    let (mut app, entity) = app(&mut player_runtime);
    press(&mut app, entity, KeyCode::ArrowDown);
    app.update();
    assert_eq!(
        app.world().resource::<UiRuntime>().server_forms().focus(),
        1
    );
    press(&mut app, entity, KeyCode::Enter);
    app.update();
    let mut response = None;
    flush_form_response(&mut app.world_mut().resource_mut::<UiRuntime>(), |packet| {
        response = Some(packet);
        Ok(())
    })
    .unwrap();
    let wire = protocol::encode(
        &response.unwrap(),
        &protocol::BedrockSession { shield_item_id: 0 },
    )
    .unwrap();
    assert_eq!(
        wire,
        protocol::encode(
            &protocol::modal_form_submit_response(
                7,
                protocol::ModalFormResponseSelection::ButtonIndex(1)
            ),
            &protocol::BedrockSession { shield_item_id: 0 }
        )
        .unwrap()
    );
}

#[test]
fn mouse_activates_visible_buttons_once_and_rejects_old_rendered_hits() {
    let mut player_runtime = crate::player_runtime::PlayerRuntime::new(1);

    for stale in [false, true] {
        let (mut app, entity) = app(&mut player_runtime);
        let point = ui::UiPoint::new(640.0, 156.0).unwrap();
        let hit = app
            .world()
            .resource::<UiPresentationRuntime>()
            .hit_test_form(point)
            .unwrap();
        assert_eq!(
            hit.1,
            client_ui::ui_runtime::LocalFormAction::SubmitButton(0)
        );
        if stale {
            app.world_mut()
                .resource_mut::<UiRuntime>()
                .apply(&mut player_runtime, retained(7, 2))
                .unwrap();
        }
        app.world_mut()
            .get_mut::<Window>(entity)
            .unwrap()
            .set_cursor_position(Some(Vec2::new(point.x(), point.y())));
        app.world_mut()
            .resource_mut::<ButtonInput<MouseButton>>()
            .press(MouseButton::Left);
        app.update();
        let sent =
            flush_form_response(&mut app.world_mut().resource_mut::<UiRuntime>(), |_| Ok(()))
                .unwrap();
        assert_eq!(sent, !stale);
        assert!(
            !flush_form_response(&mut app.world_mut().resource_mut::<UiRuntime>(), |_| Ok(()))
                .unwrap()
        );
        if stale {
            assert!(
                app.world()
                    .resource::<UiRuntime>()
                    .server_forms()
                    .active()
                    .is_some()
            );
        }
        assert!(!app.world().resource::<MenuRuntime>().is_visible());
    }
}

#[test]
fn enter_reveals_a_hidden_button_before_it_can_submit() {
    let mut player_runtime = crate::player_runtime::PlayerRuntime::new(1);

    let (mut app, entity) = app(&mut player_runtime);
    let mut event = retained(7, 2);
    if let protocol::UiEvent::Form(form) = &mut event.event
        && let protocol::ServerFormModel::TextMenu(menu) = &mut form.model
    {
        menu.content = "Long body\n".repeat(100).into();
        menu.omitted_images = 1;
    }
    let mut runtime = app.world_mut().remove_resource::<UiRuntime>().unwrap();
    runtime.apply(&mut player_runtime, event).unwrap();
    app.world_mut()
        .resource_mut::<UiPresentationRuntime>()
        .build(
            &player_runtime,
            &runtime,
            0,
            [1280, 720],
            ui::DpiScale::new(1.0).unwrap(),
        )
        .unwrap();
    let identity = runtime.server_forms().active().unwrap().identity;
    assert!(
        !app.world()
            .resource::<UiPresentationRuntime>()
            .form_button_visible(identity, 0)
    );
    app.insert_resource(runtime);
    press(&mut app, entity, KeyCode::Enter);
    app.update();
    let runtime = app.world_mut().remove_resource::<UiRuntime>().unwrap();
    assert!(runtime.server_forms().active().is_some());
    assert!(runtime.server_forms().scroll() > 0);
    app.world_mut()
        .resource_mut::<UiPresentationRuntime>()
        .build(
            &player_runtime,
            &runtime,
            0,
            [1280, 720],
            ui::DpiScale::new(1.0).unwrap(),
        )
        .unwrap();
    assert!(
        app.world()
            .resource::<UiPresentationRuntime>()
            .form_button_visible(identity, 0)
    );
    app.insert_resource(runtime);
    press(&mut app, entity, KeyCode::Enter);
    app.update();
    assert!(
        flush_form_response(&mut app.world_mut().resource_mut::<UiRuntime>(), |_| Ok(())).unwrap()
    );
    assert!(
        !flush_form_response(&mut app.world_mut().resource_mut::<UiRuntime>(), |_| Ok(())).unwrap()
    );
}

// A press then release over a button's drawn rect answers with that button's
// index, on a HiDPI window whose cursor reports logical pixels.
#[test]
fn clicking_an_engine_drawn_button_answers_its_index() {
    let mut player_runtime = crate::player_runtime::PlayerRuntime::new(1);

    use crate::ui_runtime::presentation::forms::{pack_harness, tests::mini_engine_presentation};
    use bevy::input::mouse::MouseButtonInput;
    let runtime = pack_harness::action_form(&mut player_runtime, "Menu", &["A", "B", "C"]);
    let mut presentation = mini_engine_presentation();
    let (physical, dpi) = ([2560, 1440], ui::DpiScale::new(2.0).unwrap());
    presentation
        .build(&player_runtime, &runtime, 0, physical, dpi)
        .unwrap();
    let identity = runtime.server_forms().active().unwrap().identity;
    let frame = presentation.form_engine_frame(identity).unwrap();
    let hit = frame
        .hits
        .iter()
        .find(|hit| hit.collection_index == Some(1))
        .unwrap();
    let centre = [
        frame.origin[0] + (hit.rect.x + hit.rect.w / 2.0) as f32 * frame.scale,
        frame.origin[1] + (hit.rect.y + hit.rect.h / 2.0) as f32 * frame.scale,
    ];
    let mut app = App::new();
    app.add_message::<KeyboardInput>()
        .add_message::<MouseWheel>()
        .add_message::<MouseButtonInput>()
        .init_resource::<ButtonInput<KeyCode>>()
        .init_resource::<ButtonInput<MouseButton>>()
        .init_resource::<AccumulatedMouseMotion>()
        .insert_resource(runtime)
        .insert_resource(player_runtime.clone())
        .insert_resource(presentation)
        .add_systems(Update, drive_server_form_input);
    let mut window = Window {
        focused: true,
        ..Default::default()
    };
    window.resolution.set_scale_factor_override(Some(2.0));
    window
        .resolution
        .set_physical_resolution(physical[0], physical[1]);
    window.set_cursor_position(Some(Vec2::new(centre[0], centre[1])));
    let entity = app
        .world_mut()
        .spawn((window, CursorOptions::default(), PrimaryWindow))
        .id();
    for state in [ButtonState::Pressed, ButtonState::Released] {
        if state == ButtonState::Pressed {
            app.world_mut()
                .resource_mut::<ButtonInput<MouseButton>>()
                .press(MouseButton::Left);
        }
        app.world_mut().write_message(MouseButtonInput {
            button: MouseButton::Left,
            state,
            window: entity,
        });
        app.update();
    }
    let mut response = None;
    flush_form_response(&mut app.world_mut().resource_mut::<UiRuntime>(), |packet| {
        response = Some(packet);
        Ok(())
    })
    .unwrap();
    let session = protocol::BedrockSession { shield_item_id: 0 };
    assert_eq!(
        protocol::encode(&response.expect("the click answered"), &session).unwrap(),
        protocol::encode(
            &protocol::modal_form_submit_response(
                3,
                protocol::ModalFormResponseSelection::ButtonIndex(1)
            ),
            &session
        )
        .unwrap()
    );
}

// A key held from gameplay auto-repeats; repeats must never press form buttons (pages skipped).
#[test]
fn held_keys_auto_repeating_into_an_open_form_do_not_answer_it() {
    use crate::ui_runtime::presentation::forms::pack_harness;
    use bevy::input::mouse::MouseButtonInput;
    let mut player_runtime = crate::player_runtime::PlayerRuntime::new(1);
    let runtime = pack_harness::action_form(&mut player_runtime, "Menu", &["A", "B", "C"]);
    // Vanilla's form screen carries the real key mappings; skipped without the installed pack.
    let Some(mut presentation) = pack_harness::engine_presentation() else {
        return;
    };
    let (physical, dpi) = ([2560, 1440], ui::DpiScale::new(2.0).unwrap());
    presentation
        .build(&player_runtime, &runtime, 0, physical, dpi)
        .unwrap();
    let identity = runtime.server_forms().active().unwrap().identity;
    let frame = presentation.form_engine_frame(identity).unwrap();
    let hit = frame
        .hits
        .iter()
        .find(|hit| hit.collection_index == Some(1))
        .unwrap();
    let centre = [
        frame.origin[0] + (hit.rect.x + hit.rect.w / 2.0) as f32 * frame.scale,
        frame.origin[1] + (hit.rect.y + hit.rect.h / 2.0) as f32 * frame.scale,
    ];
    let mut app = App::new();
    app.add_message::<KeyboardInput>()
        .add_message::<MouseWheel>()
        .add_message::<MouseButtonInput>()
        .init_resource::<ButtonInput<KeyCode>>()
        .init_resource::<ButtonInput<MouseButton>>()
        .init_resource::<AccumulatedMouseMotion>()
        .insert_resource(runtime)
        .insert_resource(player_runtime)
        .insert_resource(presentation)
        .add_systems(Update, drive_server_form_input);
    let mut window = Window {
        focused: true,
        ..Default::default()
    };
    window.resolution.set_scale_factor_override(Some(2.0));
    window
        .resolution
        .set_physical_resolution(physical[0], physical[1]);
    window.set_cursor_position(Some(Vec2::new(centre[0], centre[1])));
    let entity = app
        .world_mut()
        .spawn((window, CursorOptions::default(), PrimaryWindow))
        .id();
    // Space held for a jump keeps auto-repeating after the form opens.
    for _ in 0..4 {
        app.world_mut().write_message(KeyboardInput {
            key_code: KeyCode::Space,
            logical_key: Key::Space,
            state: ButtonState::Pressed,
            text: Some(" ".into()),
            repeat: true,
            window: entity,
        });
        app.update();
    }
    let mut response = None;
    flush_form_response(&mut app.world_mut().resource_mut::<UiRuntime>(), |packet| {
        response = Some(packet);
        Ok(())
    })
    .unwrap();
    assert!(response.is_none(), "an auto-repeated key answered the form");
}

// Zeqa answers a page before the mouse is released; that release must not click the next page.
#[test]
fn a_release_after_the_next_page_arrives_does_not_click_it() {
    use crate::ui_runtime::presentation::forms::pack_harness;
    use bevy::input::mouse::MouseButtonInput;
    use client_ui::ui_runtime::SequencedUiEvent;
    use protocol::{FormKind, FormRequestEvent, ServerFormModel, TextMenuForm, UiEvent};
    use std::sync::Arc;
    let Some(mut presentation) = pack_harness::engine_presentation() else {
        return;
    };
    let mut player_runtime = crate::player_runtime::PlayerRuntime::new(1);
    let runtime = pack_harness::action_form(&mut player_runtime, "Menu", &["A", "B", "C"]);
    let (physical, dpi) = ([1280, 720], ui::DpiScale::new(1.0).unwrap());
    presentation
        .build(&player_runtime, &runtime, 0, physical, dpi)
        .unwrap();
    let identity = runtime.server_forms().active().unwrap().identity;
    let frame = presentation.form_engine_frame(identity).unwrap();
    let hit = frame
        .hits
        .iter()
        .find(|hit| hit.collection_index == Some(1))
        .unwrap();
    let centre = [
        frame.origin[0] + (hit.rect.x + hit.rect.w / 2.0) as f32 * frame.scale,
        frame.origin[1] + (hit.rect.y + hit.rect.h / 2.0) as f32 * frame.scale,
    ];
    let mut app = App::new();
    app.add_message::<KeyboardInput>()
        .add_message::<MouseWheel>()
        .add_message::<MouseButtonInput>()
        .init_resource::<ButtonInput<KeyCode>>()
        .init_resource::<ButtonInput<MouseButton>>()
        .init_resource::<AccumulatedMouseMotion>()
        .insert_resource(runtime)
        .insert_resource(player_runtime)
        .insert_resource(presentation)
        .add_systems(Update, drive_server_form_input);
    let mut window = Window {
        focused: true,
        ..Default::default()
    };
    window
        .resolution
        .set_physical_resolution(physical[0], physical[1]);
    window.set_cursor_position(Some(Vec2::new(centre[0], centre[1])));
    let entity = app
        .world_mut()
        .spawn((window, CursorOptions::default(), PrimaryWindow))
        .id();
    let send = |app: &mut App, state: ButtonState| {
        {
            let mut mouse = app.world_mut().resource_mut::<ButtonInput<MouseButton>>();
            match state {
                ButtonState::Pressed => mouse.press(MouseButton::Left),
                ButtonState::Released => mouse.release(MouseButton::Left),
            }
        }
        app.world_mut().write_message(MouseButtonInput {
            button: MouseButton::Left,
            state,
            window: entity,
        });
        app.update();
    };
    let take = |app: &mut App| {
        let mut sent = Vec::new();
        flush_form_response(&mut app.world_mut().resource_mut::<UiRuntime>(), |packet| {
            sent.push(packet);
            Ok(())
        })
        .unwrap();
        sent
    };
    send(&mut app, ButtonState::Pressed);
    let first = take(&mut app);
    // The server's next page arrives while the button is still held.
    app.world_mut()
        .resource_scope(|world, mut runtime: Mut<UiRuntime>| {
            let mut player_runtime = world
                .remove_resource::<crate::player_runtime::PlayerRuntime>()
                .unwrap();
            runtime
                .apply(
                    &mut player_runtime,
                    SequencedUiEvent {
                        session_id: 1,
                        fifo_sequence: 2,
                        local_millis: 0,
                        server_tick: None,
                        event: UiEvent::Form(FormRequestEvent {
                            form_id: 4,
                            kind: FormKind::Menu,
                            title: Some(Arc::from("Next")),
                            json: Arc::from("{}"),
                            model: ServerFormModel::TextMenu(TextMenuForm {
                                title: "Next".into(),
                                content: "".into(),
                                buttons: ["A", "B", "C"]
                                    .iter()
                                    .map(|text| protocol::FormText::from(*text))
                                    .collect(),
                                button_images: Vec::new().into(),
                                omitted_images: 0,
                            }),
                        }),
                    },
                )
                .unwrap();
            world
                .resource_mut::<UiPresentationRuntime>()
                .build(&player_runtime, &runtime, 16, physical, dpi)
                .unwrap();
            world.insert_resource(player_runtime);
        });
    app.update();
    send(&mut app, ButtonState::Released);
    let second = take(&mut app);
    assert!(
        first.len() + second.len() <= 1,
        "one physical click answered more than one page"
    );
}

// With no form open, a left click stays gameplay's: the form input system never swallows it.
#[test]
fn a_click_without_a_form_reaches_gameplay() {
    let player_runtime = crate::player_runtime::PlayerRuntime::new(1);

    use bevy::input::mouse::MouseButtonInput;
    let mut app = App::new();
    let runtime = UiRuntime::new(1);
    let mut presentation = UiPresentationRuntime::new(fixture_font()).unwrap();
    presentation
        .build(
            &player_runtime,
            &runtime,
            0,
            [1280, 720],
            ui::DpiScale::new(1.0).unwrap(),
        )
        .unwrap();
    app.add_message::<KeyboardInput>()
        .add_message::<MouseWheel>()
        .add_message::<MouseButtonInput>()
        .init_resource::<ButtonInput<KeyCode>>()
        .init_resource::<ButtonInput<MouseButton>>()
        .init_resource::<AccumulatedMouseMotion>()
        .insert_resource(runtime)
        .insert_resource(player_runtime.clone())
        .insert_resource(presentation)
        .add_systems(Update, drive_server_form_input);
    let entity = app
        .world_mut()
        .spawn((
            Window {
                focused: true,
                ..Default::default()
            },
            CursorOptions::default(),
            PrimaryWindow,
        ))
        .id();
    app.world_mut()
        .resource_mut::<ButtonInput<MouseButton>>()
        .press(MouseButton::Left);
    app.world_mut().write_message(MouseButtonInput {
        button: MouseButton::Left,
        state: ButtonState::Pressed,
        window: entity,
    });
    app.update();
    assert!(
        app.world()
            .resource::<ButtonInput<MouseButton>>()
            .just_pressed(MouseButton::Left),
        "the swing click must survive the form input system"
    );
}

// A custom form's toggle and edit box keep their values through the mapping dispatcher.
#[test]
fn custom_form_toggle_and_input_edit_their_values() {
    let mut player_runtime = crate::player_runtime::PlayerRuntime::new(1);

    use crate::ui_runtime::presentation::forms::pack_harness;
    use bevy::input::mouse::MouseButtonInput;
    use client_ui::ui_runtime::forms::FormValue;
    use json_ui::HitKind;
    use protocol::{CustomForm, CustomFormElement, FormRequestEvent, ServerFormModel, UiEvent};
    use std::sync::Arc;
    let Some(mut presentation) = pack_harness::engine_presentation() else {
        eprintln!(
            "skipping custom_form_toggle_and_input_edit_their_values: fixture unavailable; requires installed local carriers (make assets)"
        );
        return;
    };
    let mut runtime = UiRuntime::new(1);
    runtime
        .apply(
            &mut player_runtime,
            client_ui::ui_runtime::SequencedUiEvent {
                session_id: 1,
                fifo_sequence: 1,
                local_millis: 0,
                server_tick: None,
                event: UiEvent::Form(FormRequestEvent {
                    form_id: 3,
                    kind: protocol::FormKind::Custom,
                    title: Some(Arc::from("T")),
                    json: Arc::from("{}"),
                    model: ServerFormModel::Custom(CustomForm {
                        icon: None,
                        title: "T".into(),
                        elements: vec![
                            CustomFormElement::Toggle {
                                text: "On".into(),
                                default: false,
                                tooltip: None,
                            },
                            CustomFormElement::Input {
                                text: "Name".into(),
                                placeholder: "".into(),
                                default: Arc::from(""),
                                tooltip: None,
                            },
                        ]
                        .into(),
                        submit: None,
                    }),
                }),
            },
        )
        .unwrap();
    let (physical, dpi) = ([2560, 1440], ui::DpiScale::new(2.0).unwrap());
    presentation
        .build(&player_runtime, &runtime, 0, physical, dpi)
        .unwrap();
    let identity = runtime.server_forms().active().unwrap().identity;
    let frame = presentation.form_engine_frame(identity).unwrap().clone();
    let centre = |kind: HitKind| {
        let hit = frame.hits.iter().find(|hit| hit.kind == kind).unwrap();
        Vec2::new(
            frame.origin[0] + (hit.rect.x + hit.rect.w / 2.0) as f32 * frame.scale,
            frame.origin[1] + (hit.rect.y + hit.rect.h / 2.0) as f32 * frame.scale,
        )
    };
    let mut app = App::new();
    app.add_message::<KeyboardInput>()
        .add_message::<MouseWheel>()
        .add_message::<MouseButtonInput>()
        .init_resource::<ButtonInput<KeyCode>>()
        .init_resource::<ButtonInput<MouseButton>>()
        .init_resource::<AccumulatedMouseMotion>()
        .insert_resource(runtime)
        .insert_resource(player_runtime.clone())
        .insert_resource(presentation)
        .add_systems(Update, drive_server_form_input);
    let mut window = Window {
        focused: true,
        ..Default::default()
    };
    window.resolution.set_scale_factor_override(Some(2.0));
    window
        .resolution
        .set_physical_resolution(physical[0], physical[1]);
    let entity = app
        .world_mut()
        .spawn((window, CursorOptions::default(), PrimaryWindow))
        .id();
    let click = |app: &mut App, at: Vec2| {
        let mut windows = app.world_mut().query::<&mut Window>();
        windows
            .single_mut(app.world_mut())
            .unwrap()
            .set_cursor_position(Some(at));
        app.update();
        for state in [ButtonState::Pressed, ButtonState::Released] {
            if state == ButtonState::Pressed {
                app.world_mut()
                    .resource_mut::<ButtonInput<MouseButton>>()
                    .press(MouseButton::Left);
            }
            app.world_mut().write_message(MouseButtonInput {
                button: MouseButton::Left,
                state,
                window: entity,
            });
            app.update();
        }
    };
    click(&mut app, centre(HitKind::Toggle));
    let values = |app: &App| {
        app.world()
            .resource::<UiRuntime>()
            .server_forms()
            .engine()
            .values
            .clone()
    };
    assert_eq!(values(&app)[0], FormValue::Toggle(true));
    click(&mut app, centre(HitKind::EditBox));
    app.world_mut().write_message(KeyboardInput {
        key_code: KeyCode::KeyX,
        logical_key: Key::Character("x".into()),
        state: ButtonState::Pressed,
        text: Some("x".into()),
        repeat: false,
        window: entity,
    });
    app.update();
    assert_eq!(values(&app)[1], FormValue::Text("x".into()));
}

#[test]
fn explicit_form_return_survives_pending_response_after_overlay_focus_loss() {
    let mut player = crate::player_runtime::PlayerRuntime::new(1);
    for key in [KeyCode::Enter, KeyCode::Escape] {
        let (mut app, window) = app(&mut player);
        let mut focus = client_presentation::camera::CursorFocus::default();
        focus.begin_frame(false);
        app.insert_resource(focus)
            .insert_resource(crate::camera::AutoFly::new(false))
            .add_systems(
                Update,
                crate::camera::update_cursor_capture.after(drive_menu_input),
            );
        app.update();
        {
            let mut focus = app
                .world_mut()
                .resource_mut::<client_presentation::camera::CursorFocus>();
            focus.begin_frame(true);
            focus.record_activation(true);
        }
        press(&mut app, window, key);
        app.update();
        assert!(
            app.world()
                .resource::<UiRuntime>()
                .server_forms()
                .active()
                .is_none()
        );
        assert!(
            app.world()
                .resource::<UiRuntime>()
                .server_forms()
                .owns_input()
        );
        assert_eq!(
            app.world().get::<CursorOptions>(window).unwrap().grab_mode,
            bevy::window::CursorGrabMode::None
        );
        flush_form_response(&mut app.world_mut().resource_mut::<UiRuntime>(), |_| Ok(())).unwrap();
        app.world_mut()
            .resource_mut::<client_presentation::camera::CursorFocus>()
            .begin_frame(true);
        app.update();
        let cursor = app.world().get::<CursorOptions>(window).unwrap();
        assert_eq!(cursor.grab_mode, bevy::window::CursorGrabMode::Locked);
        assert!(!cursor.visible);
    }
}
