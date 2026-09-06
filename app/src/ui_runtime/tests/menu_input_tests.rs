use bevy::{
    input::{keyboard::KeyboardInput, mouse::AccumulatedMouseMotion},
    prelude::*,
    time::Real,
    window::{CursorOptions, PrimaryWindow},
};

use crate::{menu::MenuRuntime, ui_runtime::UiRuntime};

#[test]
fn chat_input_preserves_buttons_for_the_visible_menu() {
    let mut app = App::new();
    app.add_message::<KeyboardInput>()
        .init_resource::<Time<Real>>()
        .init_resource::<ButtonInput<KeyCode>>()
        .init_resource::<ButtonInput<MouseButton>>()
        .init_resource::<AccumulatedMouseMotion>()
        .insert_resource(UiRuntime::new(1))
        .insert_resource(MenuRuntime::new(true, 2, "test".into()))
        .add_systems(Update, super::super::drive_chat_keyboard_input);
    app.world_mut()
        .spawn((Window::default(), CursorOptions::default(), PrimaryWindow));
    app.world_mut()
        .resource_mut::<ButtonInput<KeyCode>>()
        .press(KeyCode::Enter);
    app.world_mut()
        .resource_mut::<ButtonInput<MouseButton>>()
        .press(MouseButton::Left);

    app.update();

    assert!(
        app.world()
            .resource::<ButtonInput<KeyCode>>()
            .just_pressed(KeyCode::Enter)
    );
    assert!(
        app.world()
            .resource::<ButtonInput<MouseButton>>()
            .pressed(MouseButton::Left)
    );
    assert!(!app.world().resource::<UiRuntime>().chat_focused());
}
