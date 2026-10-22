//! Bevy device state handed between UI screens and gameplay as screens take and release input.
use bevy::{
    input::{ButtonState, mouse::AccumulatedMouseMotion},
    math::Vec2,
    prelude::{ButtonInput, Entity, KeyCode, MouseButton},
    window::{CursorGrabMode, CursorOptions, Ime, WindowEvent},
};
use client_ui::ui_runtime::UiRuntime;

#[cfg(test)]
mod cancel_tests;

pub(crate) fn suppress_gameplay_input_for_inventory(
    runtime: &UiRuntime,
    cursor: &mut CursorOptions,
    keys: &mut ButtonInput<KeyCode>,
    mouse_motion: &mut AccumulatedMouseMotion,
) {
    if !runtime.inventory_open() {
        return;
    }
    cursor.grab_mode = CursorGrabMode::None;
    cursor.visible = true;
    keys.reset_all();
    mouse_motion.delta = Vec2::ZERO;
}

pub(crate) fn restore_gameplay_input_after_chat(
    cursor: &mut CursorOptions,
    keys: &mut ButtonInput<KeyCode>,
    mouse_buttons: &mut ButtonInput<MouseButton>,
    mouse_motion: &mut AccumulatedMouseMotion,
) {
    cursor.grab_mode = CursorGrabMode::Locked;
    cursor.visible = false;
    keys.reset_all();
    mouse_buttons.reset_all();
    mouse_motion.delta = Vec2::ZERO;
}

pub(crate) fn suppress_gameplay_input_for_chat(
    player_runtime: &player_state::PlayerState,
    runtime: &UiRuntime,
    cursor: &mut CursorOptions,
    keys: &mut ButtonInput<KeyCode>,
    mouse_buttons: &mut ButtonInput<MouseButton>,
    mouse_motion: &mut AccumulatedMouseMotion,
) {
    if !runtime.ui_focused(player_runtime) {
        return;
    }
    cursor.grab_mode = CursorGrabMode::None;
    cursor.visible = true;
    keys.reset_all();
    mouse_buttons.reset_all();
    mouse_motion.delta = Vec2::ZERO;
}

/// This frame's mouse presses in arrival order, each with the number of keyboard
/// events that reached the window before it.
pub(crate) fn ordered_pointer_presses<'a>(
    events: impl IntoIterator<Item = &'a WindowEvent>,
) -> Vec<(usize, MouseButton)> {
    // Every event is consumed so the caller's cursor never replays this frame's keys.
    let mut keys = 0;
    let mut presses = Vec::new();
    for event in events {
        match event {
            WindowEvent::KeyboardInput(_) => keys += 1,
            WindowEvent::MouseButtonInput(input) if input.state == ButtonState::Pressed => {
                presses.push((keys, input.button));
            }
            _ => {}
        }
    }
    presses
}

/// Committed platform text for the window, each with the number of keyboard events before it.
pub(crate) fn ordered_ime_commits<'a>(
    events: impl IntoIterator<Item = &'a WindowEvent>,
    window: Entity,
) -> Vec<(usize, &'a str)> {
    let mut keys = 0;
    let mut commits = Vec::new();
    for event in events {
        match event {
            WindowEvent::KeyboardInput(_) => keys += 1,
            WindowEvent::Ime(Ime::Commit {
                window: target,
                value,
            }) if *target == window => commits.push((keys, value.as_str())),
            _ => {}
        }
    }
    commits
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn focused_chat_suppresses_production_gameplay_inputs() {
        let mut player_runtime = player_state::PlayerState::new(1);

        let mut runtime = UiRuntime::new(1);
        runtime.open_chat(&mut player_runtime);
        let mut cursor = CursorOptions {
            grab_mode: CursorGrabMode::Locked,
            visible: false,
            ..CursorOptions::default()
        };
        let mut keys = ButtonInput::<KeyCode>::default();
        keys.press(KeyCode::KeyW);
        let mut mouse_buttons = ButtonInput::<MouseButton>::default();
        mouse_buttons.press(MouseButton::Left);
        let mut mouse_motion = AccumulatedMouseMotion {
            delta: Vec2::new(4.0, 2.0),
        };

        suppress_gameplay_input_for_chat(
            &player_runtime,
            &runtime,
            &mut cursor,
            &mut keys,
            &mut mouse_buttons,
            &mut mouse_motion,
        );

        assert_eq!(cursor.grab_mode, CursorGrabMode::None);
        assert!(cursor.visible);
        assert!(keys.get_pressed().next().is_none());
        assert!(mouse_buttons.get_pressed().next().is_none());
        assert_eq!(mouse_motion.delta, Vec2::ZERO);
    }

    #[test]
    fn closing_chat_immediately_regrabs_and_hides_the_gameplay_cursor() {
        let mut cursor = CursorOptions {
            grab_mode: CursorGrabMode::None,
            visible: true,
            ..CursorOptions::default()
        };
        let mut keys = ButtonInput::<KeyCode>::default();
        keys.press(KeyCode::Enter);
        let mut mouse_buttons = ButtonInput::<MouseButton>::default();
        mouse_buttons.press(MouseButton::Left);
        let mut mouse_motion = AccumulatedMouseMotion {
            delta: Vec2::new(4.0, 2.0),
        };

        restore_gameplay_input_after_chat(
            &mut cursor,
            &mut keys,
            &mut mouse_buttons,
            &mut mouse_motion,
        );

        assert_eq!(cursor.grab_mode, CursorGrabMode::Locked);
        assert!(!cursor.visible);
        assert!(keys.get_pressed().next().is_none());
        assert!(mouse_buttons.get_pressed().next().is_none());
        assert_eq!(mouse_motion.delta, Vec2::ZERO);
    }

    #[test]
    fn window_scan_counts_keys_before_each_press_and_reads_everything() {
        use bevy::{
            ecs::entity::Entity,
            input::{
                keyboard::{Key, KeyboardInput},
                mouse::MouseButtonInput,
            },
        };
        let window = Entity::PLACEHOLDER;
        let key = WindowEvent::KeyboardInput(KeyboardInput {
            key_code: KeyCode::Escape,
            logical_key: Key::Escape,
            state: ButtonState::Pressed,
            text: None,
            repeat: false,
            window,
        });
        let press = |button| {
            WindowEvent::MouseButtonInput(MouseButtonInput {
                button,
                state: ButtonState::Pressed,
                window,
            })
        };
        let events = [
            press(MouseButton::Right),
            key.clone(),
            press(MouseButton::Left),
            key,
        ];
        let mut iter = events.iter();
        assert_eq!(
            ordered_pointer_presses(&mut iter),
            [(0, MouseButton::Right), (1, MouseButton::Left)]
        );
        assert!(iter.next().is_none());
    }
}
