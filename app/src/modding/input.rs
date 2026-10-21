//! Routes granted personal controls before the ordinary gameplay sample.

mod field;
mod physical;
use field::PanelSample;

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
    window::{CursorGrabMode, CursorOptions, Ime, PrimaryWindow, WindowEvent},
};
use client_shell::ui_resources::{UiPresentationResource, UiRuntimeResource};
#[cfg(test)]
use client_ui::ui_runtime::{UiRuntime, presentation::UiPresentationRuntime};

/// Physical messages and shared services routed while a personal panel owns input.
#[derive(SystemParam)]
pub(super) struct PanelInput<'w> {
    keyboard_events: Option<Res<'w, Messages<KeyboardInput>>>,
    ime_events: Option<Res<'w, Messages<Ime>>>,
    ordered_events: Option<Res<'w, Messages<WindowEvent>>>,
    clipboard: Option<ResMut<'w, client_shell::menu::MenuClipboard>>,
    mouse_events: Option<Res<'w, Messages<MouseButtonInput>>>,
    scroll: Option<Res<'w, AccumulatedMouseScroll>>,
    reserved: Option<ResMut<'w, super::ReservedControls>>,
}

const POINTER_BUTTONS: [(MouseButton, &str); 3] = [
    (MouseButton::Left, "MouseLeft"),
    (MouseButton::Right, "MouseRight"),
    (MouseButton::Middle, "MouseMiddle"),
];
const TEXT_INPUT: &str = "TextInput";

#[derive(Default)]
pub(super) struct PhysicalControls {
    keys: MessageCursor<KeyboardInput>,
    mouse: MessageCursor<MouseButtonInput>,
    ime: MessageCursor<Ime>,
    ordered: MessageCursor<WindowEvent>,
    ime_owned: bool,
    left_held: bool,
    held_keys: Vec<String>,
    /// Whether `held_keys` was seeded from physical input since the mod attached or refocused.
    held_seeded: bool,
    restore_capture: bool,
    panel_owned: bool,
    /// A packet panel accepted this primary gesture; interruption requires a fresh press.
    packet_pointer_active: bool,
}

/// Applies native editor shortcuts without forwarding command letters or touching unfocused fields.
fn native_editor_shortcut(
    ui: &mut mod_ui::ModUi,
    clipboard: Option<&mut client_shell::menu::MenuClipboard>,
    key: &str,
) -> bool {
    if key == "KeyA" {
        ui.mod_panel_key("SelectAll", None);
        return true;
    }
    if !matches!(key, "KeyC" | "KeyX" | "KeyV") {
        return false;
    }
    let Some(selection) = ui.mod_panel_field_selection() else {
        return true;
    };
    let Some(clipboard) = clipboard else {
        return true;
    };
    if key == "KeyV" {
        let maximum = ui::mod_panel::MAX_PANEL_VALUE_CHARS * char::MAX.len_utf8();
        if let Some(text) = ui::ChatClipboard::read_text_bounded(clipboard, maximum)
            .ok()
            .flatten()
        {
            ui.mod_panel_key("Paste", Some(&text));
        }
    } else if !selection.is_empty() {
        clipboard.write_text(selection.to_owned());
        if key == "KeyX" {
            ui.mod_panel_key("Delete", None);
        }
    }
    true
}

#[allow(
    clippy::too_many_arguments,
    reason = "Routes one physical frame before gameplay authority."
)]
pub(super) fn prepare_mod_input(
    extension: Option<ResMut<ModRuntime>>,
    mut packets: Option<ResMut<super::packets::PacketMods>>,
    mut physical: Local<PhysicalControls>,
    input: PanelInput,
    mut keys: ResMut<ButtonInput<KeyCode>>,
    mut mouse: Option<ResMut<ButtonInput<MouseButton>>>,
    mut motion: Option<ResMut<AccumulatedMouseMotion>>,
    mut windows: Query<(Entity, &mut Window, &mut CursorOptions), With<PrimaryWindow>>,
    ui: Res<UiRuntimeResource>,
    player: Res<client_state::PlayerRuntime>,
    menu: Option<Res<MenuRuntime>>,
    mut presentation: ResMut<UiPresentationResource>,
    time: Option<Res<Time>>,
    mut focus: Option<ResMut<client_state::CursorFocus>>,
    driven: Option<Res<client_state::DrivenInput>>,
) {
    let PanelInput {
        keyboard_events,
        ime_events,
        ordered_events,
        mut clipboard,
        mouse_events,
        scroll,
        mut reserved,
    } = input;
    if let Some(reserved) = reserved.as_mut() {
        reserved.0.clear();
    }
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
        if let Some(events) = &ime_events {
            physical.ime.clear(events);
        }
        if let Some(events) = &ordered_events {
            physical.ordered.clear(events);
        }
        if !physical.panel_owned && !physical.ime_owned {
            return;
        }
    }
    let Ok((entity, mut window, mut cursor)) = windows.single_mut() else {
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
    for sample in field::samples(
        &mut physical,
        ordered_events.as_deref(),
        keyboard_events.as_deref(),
        ime_events.as_deref(),
        entity,
    ) {
        let field::RawSample::Key(event) = sample else {
            if let field::RawSample::Ime(event) = sample {
                panel_keys.push(PanelSample::Ime(event));
            }
            continue;
        };
        let identified = !matches!(event.key_code, KeyCode::Unidentified(_));
        if !identified {
            continue;
        }
        let name = format!("{:?}", event.key_code);
        let down = event.state == ButtonState::Pressed;
        physical.track_held(name.clone(), down);
        if down
            && !event.repeat
            && extension.as_ref().is_some_and(|extension| {
                (0..extension.hosts.host_count()).any(|index| {
                    extension
                        .hosts
                        .host(index)
                        .panel()
                        .is_some_and(|panel| panel.is_toggle_key(&name))
                })
            })
        {
            debug!(key = %name, focused, "personal panel hotkey received");
        }
        if down && focused {
            panel_keys.push(PanelSample::Key((
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
                field::modifiers(&physical.held_keys),
            )));
        }
    }
    let pointer_pressed =
        physical.pointer_edges(mouse_events.as_deref(), mouse.as_deref(), entity, focused);
    if !focused {
        physical.left_held = false;
        physical.held_keys.clear();
        physical.held_seeded = false;
    }
    let primary_pressed = pointer_pressed.iter().any(|key| key == "MouseLeft");
    panel_keys.extend(
        pointer_pressed
            .iter()
            .cloned()
            .map(|key| PanelSample::Key((key, None, false, Default::default()))),
    );
    let absorbed = menu.as_deref().map_or_else(
        || ui.ui_focused(&player),
        |menu| presentation.base_absorbs_gameplay_input(&player, &ui, menu),
    );
    if packet_available
        && focused
        && !absorbed
        && let Some(packets) = packets.as_mut()
    {
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
            let subscriptions = packets
                .menu
                .retained
                .as_ref()
                .map(|retained| retained.output.keys.clone())
                .unwrap_or_default();
            packets.menu.pending.keys = panel_keys
                .iter()
                .filter_map(PanelSample::key)
                .filter(|(key, _, repeat, _)| !repeat && subscriptions.contains(key))
                .take(mod_api::MAX_CONTROL_KEYS)
                .map(|(key, _, _, _)| mod_api::native_menu::Key {
                    key: key.clone(),
                    text: None,
                })
                .collect();
            consume_reserved(
                &mut keys,
                mouse.as_deref_mut(),
                &subscriptions,
                toggle.as_deref(),
                reserved.as_deref_mut(),
            );
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
    // Native HUD geometry stays host-owned; guests cannot manufacture keep-out regions.
    if presentation.mod_ui().mod_hud_editor_open() {
        let region = presentation.mod_hud_editor_native_keep_out(ui.session_id());
        presentation
            .mod_ui_mut()
            .set_mod_hud_editor_keep_out(region.as_slice());
        presentation
            .mod_ui_mut()
            .set_mod_hud_editor_shift(physical.shift_held(focused));
    }
    if extension.is_none() && !packet_owned && !physical.panel_owned && !physical.ime_owned {
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
        for sample in panel_keys {
            let Some((key, text, _, modifiers)) =
                field::route_sample(sample, presentation.mod_ui_mut(), focused && !absorbed)
            else {
                continue;
            };
            if !open || !focused || absorbed {
                continue;
            }
            if editing {
                if (modifiers.control || modifiers.super_key)
                    && native_editor_shortcut(
                        presentation.mod_ui_mut(),
                        clipboard.as_deref_mut(),
                        &key,
                    )
                {
                    continue;
                }
                let text = if (modifiers.control || modifiers.super_key) && key != TEXT_INPUT {
                    Some("")
                } else {
                    text.as_deref()
                };
                events.extend(
                    presentation
                        .mod_ui_mut()
                        .mod_panel_field_key(&key, text, modifiers),
                );
            } else if key == "Escape" && !capture {
                open = false;
            } else if key != TEXT_INPUT {
                raw_keys.push(mod_api::native_menu::Key {
                    key,
                    text: text
                        .filter(|text| text.len() <= 256 && !text.chars().any(char::is_control)),
                });
            }
        }
        field::sync_ime(
            &mut physical,
            &mut window,
            presentation.mod_ui_mut(),
            focused && !absorbed,
        );
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
            preview_sizes: Default::default(),
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
        if was_open
            && !open
            && focused
            && !absorbed
            && let Some(focus) = focus.as_deref_mut()
        {
            focus.authorize_screen_return();
        }
        let restore = physical.finish_panel(
            open,
            focused,
            absorbed,
            crate::camera::mouse_input_active(&window, &cursor, focus.as_deref(), driven.is_some()),
        );
        finish_capture(
            open,
            was_open,
            restore,
            &mut keys,
            mouse.as_deref_mut(),
            motion.as_deref_mut(),
            &mut cursor,
        );
        return;
    }
    let Some(mut extension) = extension else {
        field::sync_ime(&mut physical, &mut window, presentation.mod_ui_mut(), false);
        let absorbed = menu.as_deref().map_or_else(
            || ui.ui_focused(&player),
            |menu| presentation.base_absorbs_gameplay_input(&player, &ui, menu),
        );
        let restore = physical.finish_panel(
            false,
            focused,
            absorbed,
            crate::camera::mouse_input_active(&window, &cursor, focus.as_deref(), driven.is_some()),
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
            .filter_map(PanelSample::key)
            .filter(|(key, _, repeat, _)| key != TEXT_INPUT && !repeat)
            .map(|(key, _, _, _)| key.clone())
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
        && panel_keys
            .iter()
            .filter_map(PanelSample::key)
            .any(|(key, _, repeat, _)| {
                key != TEXT_INPUT
                    && !repeat
                    && (key == ui::mod_panel::FALLBACK_TOGGLE_KEY
                        || presentation.mod_ui().mod_panel_toggle_key() == Some(key.as_str()))
            });
    if !focused {
        presentation.mod_ui_mut().cancel_mod_panel_pointer_input();
    }
    if absorbed || interrupt && !presentation.mod_ui().mod_hud_editor_open() {
        presentation.mod_ui_mut().cancel_mod_panel_edit();
    }
    let mut pressed = pointer_pressed;
    let mut events = Vec::new();
    for sample in panel_keys {
        let Some((key, text, repeat, modifiers)) =
            field::route_sample(sample, presentation.mod_ui_mut(), focused && !absorbed)
        else {
            continue;
        };
        if POINTER_BUTTONS.iter().any(|(_, name)| *name == key) {
            continue;
        }
        if open && focused && !absorbed && editing && !interrupt {
            if (modifiers.control || modifiers.super_key)
                && native_editor_shortcut(presentation.mod_ui_mut(), clipboard.as_deref_mut(), &key)
            {
                continue;
            }
            // An empty text value also suppresses the editor's physical digit fallback.
            let text = if (modifiers.control || modifiers.super_key) && key != TEXT_INPUT {
                Some("")
            } else {
                text.as_deref()
            };
            events.extend(
                presentation
                    .mod_ui_mut()
                    .mod_panel_field_key(&key, text, modifiers),
            );
        } else if key != TEXT_INPUT
            && !repeat
            && (!editing
                || key == ui::mod_panel::FALLBACK_TOGGLE_KEY
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
    let toggle_requested = extension.hosts.host(owner).panel().is_some_and(|panel| {
        // An open capture receives menu shortcuts for guest conflict validation.
        !(had_panel && panel.capture_key) && pressed.iter().any(|key| panel.is_toggle_key(key))
    });
    if !extension.hosts.host(owner).is_active() || (focused && (absorbed || close_requested)) {
        open = false;
    } else if toggle_requested {
        open = !open;
    }
    if toggle_requested {
        debug!(focused, absorbed, open, "personal panel hotkey resolved");
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
        crate::camera::mouse_input_active(&window, &cursor, focus.as_deref(), driven.is_some()),
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
                .filter(|key| !absorbed || key == ui::mod_panel::FALLBACK_TOGGLE_KEY)
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
    field::sync_ime(
        &mut physical,
        &mut window,
        presentation.mod_ui_mut(),
        focused && !absorbed,
    );
    let text_events = presentation
        .mod_ui_mut()
        .take_mod_panel_text_events(extension.panel_ui_owner(owner));
    if focused
        && !absorbed
        && !text_events.is_empty()
        && let Err(error) = extension
            .hosts
            .host_mut(owner)
            .queue_text_events(text_events)
    {
        warn!(%error,"native mod text edit rejected");
    }
    let field_events = presentation
        .mod_ui_mut()
        .take_mod_panel_field_events(extension.panel_ui_owner(owner));
    if focused
        && !absorbed
        && !field_events.is_empty()
        && let Err(error) = extension
            .hosts
            .host_mut(owner)
            .queue_field_events(field_events)
    {
        warn!(%error, "native mod field input rejected");
    }
    finish_capture(
        open,
        was_open,
        restore,
        &mut keys,
        mouse.as_deref_mut(),
        motion.as_deref_mut(),
        &mut cursor,
    );
    if !open {
        let mut subscriptions = extension.hosts.reserved_keys();
        for index in 0..extension.hosts.host_count() {
            if let Some(panel) = extension.hosts.host(index).panel() {
                subscriptions.push(panel.toggle_key.clone());
                subscriptions.push(ui::mod_panel::FALLBACK_TOGGLE_KEY.into());
            }
        }
        let toggle = extension
            .hosts
            .host(owner)
            .panel()
            .map(|panel| panel.toggle_key.as_str());
        consume_reserved(
            &mut keys,
            mouse.as_deref_mut(),
            &subscriptions,
            toggle,
            reserved.as_deref_mut(),
        );
    }
}

/// Applies shared pointer capture and input consumption for every native personal panel.
fn finish_capture(
    open: bool,
    was_open: bool,
    restore: bool,
    keys: &mut ButtonInput<KeyCode>,
    mut mouse: Option<&mut ButtonInput<MouseButton>>,
    mut motion: Option<&mut AccumulatedMouseMotion>,
    cursor: &mut Mut<CursorOptions>,
) {
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
    keys: &mut Vec<PanelSample>,
    toggle: &str,
    owned: bool,
    open: bool,
    capture: bool,
) -> bool {
    if (open && (!owned || capture))
        || !keys
            .iter()
            .filter_map(PanelSample::key)
            .any(|(key, _, repeat, _)| !repeat && key == toggle)
    {
        return false;
    }
    keys.retain(|sample| sample.key().is_none_or(|(key, _, _, _)| key != toggle));
    true
}

/// Removes reserved held state and edges from both gameplay input paths.
fn consume_reserved(
    keys: &mut ButtonInput<KeyCode>,
    mouse: Option<&mut ButtonInput<MouseButton>>,
    reserved: &[String],
    toggle: Option<&str>,
    mut consumed: Option<&mut super::ReservedControls>,
) {
    let is_reserved = |name: &str| reserved.iter().any(|key| key == name) || toggle == Some(name);
    let consumed_keys: Vec<_> = keys
        .get_pressed()
        .chain(keys.get_just_pressed())
        .chain(keys.get_just_released())
        .copied()
        .filter(|key| {
            let name = format!("{key:?}");
            is_reserved(&name)
        })
        .collect();
    for key in consumed_keys {
        keys.reset(key);
    }
    if let Some(mouse) = mouse {
        for (button, name) in POINTER_BUTTONS {
            if is_reserved(name) {
                mouse.reset(button);
            }
        }
    }
    if let Some(consumed) = consumed.as_mut() {
        consumed.0.extend_from_slice(reserved);
        consumed.0.extend(toggle.map(str::to_owned));
    }
}

#[cfg(test)]
mod focus_tests;

#[cfg(test)]
mod subscription_tests;

#[cfg(test)]
mod tests;
