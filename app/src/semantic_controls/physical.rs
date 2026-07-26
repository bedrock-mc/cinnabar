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
    ControllerFrame, DeviceFrame, KeyboardMouseFrame, MAX_CONTROLLERS, MAX_TOUCH_CONTACTS,
    ModifierChord, TouchContact,
};

use super::{SemanticInputRuntime, SemanticInputSnapshot, SemanticTouchTargets};
use crate::camera::input_is_active;

#[derive(Resource, Debug, Default)]
pub(crate) struct PendingDeviceFrame {
    frame: Option<DeviceFrame>,
    ignored_controllers: u64,
    ignored_touches: u64,
}

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
    let translated = translate_device_frame(inputs);
    pending.ignored_controllers = pending
        .ignored_controllers
        .saturating_add(translated.ignored_controllers as u64);
    pending.ignored_touches = pending
        .ignored_touches
        .saturating_add(translated.ignored_touches as u64);
    pending.frame = Some(translated.frame);
}

pub(crate) fn route_semantic_input(
    mut pending: ResMut<PendingDeviceFrame>,
    mut runtime: ResMut<SemanticInputRuntime>,
    mut route: ResMut<SemanticRouteState>,
) {
    route.routed = pending
        .frame
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

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
struct InputSourceGates {
    keyboard_mouse: bool,
    controllers_and_touch: bool,
}

const fn input_source_gates(window_focused: bool, cursor_captured: bool) -> InputSourceGates {
    InputSourceGates {
        keyboard_mouse: window_focused && cursor_captured,
        controllers_and_touch: window_focused,
    }
}

#[derive(Debug)]
struct BoundedSamples<T> {
    samples: Vec<T>,
    ignored: usize,
}

fn select_lowest_by_key<T>(
    samples: impl IntoIterator<Item = T>,
    limit: usize,
    key: impl Fn(&T) -> u64,
) -> BoundedSamples<T> {
    let mut selected = Vec::with_capacity(limit);
    let mut observed = 0_usize;
    for sample in samples {
        observed = observed.saturating_add(1);
        if limit == 0 {
            continue;
        }
        let insertion = selected.partition_point(|existing| key(existing) <= key(&sample));
        if selected.len() < limit {
            selected.insert(insertion, sample);
        } else if insertion < limit {
            selected.pop();
            selected.insert(insertion, sample);
        }
    }
    BoundedSamples {
        ignored: observed.saturating_sub(selected.len()),
        samples: selected,
    }
}

#[derive(Debug)]
struct TranslatedDeviceFrame {
    frame: DeviceFrame,
    ignored_controllers: usize,
    ignored_touches: usize,
}

fn translate_device_frame(inputs: SemanticPhysicalInputs) -> TranslatedDeviceFrame {
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
    let gates = input_source_gates(window.focused, input_is_active(window, cursor));
    if !gates.controllers_and_touch {
        touch_targets.release_all();
        return TranslatedDeviceFrame {
            frame: DeviceFrame {
                window_focus_lost: true,
                ..DeviceFrame::default()
            },
            ignored_controllers: 0,
            ignored_touches: 0,
        };
    }

    let keyboard_mouse = gates.keyboard_mouse.then(|| {
        let mut keyboard_keys = keys
            .get_pressed()
            .chain(keys.get_just_pressed())
            .filter_map(|key| keyboard_usage(*key))
            .collect::<Vec<_>>();
        keyboard_keys.sort_unstable();
        keyboard_keys.dedup();
        let mut buttons = mouse_buttons
            .get_pressed()
            .chain(mouse_buttons.get_just_pressed())
            .filter_map(|button| mouse_button_code(*button))
            .collect::<Vec<_>>();
        buttons.sort_unstable();
        buttons.dedup();
        KeyboardMouseFrame {
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
        }
    });
    let bounded_gamepads = select_lowest_by_key(gamepads.iter(), MAX_CONTROLLERS, |(entity, _)| {
        u64::from(entity.index().index())
    });
    let mut controllers = Vec::with_capacity(bounded_gamepads.samples.len());
    for (entity, gamepad) in bounded_gamepads.samples {
        controllers.push(ControllerFrame {
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
        });
    }
    let width = window.width().max(1.0);
    let height = window.height().max(1.0);
    let bounded_touches =
        select_lowest_by_key(touches.iter(), MAX_TOUCH_CONTACTS, |touch| touch.id());
    touch_targets.retain_active_contacts(bounded_touches.samples.iter().map(|touch| touch.id()));
    let mut contacts = Vec::with_capacity(bounded_touches.samples.len());
    for touch in bounded_touches.samples {
        let contact = TouchContact {
            contact_id: touch.id(),
            activity_sequence: 0,
            position: [
                (touch.position().x / width).clamp(0.0, 1.0),
                (touch.position().y / height).clamp(0.0, 1.0),
            ],
            delta: [touch.delta().x / width, touch.delta().y / height],
            hit_id: touch_targets.target(touch.id()),
        };
        if contact.hit_id.is_some() {
            contacts.push(contact);
        }
    }
    TranslatedDeviceFrame {
        frame: DeviceFrame {
            keyboard_mouse,
            controllers,
            touches: contacts,
            ..DeviceFrame::default()
        },
        ignored_controllers: bounded_gamepads.ignored,
        ignored_touches: bounded_touches.ignored,
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
        KeyCode::Enter => 0x28,
        KeyCode::Escape => 0x29,
        KeyCode::Tab => 0x2b,
        KeyCode::Space => 0x2c,
        KeyCode::F5 => 0x3e,
        // The UiFocused defaults bind these four HID usages; without them the
        // arrow keys are dead in every menu.
        KeyCode::ArrowRight => 0x4f,
        KeyCode::ArrowLeft => 0x50,
        KeyCode::ArrowDown => 0x51,
        KeyCode::ArrowUp => 0x52,
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

/// The exact gamepad buttons this layer translates, and the binding codes they
/// produce. This is the single source of truth: `gamepad_button_codes` reads it
/// to build a frame, and the binding-reachability test reads it to prove no
/// default binding names a code the app cannot emit.
const TRANSLATED_GAMEPAD_BUTTONS: &[(u8, GamepadButton)] = &[
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
];

fn gamepad_button_codes(gamepad: &Gamepad) -> Vec<u8> {
    let mut buttons = TRANSLATED_GAMEPAD_BUTTONS
        .iter()
        .filter_map(|(code, button)| {
            (gamepad.pressed(*button) || gamepad.just_pressed(*button)).then_some(*code)
        })
        .collect::<Vec<_>>();
    buttons.sort_unstable();
    buttons
}

#[cfg(test)]
mod tests {
    use super::{
        PendingDeviceFrame, TRANSLATED_GAMEPAD_BUTTONS, collect_raw_input, input_source_gates,
        keyboard_usage, mouse_button_code, select_lowest_by_key,
    };
    use bevy::prelude::{KeyCode, MouseButton};
    use bevy::{
        input::{ButtonInput, gamepad::Gamepad, mouse::AccumulatedMouseMotion, touch::Touches},
        prelude::{App, Update, Window},
        window::{CursorGrabMode, CursorOptions, PrimaryWindow},
    };
    use semantic_input::{
        ControlSettings, ControllerFrame, MAX_CONTROLLERS, MAX_TOUCH_CONTACTS, PhysicalControl,
        TouchContact,
    };

    use crate::semantic_controls::SemanticTouchTargets;

    /// Exactly the keys this translation layer claims to support. A default
    /// binding naming a usage outside this set is dead input: the player
    /// presses the key, nothing happens, and nothing reports why.
    const TRANSLATED_KEYS: &[KeyCode] = &[
        KeyCode::KeyA,
        KeyCode::KeyD,
        KeyCode::KeyS,
        KeyCode::KeyW,
        KeyCode::Digit1,
        KeyCode::Digit2,
        KeyCode::Digit3,
        KeyCode::Digit4,
        KeyCode::Digit5,
        KeyCode::Digit6,
        KeyCode::Digit7,
        KeyCode::Digit8,
        KeyCode::Digit9,
        KeyCode::Escape,
        KeyCode::Space,
        KeyCode::Tab,
        KeyCode::Enter,
        KeyCode::ArrowUp,
        KeyCode::ArrowDown,
        KeyCode::ArrowLeft,
        KeyCode::ArrowRight,
        KeyCode::F5,
        KeyCode::ControlLeft,
        KeyCode::ShiftLeft,
        KeyCode::AltLeft,
        KeyCode::SuperLeft,
        KeyCode::ControlRight,
        KeyCode::ShiftRight,
        KeyCode::AltRight,
        KeyCode::SuperRight,
    ];

    const TRANSLATED_MOUSE_BUTTONS: &[MouseButton] = &[
        MouseButton::Left,
        MouseButton::Right,
        MouseButton::Middle,
        MouseButton::Back,
        MouseButton::Forward,
    ];

    /// Family-level guard: every default binding must name a physical control
    /// the app can actually emit, so a future binding cannot silently reintroduce
    /// an unreachable control.
    ///
    /// Touch hit IDs are deliberately out of scope: a binding can name a valid
    /// hit ID that no on-screen region ever assigns, which this cannot see.
    /// Touch reachability is tracked as an open gap, not proven here.
    #[test]
    fn default_binding_reachability_is_explicit_for_every_device_family() {
        let usages = TRANSLATED_KEYS
            .iter()
            .filter_map(|key| keyboard_usage(*key))
            .collect::<Vec<_>>();
        let buttons = TRANSLATED_MOUSE_BUTTONS
            .iter()
            .filter_map(|button| mouse_button_code(*button))
            .collect::<Vec<_>>();
        let gamepad = TRANSLATED_GAMEPAD_BUTTONS
            .iter()
            .map(|(code, _)| *code)
            .collect::<Vec<_>>();

        let mut unavailable_touch_bindings = 0;
        for binding in ControlSettings::default().bindings() {
            let action = binding.action;
            match binding.chord.control {
                PhysicalControl::KeyboardUsage(code) => assert!(
                    usages.contains(&code),
                    "{action:?} is bound to keyboard usage {code:#04x}, which keyboard_usage never emits"
                ),
                PhysicalControl::MouseButton(button) => assert!(
                    buttons.contains(&button),
                    "{action:?} is bound to mouse button {button}, which mouse_button_code never emits"
                ),
                PhysicalControl::GamepadButton(button) => assert!(
                    gamepad.contains(&button),
                    "{action:?} is bound to gamepad button {button}, which gamepad_button_codes never emits"
                ),
                PhysicalControl::MouseAxis(_) | PhysicalControl::GamepadAxis { .. } => {}
                PhysicalControl::TouchControl(_) => {
                    unavailable_touch_bindings += usize::from(
                        !crate::ui_runtime::gameplay_touch::PRODUCTION_TOUCH_LAYOUT_AVAILABLE,
                    );
                }
            }
        }
        assert!(
            unavailable_touch_bindings > 0,
            "default touch bindings must remain explicitly classified while their layout is unavailable"
        );
    }

    /// Every key this layer translates must produce a distinct HID usage, so a
    /// mapping typo cannot quietly alias two keys onto one action.
    #[test]
    fn translated_keys_map_to_distinct_usages() {
        let mut usages = TRANSLATED_KEYS
            .iter()
            .filter_map(|key| keyboard_usage(*key))
            .collect::<Vec<_>>();
        let translated = usages.len();
        usages.sort_unstable();
        usages.dedup();
        assert_eq!(usages.len(), translated);
    }

    #[test]
    fn focus_is_global_but_cursor_capture_only_gates_keyboard_mouse() {
        let focused_released = input_source_gates(true, false);
        assert!(!focused_released.keyboard_mouse);
        assert!(focused_released.controllers_and_touch);

        let unfocused = input_source_gates(false, true);
        assert!(!unfocused.keyboard_mouse);
        assert!(!unfocused.controllers_and_touch);
    }

    #[test]
    fn device_sampling_keeps_lowest_stable_ids_and_counts_overflow() {
        let controllers = (0..=MAX_CONTROLLERS)
            .rev()
            .map(|device_id| ControllerFrame {
                device_id: device_id as u32,
                ..ControllerFrame::default()
            });
        let touches = (0..=MAX_TOUCH_CONTACTS)
            .rev()
            .map(|contact_id| TouchContact {
                contact_id: contact_id as u64,
                activity_sequence: 0,
                position: [0.5, 0.5],
                delta: [0.0, 0.0],
                hit_id: None,
            });

        let bounded_controllers =
            select_lowest_by_key(controllers, MAX_CONTROLLERS, |controller| {
                u64::from(controller.device_id)
            });
        let bounded_touches =
            select_lowest_by_key(touches, MAX_TOUCH_CONTACTS, |touch| touch.contact_id);
        assert_eq!(bounded_controllers.samples.len(), MAX_CONTROLLERS);
        assert_eq!(bounded_touches.samples.len(), MAX_TOUCH_CONTACTS);
        assert_eq!(
            bounded_controllers
                .samples
                .iter()
                .map(|controller| controller.device_id)
                .collect::<Vec<_>>(),
            (0..MAX_CONTROLLERS as u32).collect::<Vec<_>>()
        );
        assert_eq!(
            bounded_touches
                .samples
                .iter()
                .map(|contact| contact.contact_id)
                .collect::<Vec<_>>(),
            (0..MAX_TOUCH_CONTACTS as u64).collect::<Vec<_>>()
        );
        assert_eq!(bounded_controllers.ignored, 1);
        assert_eq!(bounded_touches.ignored, 1);
    }

    #[test]
    fn device_sampling_does_not_retain_raw_population_capacity() {
        let controllers = (0..MAX_CONTROLLERS * 8).map(|device_id| ControllerFrame {
            device_id: device_id as u32,
            ..ControllerFrame::default()
        });
        let touches = (0..MAX_TOUCH_CONTACTS * 8).map(|contact_id| TouchContact {
            contact_id: contact_id as u64,
            activity_sequence: 0,
            position: [0.5, 0.5],
            delta: [0.0, 0.0],
            hit_id: None,
        });

        let bounded_controllers =
            select_lowest_by_key(controllers, MAX_CONTROLLERS, |controller| {
                u64::from(controller.device_id)
            });
        let bounded_touches =
            select_lowest_by_key(touches, MAX_TOUCH_CONTACTS, |touch| touch.contact_id);

        assert!(bounded_controllers.samples.capacity() <= MAX_CONTROLLERS);
        assert!(bounded_touches.samples.capacity() <= MAX_TOUCH_CONTACTS);
    }

    #[test]
    fn production_device_sampling_bounds_controller_allocation_before_translation() {
        let mut app = App::new();
        app.init_resource::<ButtonInput<KeyCode>>()
            .init_resource::<ButtonInput<MouseButton>>()
            .init_resource::<AccumulatedMouseMotion>()
            .init_resource::<Touches>()
            .init_resource::<SemanticTouchTargets>()
            .init_resource::<PendingDeviceFrame>()
            .add_systems(Update, collect_raw_input);
        app.world_mut().spawn((
            Window {
                focused: true,
                ..Window::default()
            },
            CursorOptions {
                grab_mode: CursorGrabMode::Locked,
                visible: false,
                ..CursorOptions::default()
            },
            PrimaryWindow,
        ));
        for _ in 0..MAX_CONTROLLERS * 8 {
            app.world_mut().spawn(Gamepad::default());
        }

        app.update();

        let pending = app.world().resource::<PendingDeviceFrame>();
        let frame = pending.frame.as_ref().unwrap();
        assert_eq!(frame.controllers.len(), MAX_CONTROLLERS);
        assert!(frame.controllers.capacity() <= MAX_CONTROLLERS);
        assert_eq!(pending.ignored_controllers, (MAX_CONTROLLERS * 7) as u64);
    }
}
