use super::*;
use crate::{
    semantic_controls::{
        PendingDeviceFrame, SemanticInputRuntime, SemanticRouteState, SemanticTouchTargets,
        collect_raw_input, finalize_semantic_input_after_ui_authority, route_semantic_input,
    },
    ui_runtime::presentation::observe_mount_jump_input,
};
use bevy::{
    input::{
        InputPlugin,
        gamepad::{
            GamepadAxis, RawGamepadAxisChangedEvent, RawGamepadButtonChangedEvent, RawGamepadEvent,
        },
        mouse::MouseMotion,
        touch::{TouchInput, TouchPhase},
    },
    math::Vec2,
    prelude::{App, Entity, IntoScheduleConfigs, PreUpdate, Update},
    window::{CursorGrabMode, CursorMoved, WindowEvent},
};
use client_ui::test_support::{form_input_mode, mini_engine_presentation, pack_harness, rasterize};
use semantic_input::InputMode::{GamePad, KeyboardMouse};

/// Opens an original form through the production raw-input and UI observation systems.
fn host() -> (App, Entity, Entity) {
    let mut player = client_state::PlayerRuntime(player_state::PlayerState::new(1));
    let runtime = pack_harness::action_form(&mut player, "Menu", &["Continue"]);
    let mut presentation = mini_engine_presentation();
    presentation.set_server_ui_pack(&form_input_mode::pack());
    let mut app = App::new();
    app.add_plugins(InputPlugin)
        .add_message::<WindowEvent>()
        .init_resource::<Time<Real>>()
        .init_resource::<SemanticInputRuntime>()
        .init_resource::<PendingDeviceFrame>()
        .init_resource::<SemanticRouteState>()
        .init_resource::<SemanticTouchTargets>()
        .init_resource::<client_state::SemanticInputSnapshot>()
        .insert_resource(player)
        .insert_resource(UiRuntimeResource(runtime))
        .insert_resource(UiPresentationResource(presentation))
        .add_systems(
            PreUpdate,
            crate::ui_runtime::touch_pointer::emulate_ui_pointer.before(bevy::input::InputSystems),
        )
        .add_systems(
            Update,
            (
                crate::ui_runtime::gameplay_touch::drive_gameplay_touch_targets,
                collect_raw_input,
                route_semantic_input,
                drive_server_form_input,
                finalize_semantic_input_after_ui_authority,
                observe_mount_jump_input,
            )
                .chain(),
        );
    let window = app
        .world_mut()
        .spawn((
            Window::default(),
            CursorOptions {
                grab_mode: CursorGrabMode::Locked,
                visible: false,
                ..Default::default()
            },
            PrimaryWindow,
        ))
        .id();
    let pad = app.world_mut().spawn(Gamepad::default()).id();
    (app, pad, window)
}

/// Rebuilds through production presentation and returns its painted pixels.
fn pixels(app: &mut App) -> Vec<u8> {
    app.world_mut().resource_scope(
        |world, mut presentation: bevy::prelude::Mut<UiPresentationResource>| {
            let frame = presentation
                .build(
                    world.resource::<client_state::PlayerRuntime>(),
                    world.resource::<UiRuntimeResource>(),
                    0,
                    [640, 360],
                    ui::DpiScale::new(1.0).unwrap(),
                )
                .unwrap();
            rasterize(&frame).into_raw()
        },
    )
}

/// Borrows the finalized action frame produced by the raw-input pipeline.
fn input(app: &App) -> &semantic_input::ActionSnapshot {
    app.world()
        .resource::<client_state::SemanticInputSnapshot>()
        .snapshot()
        .unwrap()
}

/// Applies controller look activity before the production raw sampler runs.
fn controller(app: &mut App, pad: Entity, value: f32) {
    app.world_mut()
        .get_mut::<Gamepad>(pad)
        .unwrap()
        .analog_mut()
        .set(GamepadAxis::RightStickX, value);
    app.update();
}

/// Checks real pointer events against the shared mode and the form's painted bindings.
fn mouse_overrides_form_mode(event: impl FnOnce(Entity) -> WindowEvent) {
    let (mut app, pad, window) = host();
    let captured = app.world().get::<CursorOptions>(window).unwrap().clone();
    let mouse = pixels(&mut app);
    controller(&mut app, pad, 0.8);
    let gamepad = pixels(&mut app);
    assert!(gamepad != mouse, "controller use must reveal the image");
    assert_eq!(
        app.world().get::<CursorOptions>(window).unwrap().grab_mode,
        CursorGrabMode::None
    );
    let event = event(window);
    match &event {
        WindowEvent::CursorMoved(_) => {
            app.world_mut()
                .write_message(MouseMotion { delta: Vec2::X });
        }
        WindowEvent::MouseButtonInput(event) => {
            app.world_mut().write_message(*event);
        }
        WindowEvent::MouseWheel(event) => {
            app.world_mut().write_message(*event);
        }
        _ => unreachable!(),
    }
    app.world_mut().write_message(event);
    app.update();
    assert_eq!(input(&app).input_mode, GamePad);
    assert!(pixels(&mut app) == mouse);
    controller(&mut app, pad, 0.0);
    app.update();
    assert!(pixels(&mut app) == mouse);
    // A captured desktop sample changes the shared mode through the production sampler.
    *app.world_mut().get_mut::<CursorOptions>(window).unwrap() = captured;
    app.world_mut()
        .write_message(MouseMotion { delta: Vec2::X });
    app.update();
    assert_eq!(input(&app).input_mode, KeyboardMouse);
    controller(&mut app, pad, 0.9);
    assert!(pixels(&mut app) == gamepad);
}

#[test]
fn real_mouse_pointer_events_override_form_mode_until_shared_mode_changes() {
    let events: [fn(Entity) -> WindowEvent; 3] = [
        |window| {
            WindowEvent::CursorMoved(CursorMoved {
                window,
                position: Vec2::new(4.0, 4.0),
                delta: Some(Vec2::X),
            })
        },
        |window| {
            WindowEvent::MouseButtonInput(MouseButtonInput {
                button: MouseButton::Left,
                state: ButtonState::Pressed,
                window,
            })
        },
        |window| {
            WindowEvent::MouseWheel(MouseWheel {
                unit: MouseScrollUnit::Line,
                x: 0.0,
                y: 1.0,
                window,
                phase: TouchPhase::Moved,
            })
        },
    ];
    for event in events {
        mouse_overrides_form_mode(event);
    }
}

/// Exercises renewed controller input while the shared mode remains GamePad.
fn controller_after_mouse(event: impl FnOnce(Entity) -> RawGamepadEvent) {
    let (mut app, pad, window) = host();
    let mouse = pixels(&mut app);
    controller(&mut app, pad, 0.8);
    let gamepad = pixels(&mut app);
    assert!(gamepad != mouse);
    app.world_mut()
        .write_message(WindowEvent::CursorMoved(CursorMoved {
            window,
            position: Vec2::ZERO,
            delta: Some(Vec2::X),
        }));
    app.update();
    assert_eq!(input(&app).input_mode, GamePad);
    assert!(pixels(&mut app) == mouse);
    app.update();
    assert!(
        pixels(&mut app) == mouse,
        "a held stick must not reclaim the form"
    );
    app.world_mut()
        .write_message(RawGamepadEvent::Axis(RawGamepadAxisChangedEvent::new(
            pad,
            GamepadAxis::LeftStickX,
            0.2,
        )));
    app.update();
    assert!(
        pixels(&mut app) == mouse,
        "small stick input must retain mouse bindings"
    );
    app.world_mut().write_message(event(pad));
    app.update();
    assert_eq!(input(&app).input_mode, GamePad);
    assert_eq!(
        app.world().get::<CursorOptions>(window).unwrap().grab_mode,
        CursorGrabMode::None
    );
    assert!(
        pixels(&mut app) == gamepad,
        "fresh controller input must restore gamepad artwork"
    );
}

#[test]
fn fresh_controller_input_clears_the_form_mouse_override() {
    for axis in [
        GamepadAxis::RightStickX,
        GamepadAxis::RightStickY,
        GamepadAxis::LeftStickX,
        GamepadAxis::LeftStickY,
    ] {
        controller_after_mouse(|pad| {
            RawGamepadEvent::Axis(RawGamepadAxisChangedEvent::new(pad, axis, 0.9))
        });
    }
    for button in [GamepadButton::North, GamepadButton::Other(7)] {
        controller_after_mouse(|pad| {
            RawGamepadEvent::Button(RawGamepadButtonChangedEvent::new(pad, button, 1.0))
        });
    }
}

#[test]
fn idle_controller_and_touch_emulation_keep_gamepad_form_mode() {
    let (mut app, pad, window) = host();
    let mouse = pixels(&mut app);
    controller(&mut app, pad, 0.8);
    let gamepad = pixels(&mut app);
    assert!(gamepad != mouse);
    controller(&mut app, pad, 0.0);
    app.update();
    assert_eq!(input(&app).input_mode, GamePad);
    assert!(pixels(&mut app) == gamepad);
    app.world_mut().write_message(TouchInput {
        phase: TouchPhase::Started,
        position: Vec2::new(4.0, 4.0),
        window,
        force: None,
        id: 1,
    });
    app.update();
    let pointer = app.world().get::<Window>(window).unwrap().cursor_position();
    assert!(pointer.is_some());
    assert!(
        pixels(&mut app) == gamepad,
        "touch-emulated mouse input must not set the override"
    );
}

#[test]
fn disabled_touch_controls_do_not_move_gameplay() {
    let (mut app, _, window) = host();
    app.insert_resource(UiRuntimeResource(client_ui::ui_runtime::UiRuntime::new(1)));
    app.world_mut()
        .resource_mut::<UiPresentationResource>()
        .enable_touch_controls(false);
    let viewport = app.world().get::<Window>(window).unwrap();
    let position = Vec2::new(viewport.width() * 0.1, viewport.height() * 0.9);
    for phase in [TouchPhase::Started, TouchPhase::Moved] {
        app.world_mut().write_message(TouchInput {
            phase,
            position,
            window,
            force: None,
            id: 1,
        });
        app.update();
        assert_eq!(input(&app).movement, [0.0; 2]);
    }
}

#[test]
fn settings_reissue_keeps_mouse_override_without_new_input() {
    let (mut app, pad, window) = host();
    let entry = app
        .world()
        .resource::<UiRuntimeResource>()
        .server_forms()
        .active()
        .unwrap()
        .clone();
    let event = protocol::FormRequestEvent {
        form_id: entry.form_id,
        kind: protocol::FormKind::ServerSettings,
        title: entry.title,
        json: "{}".into(),
        model: entry.model,
    };
    {
        let mut runtime = app.world_mut().resource_mut::<UiRuntimeResource>();
        let session = runtime.session_id();
        let store = runtime.server_forms_mut();
        store.select_settings_form(true);
        store.admit(event.clone(), 2, session, false);
    }
    let mouse = pixels(&mut app);
    controller(&mut app, pad, 0.8);
    let gamepad = pixels(&mut app);
    assert!(gamepad != mouse, "controller use must reveal the image");
    app.world_mut()
        .write_message(WindowEvent::CursorMoved(CursorMoved {
            window,
            position: Vec2::ZERO,
            delta: Some(Vec2::X),
        }));
    app.update();
    assert!(pixels(&mut app) == mouse);
    {
        let mut runtime = app.world_mut().resource_mut::<UiRuntimeResource>();
        let session = runtime.session_id();
        runtime.server_forms_mut().admit(event, 3, session, false);
    }
    assert!(
        pixels(&mut app) == mouse,
        "reissuing the form must retain the override without another pointer event"
    );
}
