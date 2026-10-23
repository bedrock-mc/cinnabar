use bevy::{
    input::{
        ButtonState,
        keyboard::{Key, KeyboardInput},
        mouse::AccumulatedMouseMotion,
    },
    prelude::{App, ButtonInput, KeyCode, MouseButton, Update, Window},
    window::{CursorOptions, PrimaryWindow},
};
use client_shell::ui_resources::{UiPresentationResource, UiRuntimeResource};
use client_ui::ui_runtime::{
    SequencedUiEvent, UiRuntime, presentation::UiPresentationRuntime, sign_editor::SignEdit,
};
use protocol::{ddui::ScriptedScreen, test_support::ddui};
use ui_engine::test_support::fixture_font;

use super::drive_sign_editor;

#[test]
fn a_data_driven_form_keeps_typing_from_the_sign_beneath_it() {
    let mut runtime = UiRuntime::new(1);
    let mut player = player_state::PlayerState::new(1);
    let events = ddui::show_events(
        ScriptedScreen::CustomForm,
        4,
        7,
        ddui::custom_form_property(),
    );
    for (sequence, event) in (1..).zip(events) {
        runtime
            .apply(
                &mut player,
                SequencedUiEvent {
                    session_id: 1,
                    fifo_sequence: sequence,
                    local_millis: sequence * 10,
                    server_tick: None,
                    event,
                },
            )
            .unwrap();
    }
    // A sign edit that opened while the form shows.
    runtime
        .sign_editor_mut()
        .open(SignEdit::new([0, 64, 0], true, Default::default()));
    let mut app = App::new();
    app.add_message::<KeyboardInput>()
        .init_resource::<ButtonInput<KeyCode>>()
        .init_resource::<ButtonInput<MouseButton>>()
        .init_resource::<AccumulatedMouseMotion>()
        .init_resource::<crate::runtime::world::ClientWorld>()
        .insert_resource(client_state::PlayerRuntime(player))
        .insert_resource(UiRuntimeResource(runtime))
        .insert_resource(UiPresentationResource(
            UiPresentationRuntime::new(fixture_font()).unwrap(),
        ))
        .add_systems(Update, drive_sign_editor);
    let window = app
        .world_mut()
        .spawn((Window::default(), CursorOptions::default(), PrimaryWindow))
        .id();
    app.world_mut().write_message(KeyboardInput {
        key_code: KeyCode::KeyA,
        logical_key: Key::Character("a".into()),
        state: ButtonState::Pressed,
        text: Some("a".into()),
        repeat: false,
        window,
    });
    app.update();
    let runtime = app.world().resource::<UiRuntimeResource>();
    let sign = runtime.sign_editor().active().expect("the sign stays open");
    assert!(
        sign.text().is_empty(),
        "the form's typing left the sign alone"
    );
}
