//! The platform Back key or gesture acts as the mouse Back button every screen already handles.

use bevy::{
    input::{
        ButtonState,
        keyboard::{Key, KeyboardInput},
    },
    prelude::{ButtonInput, MessageReader, MouseButton, ResMut},
};

/// Whether a key event is the platform Back key, which Android also sends for its Back gesture.
pub(crate) fn is_platform_back(input: &KeyboardInput) -> bool {
    input.logical_key == Key::BrowserBack
}

/// Presses mouse Back while the platform Back key is down, so forms, menus and other screens
/// share one cancel route. Runs after input collection; key repeats press nothing. Chat and
/// inventories route the key itself at its place among the frame's keys.
pub(crate) fn press_mouse_back_for_platform_back(
    mut keyboard: MessageReader<KeyboardInput>,
    mut buttons: ResMut<ButtonInput<MouseButton>>,
) {
    for input in keyboard.read() {
        if !is_platform_back(input) {
            continue;
        }
        match input.state {
            ButtonState::Pressed if !input.repeat => buttons.press(MouseButton::Back),
            ButtonState::Pressed => {}
            ButtonState::Released => buttons.release(MouseButton::Back),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use bevy::{
        input::{InputPlugin, InputSystems, keyboard::NativeKeyCode},
        prelude::*,
    };

    fn back(window: Entity, state: ButtonState, repeat: bool) -> KeyboardInput {
        KeyboardInput {
            key_code: KeyCode::Unidentified(NativeKeyCode::Android(4)),
            logical_key: Key::BrowserBack,
            state,
            text: None,
            repeat,
            window,
        }
    }

    /// Android's Back is pressed once as mouse Back, ignores repeats, and lifts with the key.
    #[test]
    fn platform_back_presses_mouse_back_once() {
        let mut app = App::new();
        app.add_plugins((MinimalPlugins, InputPlugin)).add_systems(
            PreUpdate,
            press_mouse_back_for_platform_back.after(InputSystems),
        );
        let window = app.world_mut().spawn_empty().id();
        app.world_mut()
            .write_message(back(window, ButtonState::Pressed, false));
        app.update();
        let buttons = app.world().resource::<ButtonInput<MouseButton>>();
        assert!(buttons.just_pressed(MouseButton::Back));
        app.world_mut()
            .write_message(back(window, ButtonState::Pressed, true));
        app.update();
        let buttons = app.world().resource::<ButtonInput<MouseButton>>();
        assert!(buttons.pressed(MouseButton::Back));
        assert!(!buttons.just_pressed(MouseButton::Back));
        app.world_mut()
            .write_message(back(window, ButtonState::Released, false));
        app.update();
        assert!(
            !app.world()
                .resource::<ButtonInput<MouseButton>>()
                .pressed(MouseButton::Back)
        );
    }
}
