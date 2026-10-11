//! Binding capture consumes input before normal menu navigation can handle it.

use crate::menu::MenuRuntime;
use bevy::{
    ecs::message::MessageReader,
    input::{
        ButtonState,
        gamepad::{Gamepad, GamepadButton},
        keyboard::KeyboardInput,
    },
    prelude::{ButtonInput, KeyCode, MouseButton, Query},
};
use client_ui::physical_input::{TRANSLATED_GAMEPAD_BUTTONS, keyboard_usage, mouse_button_code};
use launcher::menu::settings_options::{GAMEPAD_OFFSET, macro_row};
use semantic_input::{AxisDirection, PhysicalControl};

/// Captures only the selected device family and allows Escape to cancel either family.
pub(super) fn capture(
    menu: &mut MenuRuntime,
    messages: &mut MessageReader<KeyboardInput>,
    keys: &mut ButtonInput<KeyCode>,
    mouse: &mut ButtonInput<MouseButton>,
    gamepads: &Query<&Gamepad>,
) {
    let pending = menu.key_remap.map(usize::from);
    let macro_slot = pending.and_then(macro_row);
    // Macro slots bind only desktop controls; Alt is applied when the macro runs.
    let gamepad = pending.is_some_and(|index| index >= GAMEPAD_OFFSET) && macro_slot.is_none();
    for input in messages.read() {
        if input.state != ButtonState::Pressed {
            continue;
        }
        if input.key_code == KeyCode::Escape {
            menu.key_remap = None;
            break;
        }
        if !gamepad && let Some(code) = keyboard_usage(input.key_code) {
            menu.capture_key(PhysicalControl::KeyboardUsage(code));
            break;
        }
    }
    if gamepad && menu.key_remap.is_some() {
        for pad in gamepads {
            let button = TRANSLATED_GAMEPAD_BUTTONS
                .iter()
                .find(|(_, button)| pad.just_pressed(*button))
                .map(|(code, _)| PhysicalControl::GamepadButton(*code));
            let trigger = [
                (GamepadButton::LeftTrigger2, 4),
                (GamepadButton::RightTrigger2, 5),
            ]
            .into_iter()
            .find(|(button, _)| pad.just_pressed(*button))
            .map(|(_, axis)| PhysicalControl::GamepadAxis {
                axis,
                direction: AxisDirection::Positive,
            });
            if let Some(control) = button.or(trigger) {
                menu.capture_key(control);
                break;
            }
        }
    } else if menu.key_remap.is_some()
        && let Some(code) = mouse
            .get_just_pressed()
            .find_map(|button| mouse_button_code(*button))
    {
        menu.capture_key(PhysicalControl::MouseButton(code));
    }
    keys.reset_all();
    mouse.reset_all();
}

#[cfg(test)]
mod tests {
    use super::*;
    use bevy::{
        input::keyboard::{Key, NativeKey},
        prelude::{App, Entity, ResMut, Update},
    };
    use launcher::menu::settings_options::{
        KEY_BINDINGS, SettingsOptions, control_bindings::is_gamepad,
    };
    use semantic_input::{Action, InputContext};

    /// Runs the production capture adapter without loading UI carriers.
    fn capture_input(
        mut menu: ResMut<MenuRuntime>,
        mut messages: MessageReader<KeyboardInput>,
        mut keys: ResMut<ButtonInput<KeyCode>>,
        mut mouse: ResMut<ButtonInput<MouseButton>>,
        gamepads: Query<&Gamepad>,
    ) {
        capture(&mut menu, &mut messages, &mut keys, &mut mouse, &gamepads);
    }

    #[test]
    fn mapped_keys_capture_save_and_reload_without_changing_other_bindings() {
        let directory = tempfile::tempdir().unwrap();
        let path = directory.path().join("settings.json");
        let row = KEY_BINDINGS
            .iter()
            .position(|(action, _)| *action == Action::Sneak)
            .unwrap();
        let mut app = App::new();
        app.insert_resource(MenuRuntime::new(false, 2, "Bindings".into()))
            .add_message::<KeyboardInput>()
            .init_resource::<ButtonInput<KeyCode>>()
            .init_resource::<ButtonInput<MouseButton>>()
            .add_systems(Update, capture_input);
        let original = app
            .world()
            .resource::<MenuRuntime>()
            .settings_options
            .controls()
            .unwrap();
        for &(key_code, usage) in [
            (KeyCode::CapsLock, 0x39),
            (KeyCode::NumLock, 0x53),
            (KeyCode::ScrollLock, 0x47),
        ]
        .iter()
        .chain(client_ui::physical_input::KEYBOARD_USAGES)
        {
            if key_code == KeyCode::Escape {
                continue;
            }
            app.world_mut().resource_mut::<MenuRuntime>().key_remap = Some(row as u16);
            app.world_mut().write_message(KeyboardInput {
                key_code,
                logical_key: Key::Unidentified(NativeKey::Unidentified),
                state: ButtonState::Pressed,
                text: None,
                repeat: false,
                window: Entity::PLACEHOLDER,
            });
            app.update();
            let menu = app.world().resource::<MenuRuntime>();
            assert_eq!(menu.key_remap, None, "{key_code:?}");
            assert!(menu.settings_dirty && menu.settings_apply);
            menu.settings_options.save(&path).unwrap();
            let loaded = SettingsOptions::load(&path);
            assert_eq!(
                loaded.key_control(row),
                Some(PhysicalControl::KeyboardUsage(usage))
            );
            let controls = loaded.controls().unwrap();
            assert!(controls.bindings().iter().any(|binding| {
                binding.action == Action::Sneak
                    && binding.chord.control == PhysicalControl::KeyboardUsage(usage)
            }));
            let mut runtime = semantic_input::SemanticInputRouter::default();
            runtime.replace_bindings(controls.clone()).unwrap();
            for (activity_sequence, held) in (1..).zip([true, true, false, false, true, false]) {
                runtime
                    .route(semantic_input::DeviceFrame {
                        keyboard_mouse: Some(semantic_input::KeyboardMouseFrame {
                            activity_sequence,
                            keys: if held { vec![usage] } else { vec![] },
                            ..Default::default()
                        }),
                        ..Default::default()
                    })
                    .unwrap();
                let snapshot = runtime.finalize().unwrap();
                assert_eq!(
                    snapshot.phases[Action::Sneak as usize].held,
                    held,
                    "{key_code:?}"
                );
            }
            let unchanged = |settings: &semantic_input::ControlSettings| {
                settings
                    .bindings()
                    .iter()
                    .copied()
                    .filter(|binding| {
                        binding.action != Action::Sneak
                            || binding.context != InputContext::Gameplay
                            || is_gamepad(binding.chord.control)
                    })
                    .collect::<Vec<_>>()
            };
            assert_eq!(unchanged(&controls), unchanged(&original));
        }
    }
}
