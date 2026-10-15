//! Consent owns the whole input frame, including the frame that dismisses it.

use bevy::{
    input::{
        gamepad::{
            Gamepad, GamepadAxisChangedEvent, GamepadButtonChangedEvent,
            GamepadButtonStateChangedEvent,
        },
        keyboard::KeyboardInput,
        mouse::{MouseButtonInput, MouseWheel},
        touch::{TouchInput, Touches},
    },
    prelude::*,
};

use client_state::ConsentInput;

/// Keyboard, mouse button, wheel and touch message queues consent clears when installed.
type PointerMessages<'w> = (
    Option<ResMut<'w, Messages<KeyboardInput>>>,
    Option<ResMut<'w, Messages<MouseButtonInput>>>,
    Option<ResMut<'w, Messages<MouseWheel>>>,
    Option<ResMut<'w, Messages<TouchInput>>>,
);

/// Gamepad button, button-state and axis message queues consent clears when installed.
type GamepadMessages<'w> = (
    Option<ResMut<'w, Messages<GamepadButtonChangedEvent>>>,
    Option<ResMut<'w, Messages<GamepadButtonStateChangedEvent>>>,
    Option<ResMut<'w, Messages<GamepadAxisChangedEvent>>>,
);

/// Removes queued physical events after consent reads them and before ordinary UI runs.
///
/// Declares exactly the input it clears instead of taking the whole world, so frames without
/// consent do not stall the parallel executor; optional streams are cleared when installed.
pub fn consume(
    consent: Res<ConsentInput>,
    keys: Option<ResMut<ButtonInput<KeyCode>>>,
    mouse: Option<ResMut<ButtonInput<MouseButton>>>,
    pointer_messages: PointerMessages,
    gamepad_messages: GamepadMessages,
    touches: Option<ResMut<Touches>>,
    mut gamepads: Query<&mut Gamepad>,
) {
    if !consent.0 {
        return;
    }
    keys.expect("consent input requires keyboard button state")
        .reset_all();
    mouse
        .expect("consent input requires mouse button state")
        .reset_all();
    let (keyboard, mouse_buttons, wheel, touch) = pointer_messages;
    clear_messages(keyboard);
    clear_messages(mouse_buttons);
    clear_messages(wheel);
    clear_messages(touch);
    let (gamepad_buttons, gamepad_button_states, gamepad_axes) = gamepad_messages;
    clear_messages(gamepad_buttons);
    clear_messages(gamepad_button_states);
    clear_messages(gamepad_axes);
    if let Some(mut touches) = touches {
        touches.reset_all();
    }
    for mut gamepad in &mut gamepads {
        gamepad.digital_mut().reset_all();
    }
}

/// Discards one event stream owned by the consent frame when that stream is installed.
fn clear_messages<T: Message>(messages: Option<ResMut<Messages<T>>>) {
    if let Some(mut messages) = messages {
        messages.clear();
    }
}

/// Keeps ordinary UI readers from observing controller and touch input owned by consent.
pub fn ordinary_input(consent: Option<Res<ConsentInput>>) -> bool {
    !consent.is_some_and(|consent| consent.0)
}

#[cfg(test)]
mod tests {
    use super::*;
    use bevy::input::ButtonState;

    #[derive(Resource, Default)]
    struct Clicks(usize);

    /// Models an ordinary form reader skipped for the consent dismissal frame.
    fn read_clicks(mut messages: MessageReader<MouseButtonInput>, mut clicks: ResMut<Clicks>) {
        clicks.0 += messages.read().count();
    }

    #[test]
    fn review_consent_clears_touch_and_controller_events_before_readers_resume() {
        use bevy::input::{
            gamepad::{GamepadButton, GamepadButtonChangedEvent},
            touch::{TouchInput, TouchPhase},
        };
        let mut app = App::new();
        app.init_resource::<ButtonInput<KeyCode>>()
            .init_resource::<ButtonInput<MouseButton>>()
            .insert_resource(ConsentInput(true))
            .add_message::<TouchInput>()
            .add_message::<GamepadButtonChangedEvent>();
        let entity = app.world_mut().spawn_empty().id();
        app.world_mut().write_message(TouchInput {
            phase: TouchPhase::Started,
            position: Vec2::ZERO,
            window: entity,
            force: None,
            id: 1,
        });
        app.world_mut()
            .write_message(GamepadButtonChangedEvent::new(
                entity,
                GamepadButton::South,
                ButtonState::Pressed,
                1.0,
            ));
        bevy::ecs::system::RunSystemOnce::run_system_once(app.world_mut(), consume)
            .expect("consent consume runs");
        assert!(app.world().resource::<Messages<TouchInput>>().is_empty());
        assert!(
            app.world()
                .resource::<Messages<GamepadButtonChangedEvent>>()
                .is_empty()
        );
    }

    #[test]
    fn dismissal_click_cannot_replay_when_ordinary_readers_resume() {
        let mut app = App::new();
        app.init_resource::<ButtonInput<KeyCode>>()
            .init_resource::<ButtonInput<MouseButton>>()
            .init_resource::<Clicks>()
            .insert_resource(ConsentInput(true))
            .add_message::<MouseButtonInput>()
            .add_systems(
                Update,
                (consume, read_clicks.run_if(ordinary_input)).chain(),
            );
        let window = app.world_mut().spawn_empty().id();
        app.world_mut()
            .resource_mut::<ButtonInput<KeyCode>>()
            .press(KeyCode::F9);
        app.world_mut()
            .resource_mut::<ButtonInput<MouseButton>>()
            .press(MouseButton::Left);
        app.world_mut().write_message(MouseButtonInput {
            button: MouseButton::Left,
            state: ButtonState::Pressed,
            window,
        });
        app.update();
        assert_eq!(app.world().resource::<Clicks>().0, 0);
        assert!(
            app.world()
                .resource::<Messages<MouseButtonInput>>()
                .is_empty()
        );
        app.world_mut().resource_mut::<ConsentInput>().0 = false;
        app.update();
        assert_eq!(app.world().resource::<Clicks>().0, 0);
        app.world_mut().write_message(MouseButtonInput {
            button: MouseButton::Left,
            state: ButtonState::Pressed,
            window,
        });
        app.update();
        assert_eq!(app.world().resource::<Clicks>().0, 1);
    }
}
