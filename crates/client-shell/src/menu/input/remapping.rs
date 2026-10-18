//! Binding capture consumes input before normal menu navigation can handle it.

use crate::menu::MenuRuntime;
use crate::physical_input::{TRANSLATED_GAMEPAD_BUTTONS, keyboard_usage, mouse_button_code};
use bevy::{
    ecs::message::MessageReader,
    input::{
        ButtonState,
        gamepad::{Gamepad, GamepadButton},
        keyboard::{Key, KeyboardInput},
    },
    prelude::{ButtonInput, KeyCode, MouseButton, Query},
};
use launcher::menu::settings_options::{GAMEPAD_OFFSET, macro_row};
use semantic_input::{AxisDirection, PhysicalControl};

/// Remembers characters from presses whose logical key is already unmodified.
pub(super) fn observe_label(menu: &mut MenuRuntime, input: &KeyboardInput) {
    if input.state != ButtonState::Pressed {
        return;
    }
    let Some(usage) = keyboard_usage(input.key_code) else {
        return;
    };
    match &input.logical_key {
        Key::Character(text) => {
            std::sync::Arc::make_mut(&mut menu.settings_options).set_key_label(usage, text);
        }
        Key::Dead(Some(character)) => {
            std::sync::Arc::make_mut(&mut menu.settings_options)
                .set_key_label(usage, &character.to_string());
        }
        _ => {}
    }
}

/// Captures the selected device; Escape clears a keyboard row or cancels other captures.
pub(super) fn capture(
    menu: &mut MenuRuntime,
    messages: &mut MessageReader<KeyboardInput>,
    keys: &mut ButtonInput<KeyCode>,
    mouse: &mut ButtonInput<MouseButton>,
    gamepads: &Query<&Gamepad>,
) {
    let pending = menu.settings.key_remap.map(usize::from);
    let macro_slot = pending.and_then(macro_row);
    // Macro slots bind only desktop controls; the selected modifier applies at runtime.
    let gamepad = pending.is_some_and(|index| index >= GAMEPAD_OFFSET) && macro_slot.is_none();
    for input in messages.read() {
        if input.state != ButtonState::Pressed {
            continue;
        }
        if input.key_code == KeyCode::Escape {
            menu.capture_escape();
            break;
        }
        if !gamepad && let Some(code) = keyboard_usage(input.key_code) {
            menu.capture_key(PhysicalControl::KeyboardUsage(code));
            break;
        }
    }
    if gamepad && menu.settings.key_remap.is_some() {
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
    } else if menu.settings.key_remap.is_some()
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
        prelude::{App, Entity, IntoScheduleConfigs, ResMut, Update},
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
        if menu.settings.key_remap.is_some() {
            capture(&mut menu, &mut messages, &mut keys, &mut mouse, &gamepads);
        } else {
            messages.clear();
        }
    }

    /// Runs passive label learning, followed by optional physical binding capture.
    fn label_input_app(capture: bool) -> App {
        let mut app = App::new();
        app.insert_resource(MenuRuntime::new(false, 2, "Bindings".into()))
            .add_message::<KeyboardInput>()
            .init_resource::<ButtonInput<KeyCode>>()
            .init_resource::<ButtonInput<MouseButton>>()
            .add_systems(Update, super::super::context::observe_keyboard_labels);
        if capture {
            app.add_systems(
                Update,
                capture_input.after(super::super::context::observe_keyboard_labels),
            );
        }
        app
    }

    /// Starts a hotbar remap after the modifier has already been pressed.
    fn start_label_capture(app: &mut App) -> usize {
        let row = KEY_BINDINGS
            .iter()
            .position(|(action, _)| *action == Action::Hotbar1)
            .unwrap();
        app.world_mut()
            .resource_mut::<MenuRuntime>()
            .settings
            .key_remap = Some(row as u16);
        row
    }

    /// Queues a logical key event without needing a native window or keyboard.
    fn label_event(app: &mut App, key_code: KeyCode, logical_key: Key, state: ButtonState) {
        app.world_mut().write_message(KeyboardInput {
            key_code,
            logical_key,
            state,
            text: None,
            repeat: false,
            window: Entity::PLACEHOLDER,
        });
    }

    #[test]
    fn shift_combinations_do_not_replace_keyboard_labels() {
        for capture in [false, true] {
            let mut app = label_input_app(capture);
            label_event(
                &mut app,
                KeyCode::ShiftLeft,
                Key::Shift,
                ButtonState::Pressed,
            );
            app.update();
            let row = capture.then(|| start_label_capture(&mut app));
            label_event(
                &mut app,
                KeyCode::Digit1,
                Key::Character("!".into()),
                ButtonState::Pressed,
            );
            label_event(
                &mut app,
                KeyCode::ShiftLeft,
                Key::Shift,
                ButtonState::Released,
            );
            app.update();
            let menu = app.world().resource::<MenuRuntime>();
            let control = PhysicalControl::KeyboardUsage(0x1e);
            assert_eq!(menu.settings_options.key_name(control), "1");
            if let Some(row) = row {
                assert_eq!(menu.settings_options.key_control(row), Some(control));
            }
        }
    }

    #[test]
    fn altgr_and_control_combinations_do_not_replace_keyboard_labels() {
        for modifier in [KeyCode::AltRight, KeyCode::ControlLeft] {
            for capture in [false, true] {
                let mut app = label_input_app(capture);
                label_event(
                    &mut app,
                    modifier,
                    Key::Unidentified(NativeKey::Unidentified),
                    ButtonState::Pressed,
                );
                app.update();
                let row = capture.then(|| start_label_capture(&mut app));
                label_event(
                    &mut app,
                    KeyCode::Digit2,
                    Key::Character("@".into()),
                    ButtonState::Pressed,
                );
                label_event(
                    &mut app,
                    modifier,
                    Key::Unidentified(NativeKey::Unidentified),
                    ButtonState::Released,
                );
                app.update();
                let menu = app.world().resource::<MenuRuntime>();
                let control = PhysicalControl::KeyboardUsage(0x1f);
                assert_eq!(menu.settings_options.key_name(control), "2");
                if let Some(row) = row {
                    assert_eq!(menu.settings_options.key_control(row), Some(control));
                }
            }
        }
    }

    #[test]
    fn unmodified_azerty_press_refreshes_the_passive_label() {
        let mut app = label_input_app(false);
        label_event(
            &mut app,
            KeyCode::KeyQ,
            Key::Character("a".into()),
            ButtonState::Pressed,
        );
        app.update();
        assert_eq!(
            app.world()
                .resource::<MenuRuntime>()
                .settings_options
                .key_name(PhysicalControl::KeyboardUsage(0x14)),
            "A"
        );
    }

    #[test]
    fn azerty_capture_displays_the_logical_letter_and_saves_the_physical_key() {
        let row = KEY_BINDINGS
            .iter()
            .position(|(action, _)| *action == Action::Hotbar1)
            .unwrap();
        let mut menu = MenuRuntime::new(false, 2, "Bindings".into());
        menu.settings.key_remap = Some(row as u16);
        let mut app = App::new();
        app.insert_resource(menu)
            .add_message::<KeyboardInput>()
            .init_resource::<ButtonInput<KeyCode>>()
            .init_resource::<ButtonInput<MouseButton>>()
            .add_systems(
                Update,
                (
                    super::super::context::observe_keyboard_labels,
                    capture_input,
                )
                    .chain(),
            );
        app.world_mut().write_message(KeyboardInput {
            key_code: KeyCode::KeyQ,
            logical_key: Key::Character("a".into()),
            state: ButtonState::Pressed,
            text: Some("a".into()),
            repeat: false,
            window: Entity::PLACEHOLDER,
        });
        app.update();
        let menu = app.world().resource::<MenuRuntime>();
        let control = menu.settings_options.key_control(row).unwrap();
        assert_eq!(control, PhysicalControl::KeyboardUsage(0x14));
        assert_eq!(menu.settings_options.key_name(control), "A");
        let loaded: SettingsOptions =
            serde_json::from_str(&serde_json::to_string(&menu.settings_options).unwrap()).unwrap();
        assert_eq!(loaded.key_control(row), Some(control));
    }

    #[test]
    fn ordinary_key_presses_refresh_labels_without_dirtying_settings() {
        let mut menu = MenuRuntime::new(false, 2, "Bindings".into());
        let saved = serde_json::to_value(&menu.settings_options).unwrap();
        let initial_flags = (menu.settings_dirty, menu.settings_apply);
        let mut input = KeyboardInput {
            key_code: KeyCode::KeyQ,
            logical_key: Key::Character("a".into()),
            state: ButtonState::Pressed,
            text: Some("a".into()),
            repeat: false,
            window: Entity::PLACEHOLDER,
        };
        observe_label(&mut menu, &input);
        let q = PhysicalControl::KeyboardUsage(0x14);
        assert_eq!(menu.settings_options.key_name(q), "A");
        input.state = ButtonState::Released;
        input.logical_key = Key::Character("q".into());
        observe_label(&mut menu, &input);
        assert_eq!(menu.settings_options.key_name(q), "A");
        input.state = ButtonState::Pressed;
        observe_label(&mut menu, &input);
        assert_eq!(menu.settings_options.key_name(q), "Q");
        input.key_code = KeyCode::Quote;
        input.logical_key = Key::Dead(Some('^'));
        observe_label(&mut menu, &input);
        assert_eq!(
            menu.settings_options
                .key_name(PhysicalControl::KeyboardUsage(0x34)),
            "^"
        );
        assert_eq!((menu.settings_dirty, menu.settings_apply), initial_flags);
        assert_eq!(serde_json::to_value(&menu.settings_options).unwrap(), saved);
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
        let desktop = crate::physical_input::DESKTOP_KEYS
            .iter()
            .filter_map(|(key_code, key)| Some((*key_code, key.usage()?)));
        for (key_code, usage) in [
            (KeyCode::CapsLock, 0x39),
            (KeyCode::NumLock, 0x53),
            (KeyCode::ScrollLock, 0x47),
        ]
        .into_iter()
        .chain(desktop)
        {
            if key_code == KeyCode::Escape {
                continue;
            }
            app.world_mut()
                .resource_mut::<MenuRuntime>()
                .settings
                .key_remap = Some(row as u16);
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
            assert_eq!(menu.settings.key_remap, None, "{key_code:?}");
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

    #[test]
    fn escape_unbinds_selected_keyboard_action_and_persists_it() {
        let directory = tempfile::tempdir().unwrap();
        let path = directory.path().join("settings.json");
        let jump = KEY_BINDINGS
            .iter()
            .position(|(action, _)| *action == Action::Jump)
            .unwrap();
        let sneak = KEY_BINDINGS
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
        {
            let mut menu = app.world_mut().resource_mut::<MenuRuntime>();
            menu.activate(launcher::menu::MenuAction::Navigate(
                launcher::menu::MenuScreen::Settings,
            ));
            menu.activate_settings(launcher::menu::MenuAction::SettingsKey(jump as u16));
            menu.activate_settings(launcher::menu::MenuAction::SettingsKey(sneak as u16));
        }
        app.world_mut().write_message(KeyboardInput {
            key_code: KeyCode::Escape,
            logical_key: Key::Escape,
            state: ButtonState::Pressed,
            text: None,
            repeat: false,
            window: Entity::PLACEHOLDER,
        });
        app.update();
        let menu = app.world().resource::<MenuRuntime>();
        assert_eq!(menu.settings.key_remap, None);
        assert_eq!(menu.screen(), launcher::menu::MenuScreen::Settings);
        assert!(menu.settings_dirty && menu.settings_apply);
        assert_eq!(menu.settings_options.key_control(sneak), None);
        menu.settings_options.save(&path).unwrap();
        let loaded = SettingsOptions::load(&path);
        assert_eq!(loaded.key_control(sneak), None);
        assert_eq!(
            loaded.controls().unwrap().bindings(),
            original
                .bindings()
                .iter()
                .copied()
                .filter(|binding| {
                    binding.action != Action::Sneak
                        || !matches!(
                            binding.chord.control,
                            PhysicalControl::KeyboardUsage(_) | PhysicalControl::MouseButton(_)
                        )
                })
                .collect::<Vec<_>>()
        );
    }

    #[test]
    fn escape_outside_capture_keeps_bindings_and_leaves_settings() {
        let mut menu = MenuRuntime::new(false, 2, "Bindings".into());
        menu.activate(launcher::menu::MenuAction::Navigate(
            launcher::menu::MenuScreen::Settings,
        ));
        let original = menu.settings_options.clone();
        assert!(!menu.is_remapping_key());
        menu.go_back_from_input();
        assert_ne!(menu.screen(), launcher::menu::MenuScreen::Settings);
        assert_eq!(menu.settings_options, original);
    }

    #[test]
    fn escape_cancels_gamepad_and_macro_capture_without_unbinding() {
        for row in [
            GAMEPAD_OFFSET,
            launcher::menu::settings_options::macro_row_index(0),
        ] {
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
                .clone();
            let original_flags = {
                let menu = app.world().resource::<MenuRuntime>();
                (menu.settings_dirty, menu.settings_apply)
            };
            app.world_mut()
                .resource_mut::<MenuRuntime>()
                .activate_settings(launcher::menu::MenuAction::SettingsKey(row as u16));
            app.world_mut().write_message(KeyboardInput {
                key_code: KeyCode::Escape,
                logical_key: Key::Escape,
                state: ButtonState::Pressed,
                text: None,
                repeat: false,
                window: Entity::PLACEHOLDER,
            });
            app.update();
            let menu = app.world().resource::<MenuRuntime>();
            assert_eq!(menu.settings.key_remap, None);
            assert_eq!(menu.settings_options, original);
            assert_eq!((menu.settings_dirty, menu.settings_apply), original_flags);
        }
    }
}
