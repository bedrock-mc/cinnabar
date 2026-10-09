//! Authoritative route replacement retires the previous screen's input.
use super::*;
use crate::menu::{MenuAction, MenuScreen};
use bevy::{prelude::*, window::WindowResolution};
use client_ui::ui_runtime::{SequencedUiEvent, UiRuntime};

#[test]
fn death_discards_a_same_frame_pause_settings_click() {
    let mut menu = MenuRuntime::new(false, 2, "Player".into());
    menu.show_world();
    menu.open_pause();
    let mut player = crate::player_runtime::PlayerRuntime::new(1);
    let mut runtime = UiRuntime::new(1);
    let mut presentation =
        UiPresentationRuntime::new(client_ui::test_support::fixture_font()).unwrap();
    presentation.set_menu_view(Some(menu.view()));
    presentation
        .build(
            &player,
            &runtime,
            0,
            [1280, 720],
            ui::DpiScale::new(1.0).unwrap(),
        )
        .unwrap();
    let bounds = presentation
        .menu_action_bounds(MenuAction::PauseSettings)
        .expect("pause settings target");
    let point = Vec2::new(
        (bounds.min().x() + bounds.max().x()) * 0.5,
        (bounds.min().y() + bounds.max().y()) * 0.5,
    );
    runtime
        .apply(
            &mut player,
            SequencedUiEvent {
                session_id: 1,
                fifo_sequence: 1,
                local_millis: 0,
                server_tick: None,
                event: protocol::UiEvent::Hud(protocol::HudEvent::Health { health: 0 }),
            },
        )
        .unwrap();
    let mut app = App::new();
    app.add_plugins(MinimalPlugins)
        .add_message::<KeyboardInput>()
        .add_message::<MouseButtonInput>()
        .init_resource::<ButtonInput<KeyCode>>()
        .init_resource::<ButtonInput<MouseButton>>()
        .init_resource::<Touches>()
        .init_resource::<MenuClipboard>()
        .insert_resource(player)
        .insert_resource(runtime)
        .insert_resource(menu)
        .insert_resource(presentation)
        .add_systems(Update, drive_menu_input);
    let mut window = Window {
        focused: true,
        resolution: WindowResolution::new(1280, 720),
        ..Default::default()
    };
    window.set_cursor_position(Some(point));
    let entity = app
        .world_mut()
        .spawn((window, CursorOptions::default(), PrimaryWindow))
        .id();
    app.world_mut().write_message(MouseButtonInput {
        button: MouseButton::Left,
        state: ButtonState::Pressed,
        window: entity,
    });
    app.world_mut()
        .resource_mut::<ButtonInput<MouseButton>>()
        .press(MouseButton::Left);
    app.update();
    assert_eq!(
        app.world().resource::<MenuRuntime>().screen(),
        MenuScreen::Death
    );
    app.update();
    assert_eq!(
        app.world().resource::<MenuRuntime>().screen(),
        MenuScreen::Death,
        "held old input stays retired"
    );
    app.world_mut()
        .resource_scope(|world, mut runtime: Mut<UiRuntime>| {
            let mut player = world.resource_mut::<crate::player_runtime::PlayerRuntime>();
            runtime
                .apply_local_attributes(
                    &mut player,
                    client_ui::ui_runtime::SequencedLocalAttributes {
                        session_id: 1,
                        fifo_sequence: 2,
                        local_millis: 1,
                        server_tick: 1,
                        attributes: vec![protocol::ActorAttribute {
                            name: "minecraft:health".into(),
                            min: 0.0,
                            max: 20.0,
                            current: 0.001,
                            default: None,
                            modifiers: std::sync::Arc::from([]),
                        }]
                        .into(),
                    },
                )
                .unwrap();
        });
    app.update();
    assert!(!app.world().resource::<MenuRuntime>().is_visible());
}

#[test]
fn death_controls_advance_on_real_time_while_simulation_is_paused() {
    let mut app = App::new();
    app.insert_resource(crate::player_runtime::PlayerRuntime::new(1))
        .add_message::<KeyboardInput>()
        .init_resource::<ButtonInput<KeyCode>>()
        .init_resource::<ButtonInput<MouseButton>>()
        .init_resource::<Touches>()
        .insert_resource(
            UiPresentationRuntime::new(client_ui::test_support::fixture_font()).unwrap(),
        )
        .insert_resource(MenuRuntime::new(false, 2, "Player".into()))
        .insert_resource(MenuClipboard::with_access(|_| None, |_| {}))
        .add_systems(Update, drive_menu_input);
    app.world_mut().spawn((
        Window {
            focused: true,
            ..Default::default()
        },
        CursorOptions::default(),
        PrimaryWindow,
    ));
    app.init_resource::<Time<Real>>()
        .init_resource::<Time<Virtual>>();
    app.world_mut().resource_mut::<Time<Virtual>>().pause();
    {
        let mut menu = app.world_mut().resource_mut::<MenuRuntime>();
        menu.show_world();
        menu.open_death();
    }
    app.update();
    assert!(
        !app.world()
            .resource::<MenuRuntime>()
            .view()
            .death_controls_visible
    );
    app.world_mut()
        .resource_mut::<Time<Real>>()
        .advance_by(std::time::Duration::from_secs_f64(
            crate::menu::death::DEATH_CONTROLS_DELAY_SECONDS + 0.001,
        ));
    app.update();
    assert!(
        app.world()
            .resource::<MenuRuntime>()
            .view()
            .death_controls_visible
    );
    assert!(app.world().resource::<Time<Virtual>>().is_paused());
}

#[test]
fn death_respawn_retains_explicit_cursor_return_until_recovery() {
    use client_presentation::camera::CursorFocus;

    let mut app = App::new();
    app.insert_resource(crate::player_runtime::PlayerRuntime::new(1))
        .add_message::<KeyboardInput>()
        .init_resource::<ButtonInput<KeyCode>>()
        .init_resource::<ButtonInput<MouseButton>>()
        .init_resource::<Touches>()
        .insert_resource(
            UiPresentationRuntime::new(client_ui::test_support::fixture_font()).unwrap(),
        )
        .insert_resource(MenuRuntime::new(false, 2, "Player".into()))
        .insert_resource(MenuClipboard::with_access(|_| None, |_| {}))
        .add_systems(Update, drive_menu_input);
    let window = app
        .world_mut()
        .spawn((Window::default(), CursorOptions::default(), PrimaryWindow))
        .id();
    let mut focus = CursorFocus::default();
    focus.begin_frame(false);
    focus.begin_frame(true);
    focus.record_activation(true);
    app.insert_resource(focus);
    app.world_mut().resource_mut::<MenuRuntime>().open_death();
    let press = |app: &mut App| {
        app.world_mut().write_message(KeyboardInput {
            key_code: KeyCode::Enter,
            logical_key: bevy::input::keyboard::Key::Enter,
            state: ButtonState::Pressed,
            text: None,
            repeat: false,
            window,
        });
        app.update();
    };
    press(&mut app);
    assert!(!app.world().resource::<CursorFocus>().capture_allowed());
    app.world_mut()
        .resource_mut::<MenuRuntime>()
        .advance_death_controls(crate::menu::death::DEATH_CONTROLS_DELAY_SECONDS);
    press(&mut app);
    assert!(app.world().resource::<MenuRuntime>().view().death_loading);
    assert!(app.world().resource::<CursorFocus>().capture_allowed());
    app.world_mut()
        .resource_mut::<CursorFocus>()
        .begin_frame(true);
    app.world_mut()
        .resource_mut::<MenuRuntime>()
        .note_player_alive();
    app.update();
    assert!(!app.world().resource::<MenuRuntime>().is_visible());
    assert!(app.world().resource::<CursorFocus>().capture_allowed());
}
