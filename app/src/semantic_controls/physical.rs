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
    ButtonEdges, ControllerFrame, DeviceFrame, KeyboardMouseFrame, MAX_CONTROLLERS,
    MAX_TOUCH_CONTACTS, ModifierChord, TouchContact,
};

use super::{SemanticInputRuntime, SemanticInputSnapshot, SemanticTouchTargets};
use crate::camera::mouse_input_active;

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
    focus: Option<Res<'w, client_presentation::camera::CursorFocus>>,
    driven: Option<Res<'w, crate::camera::DrivenInput>>,
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
    consent: Option<Res<crate::server_experiences::input::ConsentInput>>,
) {
    if consent.is_some_and(|consent| consent.0) {
        pending.frame = Some(DeviceFrame {
            window_focus_lost: true,
            ..Default::default()
        });
    }
    route.routed = pending
        .frame
        .take()
        .is_some_and(|frame| runtime.route_device_frame(frame).is_ok());
}

pub(crate) fn finalize_semantic_input_after_ui_authority(
    mut runtime: ResMut<SemanticInputRuntime>,
    mut route: ResMut<SemanticRouteState>,
    mut published: ResMut<SemanticInputSnapshot>,
    emote_input: Option<Res<crate::ui_runtime::emotes::EmoteInputConsumed>>,
) {
    let routed = std::mem::take(&mut route.routed);
    if !routed {
        published.clear();
        return;
    }
    if emote_input.is_some_and(|consumed| consumed.0) {
        // Raw devices were sampled before the wheel could open and close. Retire
        // that owned sample through the router's normal UI release/quarantine.
        runtime.release_all(semantic_input::ReleaseReason::UiFocusTaken);
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
        focus,
        driven,
    } = inputs;
    let (window, cursor) = window.into_inner();
    let focused = driven.is_some()
        || (window.focused && focus.as_ref().is_none_or(|focus| focus.available()));
    let captured = mouse_input_active(window, cursor, focus.as_deref(), driven.is_some());
    let gates = input_source_gates(focused, captured);
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
            .filter_map(|key| keyboard_usage(*key))
            .collect::<Vec<_>>();
        keyboard_keys.sort_unstable();
        keyboard_keys.dedup();
        let mut buttons = mouse_buttons
            .get_pressed()
            .filter_map(|button| mouse_button_code(*button))
            .collect::<Vec<_>>();
        buttons.sort_unstable();
        buttons.dedup();
        let sampled_key = |key| keys.pressed(key) || keys.just_pressed(key);
        KeyboardMouseFrame {
            activity_sequence: 0,
            keys: keyboard_keys,
            mouse_buttons: buttons,
            key_edges: ButtonEdges {
                pressed: keys
                    .get_just_pressed()
                    .filter_map(|key| keyboard_usage(*key))
                    .collect(),
                released: keys
                    .get_just_released()
                    .filter_map(|key| keyboard_usage(*key))
                    .collect(),
            },
            mouse_edges: ButtonEdges {
                pressed: mouse_buttons
                    .get_just_pressed()
                    .filter_map(|button| mouse_button_code(*button))
                    .collect(),
                released: mouse_buttons
                    .get_just_released()
                    .filter_map(|button| mouse_button_code(*button))
                    .collect(),
            },
            mouse_motion: mouse_motion.delta.to_array(),
            modifiers: ModifierChord {
                shift: sampled_key(KeyCode::ShiftLeft) || sampled_key(KeyCode::ShiftRight),
                control: sampled_key(KeyCode::ControlLeft) || sampled_key(KeyCode::ControlRight),
                alt: sampled_key(KeyCode::AltLeft) || sampled_key(KeyCode::AltRight),
                super_key: sampled_key(KeyCode::SuperLeft) || sampled_key(KeyCode::SuperRight),
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
            button_edges: ButtonEdges {
                pressed: TRANSLATED_GAMEPAD_BUTTONS
                    .iter()
                    .filter_map(|(code, button)| gamepad.just_pressed(*button).then_some(*code))
                    .collect(),
                released: TRANSLATED_GAMEPAD_BUTTONS
                    .iter()
                    .filter_map(|(code, button)| gamepad.just_released(*button).then_some(*code))
                    .collect(),
            },
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

/// Desktop keys and the USB usages settings and gameplay bind them by.
pub(crate) const KEYBOARD_USAGES: &[(KeyCode, u16)] = &[
    (KeyCode::KeyA, 0x04),
    (KeyCode::KeyD, 0x07),
    (KeyCode::KeyB, 0x05),
    (KeyCode::KeyC, 0x06),
    (KeyCode::KeyE, 0x08),
    (KeyCode::KeyF, 0x09),
    (KeyCode::KeyG, 0x0a),
    (KeyCode::KeyH, 0x0b),
    (KeyCode::KeyI, 0x0c),
    (KeyCode::KeyJ, 0x0d),
    (KeyCode::KeyK, 0x0e),
    (KeyCode::KeyL, 0x0f),
    (KeyCode::KeyM, 0x10),
    (KeyCode::KeyN, 0x11),
    (KeyCode::KeyO, 0x12),
    (KeyCode::KeyP, 0x13),
    (KeyCode::KeyQ, 0x14),
    (KeyCode::KeyR, 0x15),
    (KeyCode::KeyT, 0x17),
    (KeyCode::KeyU, 0x18),
    (KeyCode::KeyV, 0x19),
    (KeyCode::KeyX, 0x1b),
    (KeyCode::KeyY, 0x1c),
    (KeyCode::KeyZ, 0x1d),
    (KeyCode::KeyS, 0x16),
    (KeyCode::KeyW, 0x1a),
    (KeyCode::Digit1, 0x1e),
    (KeyCode::Digit2, 0x1f),
    (KeyCode::Digit3, 0x20),
    (KeyCode::Digit4, 0x21),
    (KeyCode::Digit5, 0x22),
    (KeyCode::Digit6, 0x23),
    (KeyCode::Digit7, 0x24),
    (KeyCode::Digit8, 0x25),
    (KeyCode::Digit9, 0x26),
    (KeyCode::Digit0, 0x27),
    (KeyCode::Backspace, 0x2a),
    (KeyCode::Enter, 0x28),
    (KeyCode::Escape, 0x29),
    (KeyCode::Tab, 0x2b),
    (KeyCode::Space, 0x2c),
    (KeyCode::Minus, 0x2d),
    (KeyCode::Equal, 0x2e),
    (KeyCode::BracketLeft, 0x2f),
    (KeyCode::BracketRight, 0x30),
    (KeyCode::Backslash, 0x31),
    (KeyCode::Semicolon, 0x33),
    (KeyCode::Quote, 0x34),
    (KeyCode::Backquote, 0x35),
    (KeyCode::Comma, 0x36),
    (KeyCode::Period, 0x37),
    (KeyCode::Slash, 0x38),
    (KeyCode::Insert, 0x49),
    (KeyCode::Home, 0x4a),
    (KeyCode::PageUp, 0x4b),
    (KeyCode::Delete, 0x4c),
    (KeyCode::End, 0x4d),
    (KeyCode::PageDown, 0x4e),
    (KeyCode::F1, 0x3a),
    (KeyCode::F2, 0x3b),
    (KeyCode::F3, 0x3c),
    (KeyCode::F4, 0x3d),
    (KeyCode::F5, 0x3e),
    (KeyCode::F6, 0x3f),
    (KeyCode::F7, 0x40),
    (KeyCode::F8, 0x41),
    (KeyCode::F9, 0x42),
    (KeyCode::F10, 0x43),
    (KeyCode::F11, 0x44),
    (KeyCode::F12, 0x45),
    // The UiFocused defaults bind these four HID usages; without them the
    // arrow keys are dead in every menu.
    (KeyCode::ArrowRight, 0x4f),
    (KeyCode::ArrowLeft, 0x50),
    (KeyCode::ArrowDown, 0x51),
    (KeyCode::ArrowUp, 0x52),
    (KeyCode::ControlLeft, 0xe0),
    (KeyCode::ShiftLeft, 0xe1),
    (KeyCode::AltLeft, 0xe2),
    (KeyCode::SuperLeft, 0xe3),
    (KeyCode::ControlRight, 0xe4),
    (KeyCode::ShiftRight, 0xe5),
    (KeyCode::AltRight, 0xe6),
    (KeyCode::SuperRight, 0xe7),
];

/// Converts a desktop key to the USB usage used by settings and gameplay.
pub(crate) fn keyboard_usage(key: KeyCode) -> Option<u16> {
    KEYBOARD_USAGES
        .iter()
        .find_map(|(candidate, usage)| (*candidate == key).then_some(*usage))
}

/// Converts desktop mouse buttons to the persisted gameplay binding codes.
pub(crate) fn mouse_button_code(button: MouseButton) -> Option<u8> {
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
pub(crate) const TRANSLATED_GAMEPAD_BUTTONS: &[(u8, GamepadButton)] = &[
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
        .filter_map(|(code, button)| gamepad.pressed(*button).then_some(*code))
        .collect::<Vec<_>>();
    buttons.sort_unstable();
    buttons
}

#[cfg(test)]
mod tests {
    use super::{
        KEYBOARD_USAGES, PendingDeviceFrame, TRANSLATED_GAMEPAD_BUTTONS, collect_raw_input,
        input_source_gates, keyboard_usage, mouse_button_code, select_lowest_by_key,
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
        let usages = KEYBOARD_USAGES
            .iter()
            .filter_map(|(key, _)| keyboard_usage(*key))
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
        let mut usages = KEYBOARD_USAGES
            .iter()
            .filter_map(|(key, _)| keyboard_usage(*key))
            .collect::<Vec<_>>();
        let translated = usages.len();
        usages.sort_unstable();
        usages.dedup();
        assert_eq!(usages.len(), translated);
    }

    #[test]
    fn review_same_frame_released_modifier_keeps_the_preserved_chord() {
        let mut app = App::new();
        app.init_resource::<ButtonInput<KeyCode>>()
            .init_resource::<ButtonInput<MouseButton>>()
            .init_resource::<AccumulatedMouseMotion>()
            .init_resource::<Touches>()
            .init_resource::<SemanticTouchTargets>()
            .init_resource::<PendingDeviceFrame>()
            .add_systems(Update, collect_raw_input);
        app.world_mut().spawn((
            Window::default(),
            CursorOptions {
                grab_mode: CursorGrabMode::Locked,
                visible: false,
                ..Default::default()
            },
            PrimaryWindow,
        ));
        {
            let mut keys = app.world_mut().resource_mut::<ButtonInput<KeyCode>>();
            keys.press(KeyCode::ShiftLeft);
            keys.press(KeyCode::KeyW);
            keys.release(KeyCode::ShiftLeft);
            keys.release(KeyCode::KeyW);
        }
        app.update();
        let pending = app.world().resource::<PendingDeviceFrame>();
        let keyboard = pending
            .frame
            .as_ref()
            .unwrap()
            .keyboard_mouse
            .as_ref()
            .unwrap();
        let forward = keyboard_usage(KeyCode::KeyW).unwrap();
        assert!(!keyboard.keys.contains(&forward));
        assert!(keyboard.key_edges.pressed.contains(&forward));
        assert!(keyboard.key_edges.released.contains(&forward));
        assert!(keyboard.modifiers.shift);
    }

    #[test]
    fn raw_motion_requires_desktop_capture_but_driven_input_has_no_os_grab() {
        let mut app = App::new();
        app.init_resource::<ButtonInput<KeyCode>>()
            .init_resource::<ButtonInput<MouseButton>>()
            .init_resource::<AccumulatedMouseMotion>()
            .init_resource::<Touches>()
            .init_resource::<SemanticTouchTargets>()
            .init_resource::<PendingDeviceFrame>()
            .init_resource::<client_presentation::camera::CursorFocus>()
            .add_systems(Update, collect_raw_input);
        let window = app
            .world_mut()
            .spawn((
                Window::default(),
                CursorOptions {
                    grab_mode: CursorGrabMode::Locked,
                    visible: false,
                    ..Default::default()
                },
                PrimaryWindow,
            ))
            .id();
        app.world_mut()
            .resource_mut::<AccumulatedMouseMotion>()
            .delta = bevy::prelude::Vec2::new(12.0, -4.0);
        app.update();
        assert_eq!(
            app.world()
                .resource::<PendingDeviceFrame>()
                .frame
                .as_ref()
                .unwrap()
                .keyboard_mouse
                .as_ref()
                .unwrap()
                .mouse_motion,
            [12.0, -4.0]
        );
        app.world_mut()
            .resource_mut::<client_presentation::camera::CursorFocus>()
            .occlusion_changed(true);
        app.update();
        let pending = app.world().resource::<PendingDeviceFrame>();
        assert!(pending.frame.as_ref().unwrap().window_focus_lost);
        assert!(pending.frame.as_ref().unwrap().keyboard_mouse.is_none());
        app.world_mut()
            .get_mut::<CursorOptions>(window)
            .unwrap()
            .grab_mode = CursorGrabMode::None;
        app.world_mut()
            .get_mut::<CursorOptions>(window)
            .unwrap()
            .visible = true;
        app.world_mut().get_mut::<Window>(window).unwrap().focused = false;
        app.init_resource::<crate::camera::DrivenInput>();
        app.update();
        assert_eq!(
            app.world()
                .resource::<PendingDeviceFrame>()
                .frame
                .as_ref()
                .unwrap()
                .keyboard_mouse
                .as_ref()
                .unwrap()
                .mouse_motion,
            [12.0, -4.0]
        );
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
