//! Routes granted personal controls before the ordinary gameplay sample.

mod physical;

use super::{MenuRuntime, ModRuntime};
use bevy::{
    ecs::{message::MessageCursor, system::SystemParam},
    input::{
        ButtonState,
        keyboard::KeyboardInput,
        mouse::{
            AccumulatedMouseMotion, AccumulatedMouseScroll, MouseButtonInput, MouseScrollUnit,
        },
    },
    prelude::*,
    window::{CursorGrabMode, CursorOptions, PrimaryWindow},
};
use client_shell::ui_resources::{UiPresentationResource, UiRuntimeResource};
#[cfg(test)]
use client_ui::ui_runtime::{UiRuntime, presentation::UiPresentationRuntime};

/// Pointer messages and wheel accumulation routed while a personal panel owns input.
#[derive(SystemParam)]
pub(super) struct PanelPointerInput<'w> {
    mouse_events: Option<Res<'w, Messages<MouseButtonInput>>>,
    scroll: Option<Res<'w, AccumulatedMouseScroll>>,
}

const POINTER_BUTTONS: [(MouseButton, &str); 3] = [
    (MouseButton::Left, "MouseLeft"),
    (MouseButton::Right, "MouseRight"),
    (MouseButton::Middle, "MouseMiddle"),
];

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
    /// A packet panel accepted this primary gesture; interruption requires a fresh press.
    packet_pointer_active: bool,
}

#[allow(
    clippy::too_many_arguments,
    reason = "Routes one physical frame before gameplay authority."
)]
pub(super) fn prepare_mod_input(
    extension: Option<ResMut<ModRuntime>>,
    mut packets: Option<ResMut<super::packets::PacketMods>>,
    mut physical: Local<PhysicalControls>,
    keyboard_events: Option<Res<Messages<KeyboardInput>>>,
    pointer_input: PanelPointerInput,
    mut keys: ResMut<ButtonInput<KeyCode>>,
    mut mouse: Option<ResMut<ButtonInput<MouseButton>>>,
    mut motion: Option<ResMut<AccumulatedMouseMotion>>,
    mut windows: Query<(Entity, &Window, &mut CursorOptions), With<PrimaryWindow>>,
    ui: Res<UiRuntimeResource>,
    player: Res<client_state::PlayerRuntime>,
    menu: Option<Res<MenuRuntime>>,
    mut presentation: ResMut<UiPresentationResource>,
    time: Option<Res<Time>>,
    mut focus: Option<ResMut<client_state::CursorFocus>>,
    driven: Option<Res<client_state::DrivenInput>>,
) {
    let PanelPointerInput {
        mouse_events,
        scroll,
    } = pointer_input;
    let mut extension = extension.filter(|runtime| !runtime.suspended);
    let viewport = windows
        .single()
        .ok()
        .map(|(_, window, _)| presentation.mod_panel_viewport([window.width(), window.height()]))
        .filter(|size| size.iter().all(|extent| *extent > 0.0));
    if let Some(packets) = packets.as_mut() {
        super::packet_menu::poll(packets, &mut presentation, ui.session_id(), viewport);
    }
    let packet_available = packets
        .as_ref()
        .is_some_and(|packets| packets.native_menu_identity().is_some());
    let mut packet_owned = packets
        .as_ref()
        .is_some_and(|packets| packets.menu_owns_panel());
    if extension.is_none() && !packet_owned && !packet_available {
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
    if focused {
        let seed_pointer = !physical.held_seeded;
        physical.seed_held(keys.get_pressed());
        if seed_pointer && let Some(buttons) = mouse.as_ref() {
            for (button, name) in POINTER_BUTTONS {
                physical.track_held(
                    name.into(),
                    buttons.pressed(button) && !buttons.just_pressed(button),
                );
            }
        }
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
    let pointer_pressed =
        physical.pointer_edges(mouse_events.as_deref(), mouse.as_deref(), entity, focused);
    let primary_pressed = pointer_pressed.iter().any(|key| key == "MouseLeft");
    let absorbed = menu.as_deref().map_or_else(
        || ui.ui_focused(&player),
        |menu| presentation.base_absorbs_gameplay_input(&player, &ui, menu),
    );
    if packet_available && focused && !absorbed {
        if let Some(packets) = packets.as_mut() {
            let toggle = packets
                .menu
                .retained
                .as_ref()
                .and_then(|retained| retained.output.panel.as_ref())
                .map(|panel| panel.toggle_key.clone());
            if toggle.as_ref().is_some_and(|toggle| {
                take_packet_toggle(
                    &mut panel_keys,
                    toggle,
                    packet_owned,
                    presentation.mod_ui().mod_panel_open(),
                    presentation.mod_ui().mod_panel_capture_key(),
                )
            }) {
                super::packet_menu::toggle(packets, &mut presentation);
                packet_owned = packets.menu_owns_panel();
            }
            if !packet_owned && !presentation.mod_ui().mod_panel_open() {
                let reserved = packets
                    .menu
                    .retained
                    .as_ref()
                    .map(|retained| retained.output.keys.clone())
                    .unwrap_or_default();
                packets.menu.pending.keys = panel_keys
                    .iter()
                    .filter(|(key, _, repeat)| !repeat && reserved.contains(key))
                    .take(mod_api::MAX_CONTROL_KEYS)
                    .map(|(key, _, _)| mod_api::native_menu::Key {
                        key: key.clone(),
                        text: None,
                    })
                    .collect();
                consume_reserved(&mut keys, &reserved, toggle.as_deref());
            }
        }
    }
    if !packet_owned {
        let (_, cancelled) = physical.packet_pointer_sample(
            false,
            focused,
            absorbed,
            window.cursor_position().is_some(),
            primary_pressed,
        );
        if let Some(packets) = packets.as_mut() {
            packets.menu.pending.pointer_held = false;
            packets.menu.pending.pointer_cancelled |= cancelled;
            packets.menu.pending.shift_held = physical.shift_held(focused);
        }
    }
    if extension.is_none() && !packet_owned && !physical.panel_owned {
        return;
    }
    if packet_owned {
        let mut packets = packets.expect("packet-owned panel requires its packet session");
        let was_open = physical.panel_owned;
        let mut open = presentation.mod_ui().mod_panel_open();
        let editing = open && presentation.mod_ui().mod_panel_editing();
        let capture = presentation.mod_ui().mod_panel_capture_key();
        let mut events = Vec::new();
        let mut raw_keys = Vec::new();
        if !focused {
            presentation.mod_ui_mut().cancel_mod_panel_pointer_input();
        }
        if absorbed {
            presentation.mod_ui_mut().cancel_mod_panel_edit();
            open = false;
        }
        for (key, text, _) in panel_keys {
            if !open || !focused || absorbed {
                continue;
            }
            if editing {
                let key = if key == "KeyA"
                    && (keys.pressed(KeyCode::ControlLeft) || keys.pressed(KeyCode::ControlRight))
                {
                    "SelectAll"
                } else {
                    key.as_str()
                };
                events.extend(
                    presentation
                        .mod_ui_mut()
                        .mod_panel_key(key, text.as_deref()),
                );
            } else if key == "Escape" && !capture {
                open = false;
            } else {
                raw_keys.push(mod_api::native_menu::Key {
                    key,
                    text: text
                        .filter(|text| text.len() <= 256 && !text.chars().any(char::is_control)),
                });
            }
        }
        presentation.mod_ui_mut().set_mod_panel_open(open);
        if open && focused {
            if let Some(position) = window.cursor_position() {
                events.extend(presentation.mod_ui_mut().mod_panel_events(
                    position.to_array(),
                    primary_pressed,
                    physical.left_held,
                ));
                if let Some(scroll) = scroll.as_ref() {
                    presentation.mod_ui_mut().scroll_mod_panel(
                        position.to_array(),
                        f64::from(scroll.delta.y),
                        scroll.unit == MouseScrollUnit::Pixel,
                    );
                }
            } else {
                presentation.mod_ui_mut().cancel_mod_panel_pointer_input();
            }
        }
        open = presentation.mod_ui().mod_panel_open();
        let (pointer_held, pointer_cancelled) = physical.packet_pointer_sample(
            open,
            focused,
            absorbed,
            window.cursor_position().is_some(),
            primary_pressed,
        );
        packets.menu.pending = mod_api::native_menu::Input {
            version: mod_api::native_menu::VERSION,
            open,
            pointer_held,
            pointer_cancelled,
            shift_held: physical.shift_held(focused),
            viewport,
            hud_geometry: Default::default(),
            events: events
                .into_iter()
                .take(ui::mod_panel::MAX_PANEL_EVENTS)
                .map(|event| mod_api::native_menu::Event {
                    id: event.id,
                    value: event.value,
                })
                .collect(),
            keys: raw_keys
                .into_iter()
                .take(mod_api::MAX_CONTROL_KEYS)
                .collect(),
        };
        if let Some(extension) = extension.as_mut() {
            extension.controls = mod_host::empty_controls();
            extension.controls.panel_open = open;
        }
        if was_open && !open && focused && !absorbed {
            if let Some(focus) = focus.as_deref_mut() {
                focus.authorize_screen_return();
            }
        }
        let restore = physical.finish_panel(
            open,
            focused,
            absorbed,
            crate::camera::mouse_input_active(window, &cursor, focus.as_deref(), driven.is_some()),
        );
        finish_capture(
            &mut physical,
            open,
            was_open,
            focused,
            restore,
            &mut keys,
            mouse.as_deref_mut(),
            motion.as_deref_mut(),
            &mut cursor,
        );
        return;
    }
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
    super::hud_editor::guard(
        &mut extension,
        &mut presentation,
        Some(ui.session_id()),
        focused,
        absorbed,
    );
    let was_open = physical.panel_owned;
    if focused && !absorbed {
        let toggles: Vec<_> = panel_keys
            .iter()
            .filter(|(_, _, repeat)| !repeat)
            .map(|(key, _, _)| key.clone())
            .collect();
        if extension.hosts.select_panel(&toggles) {
            presentation.mod_ui_mut().cancel_mod_panel_edit();
            let owner = extension.hosts.panel_owner();
            let _ = presentation.set_mod_panel_font(extension.metadata[owner].font.clone());
            if let Err(error) = presentation.mod_ui_mut().set_mod_panel(
                extension.panel_ui_owner(owner),
                extension.hosts.host(owner).panel(),
            ) {
                warn!("Cinnabar extension panel rejected: {error}");
            }
            presentation.mod_ui_mut().set_mod_panel_open(false);
        }
    }
    let owner = extension.hosts.panel_owner();
    let mut open =
        extension.hosts.host(owner).panel_open() && presentation.mod_ui().mod_panel_open();
    let had_panel = open;
    let editing = open && presentation.mod_ui().mod_panel_editing();
    let interrupt = editing
        && panel_keys.iter().any(|(key, _, repeat)| {
            !repeat
                && (key == "F10"
                    || presentation.mod_ui().mod_panel_toggle_key() == Some(key.as_str()))
        });
    if !focused {
        presentation.mod_ui_mut().cancel_mod_panel_pointer_input();
    }
    if absorbed || interrupt {
        presentation.mod_ui_mut().cancel_mod_panel_edit();
    }
    let mut pressed = pointer_pressed;
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
            events.extend(
                presentation
                    .mod_ui_mut()
                    .mod_panel_key(edit_key, text.as_deref()),
            );
        } else if !repeat
            && (!editing
                || key == "F10"
                || presentation.mod_ui().mod_panel_toggle_key() == Some(key.as_str()))
        {
            pressed.push(key);
        }
    }
    let editor_closed_panel = open && !presentation.mod_ui().mod_panel_open();
    open &= presentation.mod_ui().mod_panel_open();
    let close_requested = pressed.iter().any(|key| key == "Escape")
        && open
        && !extension
            .hosts
            .host(owner)
            .panel()
            .is_some_and(|panel| panel.capture_key);
    let toggle_requested = extension
        .hosts
        .host(owner)
        .panel()
        .is_some_and(|panel| pressed.contains(&panel.toggle_key));
    if !extension.hosts.host(owner).is_active() || (focused && (absorbed || close_requested)) {
        open = false;
    } else if toggle_requested {
        open = !open;
    }
    extension.hosts.host_mut(owner).set_panel_open(open);
    presentation.mod_ui_mut().set_mod_panel_open(open);
    let before_pointer_open = open;
    let was_held = mouse
        .as_ref()
        .is_some_and(|buttons| buttons.just_pressed(MouseButton::Left));
    if open && focused {
        if let Some(position) = window.cursor_position() {
            events.extend(presentation.mod_ui_mut().mod_panel_events(
                position.to_array(),
                was_held,
                physical.left_held,
            ));
            if let Some(scroll) = scroll.as_ref() {
                presentation.mod_ui_mut().scroll_mod_panel(
                    position.to_array(),
                    f64::from(scroll.delta.y),
                    scroll.unit == MouseScrollUnit::Pixel,
                );
            }
        } else if !physical.left_held {
            // A release outside the window must end capture before a later pointer re-entry.
            presentation.mod_ui_mut().cancel_mod_panel_pointer_input();
        }
    }
    super::hud_editor::collect(&mut extension, &mut presentation);
    if extension.hosts.hud_owner() == Some(owner) {
        events.extend(
            presentation
                .take_chat_extension_actions()
                .into_iter()
                .map(|id| ui::mod_panel::Event { id, value: 1. }),
        );
    }
    let events = events
        .into_iter()
        .take(ui::mod_panel::MAX_PANEL_EVENTS)
        .map(|event| mod_host::ControlEvent {
            id: event.id,
            value: event.value,
        })
        .collect();
    open = presentation.mod_ui().mod_panel_open();
    let pointer_closed = before_pointer_open && !open;
    if !open
        && focused
        && !absorbed
        && extension.hosts.host(owner).is_active()
        && ((had_panel && (close_requested || toggle_requested))
            || pointer_closed
            || editor_closed_panel)
        && let Some(focus) = focus.as_deref_mut()
    {
        focus.authorize_screen_return();
    }
    extension.hosts.host_mut(owner).set_panel_open(open);
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
                .take(mod_api::MAX_CONTROL_KEYS)
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
    finish_capture(
        &mut physical,
        open,
        was_open,
        focused,
        restore,
        &mut keys,
        mouse.as_deref_mut(),
        motion.as_deref_mut(),
        &mut cursor,
    );
    if !open {
        let mut reserved = extension.hosts.reserved_keys();
        for index in 0..extension.hosts.host_count() {
            if let Some(panel) = extension.hosts.host(index).panel() {
                reserved.push(panel.toggle_key.clone());
            }
        }
        let toggle = extension
            .hosts
            .host(owner)
            .panel()
            .map(|panel| panel.toggle_key.as_str());
        consume_reserved(&mut keys, &reserved, toggle);
    }
}

/// Applies shared pointer capture and input consumption for every native personal panel.
#[allow(
    clippy::too_many_arguments,
    reason = "Applies one native panel ownership transition."
)]
fn finish_capture(
    physical: &mut PhysicalControls,
    open: bool,
    was_open: bool,
    focused: bool,
    restore: bool,
    keys: &mut ButtonInput<KeyCode>,
    mut mouse: Option<&mut ButtonInput<MouseButton>>,
    mut motion: Option<&mut AccumulatedMouseMotion>,
    cursor: &mut Mut<CursorOptions>,
) {
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
        client_shell::physical_input::release_cursor(cursor);
        keys.clear();
        if let Some(motion) = motion.as_mut() {
            motion.delta = Vec2::ZERO;
        }
    } else if restore {
        cursor.grab_mode = CursorGrabMode::Locked;
        cursor.visible = false;
        if let Some(motion) = motion.as_mut() {
            motion.delta = Vec2::ZERO;
        }
        keys.reset(KeyCode::Escape);
    }
}

/// A locally handled toggle cannot also become an immediate guest close action.
fn take_packet_toggle(
    keys: &mut Vec<(String, Option<String>, bool)>,
    toggle: &str,
    owned: bool,
    open: bool,
    capture: bool,
) -> bool {
    if (open && (!owned || capture))
        || !keys.iter().any(|(key, _, repeat)| !repeat && key == toggle)
    {
        return false;
    }
    keys.retain(|(key, _, _)| key != toggle);
    true
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

#[cfg(test)]
mod focus_tests;

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn packet_pointer_interruptions_emit_one_cancel_and_require_a_fresh_press() {
        for (open, focused, absorbed, cursor) in [
            (false, true, false, true),
            (true, false, false, true),
            (true, true, true, true),
            (true, true, false, false),
        ] {
            let mut physical = PhysicalControls {
                left_held: true,
                ..Default::default()
            };
            assert_eq!(
                physical.packet_pointer_sample(true, true, false, true, true),
                (true, false)
            );
            assert_eq!(
                physical.packet_pointer_sample(true, true, false, true, false),
                (true, false)
            );
            assert_eq!(
                physical.packet_pointer_sample(open, focused, absorbed, cursor, false),
                (false, true)
            );
            assert_eq!(
                physical.packet_pointer_sample(open, focused, absorbed, cursor, false),
                (false, false),
                "an interruption is an edge"
            );
            assert_eq!(
                physical.packet_pointer_sample(true, true, false, true, false),
                (false, false),
                "holding across refocus or re-entry cannot resume capture"
            );
            assert_eq!(
                physical.packet_pointer_sample(true, true, false, true, true),
                (true, false)
            );
        }
    }

    #[test]
    fn packet_pointer_release_is_a_commit_sample_instead_of_a_cancellation() {
        let mut physical = PhysicalControls {
            left_held: true,
            ..Default::default()
        };
        assert_eq!(
            physical.packet_pointer_sample(true, true, false, true, true),
            (true, false)
        );
        physical.left_held = false;
        assert_eq!(
            physical.packet_pointer_sample(true, true, false, true, false),
            (false, false)
        );
        assert_eq!(
            physical.packet_pointer_sample(false, false, true, false, false),
            (false, false),
            "losing the window after release does not cancel a completed gesture"
        );
    }

    #[test]
    fn shift_snapshot_uses_the_focused_physical_key_owner() {
        let mut physical = PhysicalControls::default();
        physical.track_held("ShiftLeft".into(), true);
        assert!(physical.shift_held(true));
        assert!(!physical.shift_held(false));
        physical.track_held("ShiftRight".into(), true);
        physical.track_held("ShiftLeft".into(), false);
        assert!(
            physical.shift_held(true),
            "either physical Shift key remains held"
        );
        physical.track_held("ShiftRight".into(), false);
        physical.track_held("KeyS".into(), true);
        assert!(!physical.shift_held(true));
    }

    #[test]
    fn native_menu_reopen_consumes_its_toggle_but_focused_capture_keeps_it() {
        let input = || {
            vec![
                ("ShiftRight".into(), None, false),
                ("KeyR".into(), None, false),
            ]
        };
        for owned in [false, true] {
            let mut keys = input();
            assert!(take_packet_toggle(
                &mut keys,
                "ShiftRight",
                owned,
                false,
                true
            ));
            assert_eq!(keys.len(), 1);
            assert_eq!(keys[0].0, "KeyR");
        }
        let mut keys = input();
        assert!(!take_packet_toggle(
            &mut keys,
            "ShiftRight",
            true,
            true,
            true
        ));
        assert_eq!(keys.len(), 2);
        assert!(!take_packet_toggle(
            &mut keys,
            "ShiftRight",
            false,
            true,
            false
        ));
        assert_eq!(keys.len(), 2);
        assert!(take_packet_toggle(
            &mut keys,
            "ShiftRight",
            true,
            true,
            false
        ));
        assert_eq!(keys.len(), 1);
    }

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
            .insert_resource(UiRuntimeResource(UiRuntime::new(1)))
            .insert_resource(client_state::PlayerRuntime(player_state::PlayerState::new(
                1,
            )))
            .insert_resource(UiPresentationResource(
                UiPresentationRuntime::new(ui_engine::test_support::fixture_font()).unwrap(),
            ))
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
        for index in 0..mod_api::MAX_CONTROL_KEYS * 2 {
            physical.track_held(format!("Key{index}"), true);
        }
        assert_eq!(physical.held_keys.len(), mod_api::MAX_CONTROL_KEYS);
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
    fn pointer_observations_count_edges_preserve_holds_and_ignore_other_windows() {
        let mut physical = PhysicalControls::default();
        let mut events = Messages::default();
        let mut world = World::new();
        let window = world.spawn_empty().id();
        let other = world.spawn_empty().id();
        let event = |window, button, state| MouseButtonInput {
            window,
            button,
            state,
        };
        events.write(event(other, MouseButton::Right, ButtonState::Pressed));
        events.write(event(window, MouseButton::Left, ButtonState::Pressed));
        events.write(event(window, MouseButton::Left, ButtonState::Pressed));
        assert_eq!(
            physical.pointer_edges(Some(&events), None, window, true),
            ["MouseLeft"]
        );
        assert!(physical.left_held && physical.held_keys.contains(&"MouseLeft".into()));
        assert!(
            physical
                .pointer_edges(Some(&events), None, window, true)
                .is_empty()
        );
        events.write(event(window, MouseButton::Left, ButtonState::Released));
        events.write(event(window, MouseButton::Left, ButtonState::Pressed));
        events.write(event(window, MouseButton::Right, ButtonState::Pressed));
        assert_eq!(
            physical.pointer_edges(Some(&events), None, window, true),
            ["MouseLeft", "MouseRight"]
        );
        physical.discard_pending(None, Some(&events));
        assert!(physical.held_keys.is_empty() && !physical.left_held);
        events.write(event(window, MouseButton::Left, ButtonState::Pressed));
        assert!(
            physical
                .pointer_edges(Some(&events), None, window, false)
                .is_empty()
        );
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
