use bevy::{
    ecs::{entity::Entity, system::SystemParam},
    input::{
        gamepad::{Gamepad, GamepadAxis, GamepadButton},
        mouse::AccumulatedMouseMotion,
        touch::Touches,
    },
    prelude::{
        ButtonInput, KeyCode, MouseButton, Query, Res, ResMut, Resource, Single, Window, With,
    },
    window::{CursorOptions, PrimaryWindow},
};
use semantic_input::{
    ControllerFrame, DeviceFrame, KeyboardMouseFrame, ModifierChord, TouchContact,
};

use super::{SemanticInputRuntime, SemanticInputSnapshot, SemanticTouchTargets};
use crate::camera::input_is_active;

#[derive(Resource, Debug, Default)]
pub(crate) struct PendingDeviceFrame(Option<DeviceFrame>);

#[derive(Resource, Debug, Default)]
pub(crate) struct SemanticRouteState {
    routed: bool,
}

#[derive(SystemParam)]
pub(crate) struct SemanticPhysicalInputs<'w, 's> {
    window: Single<'w, 's, (&'static Window, &'static CursorOptions), With<PrimaryWindow>>,
    keys: Res<'w, ButtonInput<KeyCode>>,
    mouse_buttons: Res<'w, ButtonInput<MouseButton>>,
    mouse_motion: Res<'w, AccumulatedMouseMotion>,
    gamepads: Query<'w, 's, (Entity, &'static Gamepad)>,
    touches: Res<'w, Touches>,
    touch_targets: ResMut<'w, SemanticTouchTargets>,
}

pub(crate) fn collect_raw_input(
    inputs: SemanticPhysicalInputs,
    mut pending: ResMut<PendingDeviceFrame>,
) {
    pending.0 = Some(translate_device_frame(inputs));
}

pub(crate) fn route_semantic_input(
    mut pending: ResMut<PendingDeviceFrame>,
    mut runtime: ResMut<SemanticInputRuntime>,
    mut route: ResMut<SemanticRouteState>,
) {
    route.routed = pending
        .0
        .take()
        .is_some_and(|frame| runtime.route_device_frame(frame).is_ok());
}

pub(crate) fn finalize_semantic_input_after_ui_authority(
    mut runtime: ResMut<SemanticInputRuntime>,
    mut route: ResMut<SemanticRouteState>,
    mut published: ResMut<SemanticInputSnapshot>,
) {
    let routed = std::mem::take(&mut route.routed);
    if !routed {
        published.clear();
        return;
    }
    match runtime.finalize_routed_input() {
        Ok(snapshot) => published.replace(snapshot),
        Err(_) => published.clear(),
    }
}

fn translate_device_frame(inputs: SemanticPhysicalInputs) -> DeviceFrame {
    let SemanticPhysicalInputs {
        window,
        keys,
        mouse_buttons,
        mouse_motion,
        gamepads,
        touches,
        mut touch_targets,
    } = inputs;
    let (window, cursor) = window.into_inner();
    if !input_is_active(window, cursor) {
        touch_targets.release_all();
        return DeviceFrame {
            keyboard_mouse: Some(KeyboardMouseFrame::default()),
            window_focus_lost: !window.focused,
            ..DeviceFrame::default()
        };
    }

    let mut keyboard_keys = keys
        .get_pressed()
        .chain(keys.get_just_pressed())
        .filter_map(|key| keyboard_usage(*key))
        .collect::<Vec<_>>();
    keyboard_keys.sort_unstable();
    keyboard_keys.dedup();
    let mut buttons = mouse_buttons
        .get_pressed()
        .filter_map(|button| mouse_button_code(*button))
        .collect::<Vec<_>>();
    buttons.sort_unstable();
    let keyboard_mouse = Some(KeyboardMouseFrame {
        activity_sequence: 0,
        keys: keyboard_keys,
        mouse_buttons: buttons,
        mouse_motion: mouse_motion.delta.to_array(),
        modifiers: ModifierChord {
            shift: keys.pressed(KeyCode::ShiftLeft) || keys.pressed(KeyCode::ShiftRight),
            control: keys.pressed(KeyCode::ControlLeft) || keys.pressed(KeyCode::ControlRight),
            alt: keys.pressed(KeyCode::AltLeft) || keys.pressed(KeyCode::AltRight),
            super_key: keys.pressed(KeyCode::SuperLeft) || keys.pressed(KeyCode::SuperRight),
        },
    });
    let mut controllers = gamepads
        .iter()
        .map(|(entity, gamepad)| ControllerFrame {
            device_id: entity.index().index(),
            activity_sequence: 0,
            axes: [
                gamepad.get(GamepadAxis::LeftStickX).unwrap_or(0.0),
                gamepad.get(GamepadAxis::LeftStickY).unwrap_or(0.0),
                gamepad.get(GamepadAxis::RightStickX).unwrap_or(0.0),
                gamepad.get(GamepadAxis::RightStickY).unwrap_or(0.0),
                gamepad.get(GamepadButton::LeftTrigger2).unwrap_or(0.0),
                gamepad.get(GamepadButton::RightTrigger2).unwrap_or(0.0),
                0.0,
                0.0,
            ],
            buttons: gamepad_button_codes(gamepad),
        })
        .collect::<Vec<_>>();
    controllers.sort_by_key(|controller| controller.device_id);
    let width = window.width().max(1.0);
    let height = window.height().max(1.0);
    touch_targets.retain_active_contacts(touches.iter().map(|touch| touch.id()));
    let mut contacts = touches
        .iter()
        .map(|touch| TouchContact {
            contact_id: touch.id(),
            activity_sequence: 0,
            position: [
                (touch.position().x / width).clamp(0.0, 1.0),
                (touch.position().y / height).clamp(0.0, 1.0),
            ],
            delta: [touch.delta().x / width, touch.delta().y / height],
            hit_id: touch_targets.target(touch.id()),
        })
        .collect::<Vec<_>>();
    contacts.sort_by_key(|touch| touch.contact_id);
    DeviceFrame {
        keyboard_mouse,
        controllers,
        touches: contacts,
        ..DeviceFrame::default()
    }
}

fn keyboard_usage(key: KeyCode) -> Option<u16> {
    Some(match key {
        KeyCode::KeyA => 0x04,
        KeyCode::KeyD => 0x07,
        KeyCode::KeyS => 0x16,
        KeyCode::KeyW => 0x1a,
        KeyCode::Digit1 => 0x1e,
        KeyCode::Digit2 => 0x1f,
        KeyCode::Digit3 => 0x20,
        KeyCode::Digit4 => 0x21,
        KeyCode::Digit5 => 0x22,
        KeyCode::Digit6 => 0x23,
        KeyCode::Digit7 => 0x24,
        KeyCode::Digit8 => 0x25,
        KeyCode::Digit9 => 0x26,
        KeyCode::Escape => 0x29,
        KeyCode::Space => 0x2c,
        KeyCode::F5 => 0x3e,
        KeyCode::ControlLeft => 0xe0,
        KeyCode::ShiftLeft => 0xe1,
        KeyCode::AltLeft => 0xe2,
        KeyCode::SuperLeft => 0xe3,
        KeyCode::ControlRight => 0xe4,
        KeyCode::ShiftRight => 0xe5,
        KeyCode::AltRight => 0xe6,
        KeyCode::SuperRight => 0xe7,
        _ => return None,
    })
}

fn mouse_button_code(button: MouseButton) -> Option<u8> {
    Some(match button {
        MouseButton::Left => 1,
        MouseButton::Right => 2,
        MouseButton::Middle => 3,
        MouseButton::Back => 4,
        MouseButton::Forward => 5,
        MouseButton::Other(code) => u8::try_from(code).ok()?.checked_add(1)?,
    })
}

fn gamepad_button_codes(gamepad: &Gamepad) -> Vec<u8> {
    let mut buttons = [
        (0, GamepadButton::South),
        (1, GamepadButton::East),
        (2, GamepadButton::North),
        (3, GamepadButton::West),
        (4, GamepadButton::LeftTrigger),
        (5, GamepadButton::RightTrigger),
        (6, GamepadButton::Select),
        (7, GamepadButton::Start),
        (8, GamepadButton::LeftThumb),
        (9, GamepadButton::RightThumb),
        (11, GamepadButton::DPadUp),
        (12, GamepadButton::DPadDown),
        (13, GamepadButton::DPadLeft),
        (14, GamepadButton::DPadRight),
    ]
    .into_iter()
    .filter_map(|(code, button)| gamepad.pressed(button).then_some(code))
    .collect::<Vec<_>>();
    buttons.sort_unstable();
    buttons
}
