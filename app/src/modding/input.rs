//! Routes granted personal controls before the ordinary gameplay sample.

use super::{MenuRuntime, ModRuntime};
use bevy::{
    ecs::message::MessageCursor,
    input::{
        ButtonState,
        keyboard::KeyboardInput,
        mouse::{AccumulatedMouseMotion, MouseButtonInput},
    },
    prelude::*,
    window::{CursorGrabMode, CursorOptions, PrimaryWindow},
};
use client_ui::ui_runtime::{UiRuntime, presentation::UiPresentationRuntime};

#[derive(Default)]
pub(super) struct PhysicalControls {
    keys: MessageCursor<KeyboardInput>,
    mouse: MessageCursor<MouseButtonInput>,
    left_held: bool,
    held_keys: Vec<String>,
    /// Whether `held_keys` was seeded from physical input since the mod attached or refocused.
    held_seeded: bool,
    restore_capture: bool,
    panel_owned: bool,
}

#[cfg(test)]
mod focus_tests;

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn reserved_fast_tap_never_reaches_gameplay_edges() {
        let mut keys = ButtonInput::default();
        keys.press(KeyCode::KeyW);
        keys.release(KeyCode::KeyW);
        keys.press(KeyCode::KeyA);
        consume_reserved(&mut keys, &["KeyW".into()], Some("ShiftRight"));
        assert!(!keys.just_pressed(KeyCode::KeyW));
        assert!(!keys.just_released(KeyCode::KeyW));
        assert!(keys.pressed(KeyCode::KeyA));
        assert!(keys.just_pressed(KeyCode::KeyA));
    }

    #[test]
    fn keys_already_held_when_a_mod_attaches_are_reported_held_without_a_press_edge() {
        let directory = tempfile::tempdir().unwrap();
        let path = directory.path().join("empty.wat");
        std::fs::write(
            &path,
            r#"(component
            (core module $m (func (export "init")) (func (export "frame")))
            (core instance $i (instantiate $m))
            (func (export "init") (canon lift (core func $i "init")))
            (func (export "frame") (canon lift (core func $i "frame"))))"#,
        )
        .unwrap();
        let mut scratch = App::new();
        super::super::configure_set(&mut scratch, vec![(path, mod_host::ModGrants::default())]);
        let runtime = scratch
            .world_mut()
            .remove_resource::<super::super::ModRuntime>()
            .unwrap();
        let mut app = App::new();
        let mut keys = ButtonInput::<KeyCode>::default();
        keys.press(KeyCode::ShiftLeft);
        keys.clear_just_pressed(KeyCode::ShiftLeft);
        app.insert_resource(runtime)
            .insert_resource(keys)
            .insert_resource(UiRuntime::new(1))
            .insert_resource(crate::player_runtime::PlayerRuntime::new(1))
            .insert_resource(
                UiPresentationRuntime::new(client_ui::test_support::fixture_font()).unwrap(),
            )
            .add_systems(Update, prepare_mod_input);
        app.world_mut().spawn((
            Window {
                focused: true,
                ..Default::default()
            },
            CursorOptions::default(),
            PrimaryWindow,
        ));
        app.update();
        let controls = &app.world().resource::<super::super::ModRuntime>().controls;
        assert_eq!(controls.keys_held, ["ShiftLeft"]);
        assert!(controls.keys_pressed.is_empty());
    }

    #[test]
    fn held_keys_follow_press_and_release_and_stay_bounded() {
        let mut physical = PhysicalControls::default();
        physical.track_held("Digit1".into(), true);
        physical.track_held("Digit1".into(), true);
        physical.track_held("ShiftLeft".into(), true);
        assert_eq!(physical.held_keys, ["Digit1", "ShiftLeft"]);
        physical.track_held("Digit1".into(), false);
        assert_eq!(physical.held_keys, ["ShiftLeft"]);
        for index in 0..mod_host::MAX_CONTROL_KEYS * 2 {
            physical.track_held(format!("Key{index}"), true);
        }
        assert_eq!(physical.held_keys.len(), mod_host::MAX_CONTROL_KEYS);
        physical.discard_pending(None, None);
        assert!(physical.held_keys.is_empty());
    }

    #[test]
    fn guest_removal_or_quarantine_restores_previous_capture_once() {
        let mut physical = PhysicalControls::default();
        assert!(!physical.finish_panel(true, true, false, true));
        assert!(physical.finish_panel(false, true, false, false));
        assert!(!physical.finish_panel(false, true, false, false));
    }

    #[test]
    fn focus_or_other_ui_cancels_recapture_and_uncaptured_open_stays_free() {
        for (focused, absorbed) in [(false, false), (true, true)] {
            let mut physical = PhysicalControls::default();
            physical.finish_panel(true, true, false, true);
            assert!(!physical.finish_panel(false, focused, absorbed, false));
            assert!(!physical.finish_panel(false, true, false, false));
        }
        let mut physical = PhysicalControls::default();
        physical.finish_panel(true, true, false, false);
        assert!(!physical.finish_panel(false, true, false, false));
    }

    #[test]
    fn dormant_input_discards_old_edges_and_observes_only_next_attachment_edges() {
        use bevy::input::keyboard::Key;
        let mut keyboard = Messages::default();
        let key = |key_code, logical_key| KeyboardInput {
            key_code,
            logical_key,
            state: ButtonState::Pressed,
            text: None,
            repeat: false,
            window: Entity::PLACEHOLDER,
        };
        keyboard.write(key(KeyCode::F8, Key::F8));
        let mut mouse = Messages::default();
        mouse.write(MouseButtonInput {
            button: MouseButton::Left,
            state: ButtonState::Pressed,
            window: Entity::PLACEHOLDER,
        });
        let mut physical = PhysicalControls {
            left_held: true,
            ..Default::default()
        };
        physical.discard_pending(Some(&keyboard), Some(&mouse));
        assert!(!physical.left_held);
        assert_eq!(physical.keys.read(&keyboard).count(), 0);
        assert_eq!(physical.mouse.read(&mouse).count(), 0);
        keyboard.write(key(KeyCode::F10, Key::F10));
        mouse.write(MouseButtonInput {
            button: MouseButton::Left,
            state: ButtonState::Released,
            window: Entity::PLACEHOLDER,
        });
        assert_eq!(
            physical
                .keys
                .read(&keyboard)
                .map(|event| event.key_code)
                .collect::<Vec<_>>(),
            [KeyCode::F10]
        );
        assert_eq!(
            physical
                .mouse
                .read(&mouse)
                .map(|event| event.state)
                .collect::<Vec<_>>(),
            [ButtonState::Released]
        );
    }
}

impl PhysicalControls {
    fn discard_pending(
        &mut self,
        keyboard: Option<&Messages<KeyboardInput>>,
        mouse: Option<&Messages<MouseButtonInput>>,
    ) {
        if let Some(events) = keyboard {
            self.keys.clear(events);
        }
        if let Some(events) = mouse {
            self.mouse.clear(events);
        }
        self.left_held = false;
        self.held_keys.clear();
        self.held_seeded = false;
    }

    /// Adopts keys already down at attachment as held, without producing press edges.
    fn seed_held<'a>(&mut self, pressed: impl Iterator<Item = &'a KeyCode>) {
        if std::mem::replace(&mut self.held_seeded, true) {
            return;
        }
        for key in pressed.filter(|key| !matches!(key, KeyCode::Unidentified(_))) {
            self.track_held(format!("{key:?}"), true);
        }
    }

    /// Tracks held keys from events, since reserved keys are reset out of the shared input state.
    fn track_held(&mut self, key: String, pressed: bool) {
        let index = self.held_keys.iter().position(|held| *held == key);
        match (pressed, index) {
            (true, None) if self.held_keys.len() < mod_host::MAX_CONTROL_KEYS => {
                self.held_keys.push(key);
            }
            (false, Some(index)) => {
                self.held_keys.swap_remove(index);
            }
            _ => {}
        }
    }

    /// Remembers input ownership independently of guest reload or quarantine.
    fn finish_panel(&mut self, open: bool, focused: bool, absorbed: bool, captured: bool) -> bool {
        if !focused || absorbed {
            self.restore_capture = false;
        }
        if open && !self.panel_owned {
            self.restore_capture = captured;
        }
        let restore = !open && self.panel_owned && self.restore_capture && focused && !absorbed;
        self.panel_owned = open;
        if !open {
            self.restore_capture = false;
        }
        restore
    }
}

#[allow(
    clippy::too_many_arguments,
    reason = "Routes one physical frame before gameplay authority."
)]
pub(super) fn prepare_mod_input(
    extension: Option<ResMut<ModRuntime>>,
    mut physical: Local<PhysicalControls>,
    keyboard_events: Option<Res<Messages<KeyboardInput>>>,
    mouse_events: Option<Res<Messages<MouseButtonInput>>>,
    mut keys: ResMut<ButtonInput<KeyCode>>,
    mut mouse: Option<ResMut<ButtonInput<MouseButton>>>,
    mut motion: Option<ResMut<AccumulatedMouseMotion>>,
    mut windows: Query<(Entity, &Window, &mut CursorOptions), With<PrimaryWindow>>,
    ui: Res<UiRuntime>,
    player: Res<crate::player_runtime::PlayerRuntime>,
    menu: Option<Res<MenuRuntime>>,
    mut presentation: ResMut<UiPresentationRuntime>,
    time: Option<Res<Time>>,
    mut focus: Option<ResMut<client_presentation::camera::CursorFocus>>,
    driven: Option<Res<crate::camera::DrivenInput>>,
) {
    let extension = extension.filter(|runtime| !runtime.suspended);
    if extension.is_none() {
        physical.discard_pending(keyboard_events.as_deref(), mouse_events.as_deref());
        if !physical.panel_owned {
            return;
        }
    }
    let Ok((entity, window, mut cursor)) = windows.single_mut() else {
        return;
    };
    let focused = driven.is_some()
        || (window.focused && focus.as_ref().is_none_or(|focus| focus.available()));
    let Some(mut extension) = extension else {
        let absorbed = menu.as_deref().map_or_else(
            || ui.ui_focused(&player),
            |menu| presentation.base_absorbs_gameplay_input(&player, &ui, menu),
        );
        let restore = physical.finish_panel(
            false,
            focused,
            absorbed,
            crate::camera::mouse_input_active(window, &cursor, focus.as_deref(), driven.is_some()),
        );
        if let Some(mouse) = mouse.as_mut() {
            mouse.clear();
        }
        keys.reset(KeyCode::Escape);
        if restore {
            cursor.grab_mode = CursorGrabMode::Locked;
            cursor.visible = false;
            if let Some(motion) = motion.as_mut() {
                motion.delta = Vec2::ZERO;
            }
        }
        return;
    };
    if focused {
        physical.seed_held(keys.get_pressed());
    }
    let mut panel_keys = Vec::new();
    if let Some(events) = keyboard_events {
        let mut transitions = Vec::new();
        for event in physical.keys.read(&events) {
            if event.window != entity || matches!(event.key_code, KeyCode::Unidentified(_)) {
                continue;
            }
            let name = format!("{:?}", event.key_code);
            let down = event.state == ButtonState::Pressed;
            transitions.push((name.clone(), down));
            if down && focused {
                panel_keys.push((
                    name,
                    event
                        .text
                        .as_deref()
                        .or_else(|| match &event.logical_key {
                            bevy::input::keyboard::Key::Character(text) => Some(text.as_str()),
                            _ => None,
                        })
                        .map(str::to_owned),
                    event.repeat,
                ));
            }
        }
        for (name, down) in transitions {
            physical.track_held(name, down);
        }
    }
    if let Some(events) = mouse_events {
        let mut held = physical.left_held;
        for event in physical.mouse.read(&events) {
            if event.window == entity && event.button == MouseButton::Left {
                held = event.state == ButtonState::Pressed;
            }
        }
        physical.left_held = held;
    } else {
        physical.left_held = mouse
            .as_ref()
            .is_some_and(|buttons| buttons.pressed(MouseButton::Left));
    }
    let absorbed = menu.as_deref().map_or_else(
        || ui.ui_focused(&player),
        |menu| presentation.base_absorbs_gameplay_input(&player, &ui, menu),
    );
    super::hud_editor::guard(
        &mut extension,
        &mut presentation,
        Some(ui.session_id()),
        focused,
        absorbed,
    );
    let was_open = physical.panel_owned;
    let owner = extension.panel_owner();
    let mut open = extension.host(owner).panel_open() && presentation.mod_panel_open();
    let had_panel = open;
    let editing = open && presentation.mod_panel_editing();
    let interrupt = editing
        && panel_keys.iter().any(|(key, _, repeat)| {
            !repeat && (key == "F10" || presentation.mod_panel_toggle_key() == Some(key.as_str()))
        });
    if !focused {
        presentation.cancel_mod_panel_pointer_input();
    }
    if absorbed || interrupt {
        presentation.cancel_mod_panel_edit();
    }
    let mut pressed = Vec::new();
    let mut events = Vec::new();
    for (key, text, repeat) in panel_keys {
        if open && focused && !absorbed && editing && !interrupt {
            let edit_key = if key == "KeyA"
                && (keys.pressed(KeyCode::ControlLeft) || keys.pressed(KeyCode::ControlRight))
            {
                "SelectAll"
            } else {
                key.as_str()
            };
            events.extend(presentation.mod_panel_key(edit_key, text.as_deref()));
        } else if !repeat
            && (!editing
                || key == "F10"
                || presentation.mod_panel_toggle_key() == Some(key.as_str()))
        {
            pressed.push(key);
        }
    }
    let close_requested = pressed.iter().any(|key| key == "Escape")
        && open
        && !extension
            .host(owner)
            .panel()
            .is_some_and(|panel| panel.capture_key);
    let toggle_requested = extension
        .host(owner)
        .panel()
        .is_some_and(|panel| pressed.contains(&panel.toggle_key));
    if !extension.host(owner).is_active() || (focused && (absorbed || close_requested)) {
        open = false;
    } else if toggle_requested {
        open = !open;
    }
    extension.host_mut(owner).set_panel_open(open);
    presentation.set_mod_panel_open(open);
    let before_pointer_open = open;
    let was_held = mouse
        .as_ref()
        .is_some_and(|buttons| buttons.just_pressed(MouseButton::Left));
    if open && focused {
        if let Some(position) = window.cursor_position() {
            events.extend(presentation.mod_panel_events(
                position.to_array(),
                was_held,
                physical.left_held,
            ));
        } else if !physical.left_held {
            // A release outside the window must end capture before a later pointer re-entry.
            presentation.cancel_mod_panel_pointer_input();
        }
    }
    super::hud_editor::collect(&mut extension, &mut presentation);
    let events = events
        .into_iter()
        .take(ui::mod_panel::MAX_PANEL_CONTROLS)
        .map(|event| mod_host::ControlEvent {
            id: event.id,
            value: event.value,
        })
        .collect();
    open = presentation.mod_panel_open();
    let pointer_closed = before_pointer_open && !open;
    if !open
        && focused
        && !absorbed
        && extension.host(owner).is_active()
        && ((had_panel && (close_requested || toggle_requested)) || pointer_closed)
        && let Some(focus) = focus.as_deref_mut()
    {
        focus.authorize_screen_return();
    }
    extension.host_mut(owner).set_panel_open(open);
    let restore = physical.finish_panel(
        open,
        focused,
        absorbed,
        crate::camera::mouse_input_active(window, &cursor, focus.as_deref(), driven.is_some()),
    );
    extension.controls = mod_host::ControlFrame {
        seconds: time
            .as_ref()
            .map_or(0.0, |time| time.delta_secs().clamp(0.0, 1.0)),
        focused,
        gameplay: false,
        panel_open: open,
        keys_pressed: if focused {
            pressed
                .into_iter()
                .filter(|key| !absorbed || key == "F10")
                .take(mod_host::MAX_CONTROL_KEYS)
                .collect()
        } else {
            Vec::new()
        },
        keys_held: if focused && !absorbed && !editing {
            physical.held_keys.clone()
        } else {
            Vec::new()
        },
        events,
    };
    if !focused {
        physical.left_held = false;
        physical.held_keys.clear();
        physical.held_seeded = false;
    }
    if (open || was_open)
        && let Some(mouse) = mouse.as_mut()
    {
        mouse.clear();
    }
    if was_open && !open {
        keys.reset(KeyCode::Escape);
    }
    if open {
        crate::camera::release_cursor(&mut cursor);
        keys.clear();
        if let Some(motion) = motion.as_mut() {
            motion.delta = Vec2::ZERO;
        }
    } else {
        if restore {
            cursor.grab_mode = CursorGrabMode::Locked;
            cursor.visible = false;
            if let Some(motion) = motion.as_mut() {
                motion.delta = Vec2::ZERO;
            }
            // A close edge belongs to the panel, not the underlying pause menu.
            keys.reset(KeyCode::Escape);
        }
        let reserved = extension.reserved_keys();
        let toggle = extension
            .host(owner)
            .panel()
            .map(|panel| panel.toggle_key.as_str());
        consume_reserved(&mut keys, &reserved, toggle);
    }
}

fn consume_reserved(keys: &mut ButtonInput<KeyCode>, reserved: &[String], toggle: Option<&str>) {
    let consumed: Vec<_> = keys
        .get_pressed()
        .chain(keys.get_just_pressed())
        .chain(keys.get_just_released())
        .copied()
        .filter(|key| {
            let name = format!("{key:?}");
            reserved.contains(&name) || toggle == Some(name.as_str())
        })
        .collect();
    for key in consumed {
        keys.reset(key);
    }
}
