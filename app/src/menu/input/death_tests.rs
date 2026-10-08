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
}
