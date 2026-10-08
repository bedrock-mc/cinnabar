//! Consent owns the whole input frame, including the frame that dismisses it.

use bevy::{
    input::{
        keyboard::KeyboardInput,
        mouse::{MouseButtonInput, MouseWheel},
    },
    prelude::*,
};

#[derive(Resource, Default)]
pub(crate) struct ConsentInput(pub(crate) bool);

/// Removes queued physical events after consent reads them and before ordinary UI runs.
pub(crate) fn consume(world: &mut World) {
    if !world.resource::<ConsentInput>().0 {
        return;
    }
    world.resource_mut::<ButtonInput<KeyCode>>().reset_all();
    world.resource_mut::<ButtonInput<MouseButton>>().reset_all();
    if let Some(mut messages) = world.get_resource_mut::<Messages<KeyboardInput>>() {
        messages.clear();
    }
    if let Some(mut messages) = world.get_resource_mut::<Messages<MouseButtonInput>>() {
        messages.clear();
    }
    if let Some(mut messages) = world.get_resource_mut::<Messages<MouseWheel>>() {
        messages.clear();
    }
    clear_messages::<bevy::input::touch::TouchInput>(world);
    clear_messages::<bevy::input::gamepad::GamepadButtonChangedEvent>(world);
    clear_messages::<bevy::input::gamepad::GamepadButtonStateChangedEvent>(world);
    clear_messages::<bevy::input::gamepad::GamepadAxisChangedEvent>(world);
    if let Some(mut touches) = world.get_resource_mut::<bevy::input::touch::Touches>() {
        touches.reset_all();
    }
    for mut gamepad in world
        .query::<&mut bevy::input::gamepad::Gamepad>()
        .iter_mut(world)
    {
        gamepad.digital_mut().reset_all();
    }
}

/// Discards one event stream owned by the consent frame when that stream is installed.
fn clear_messages<T: Message>(world: &mut World) {
    if let Some(mut messages) = world.get_resource_mut::<Messages<T>>() {
        messages.clear();
    }
}

/// Keeps ordinary UI readers from observing controller and touch input owned by consent.
pub(crate) fn ordinary_input(consent: Option<Res<ConsentInput>>) -> bool {
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
        consume(app.world_mut());
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
