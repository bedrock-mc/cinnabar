mod chat;
mod chat_coordinates;
mod chat_modifiers;
pub(crate) use chat::drive_chat_ui_actions;

use bevy::{
    ecs::message::{MessageCursor, Messages},
    input::{
        ButtonState,
        gamepad::{Gamepad, GamepadButton},
        keyboard::KeyboardInput,
        mouse::{
            AccumulatedMouseMotion, AccumulatedMouseScroll, MouseButtonInput, MouseScrollUnit,
            MouseWheel,
        },
        touch::Touches,
    },
    math::Vec2,
    prelude::{
        ButtonInput, KeyCode, Local, MessageReader, MouseButton, Query, Res, ResMut, Single, Time,
        With,
    },
    time::Real,
    window::{CursorGrabMode, CursorOptions, PrimaryWindow, Window},
};

use crate::menu::settings_options::{
    binding_gamepad, binding_key, binding_mouse, binding_pressed, hotbar_control_slot,
};
use ui::{ChatEditor, PointerPhase, UiAction, UiPoint};

use client_ui::ui_runtime::inventory_ledger::DropSource;
use client_ui::ui_runtime::presentation::{
    UiPresentationRuntime, inventory_pointer::InventoryScreen,
};
use client_ui::ui_runtime::{PlatformClipboard, UiRuntime};

use client_ui::ui_runtime::interaction::{
    ChatFlushError, dispatch_chat_ui_action, dispatch_inventory_hotbar, dispatch_inventory_key,
    flush_chat_sends, flush_inventory_send, gamepad_chat_action, is_chat_edit_shortcut,
    paste_chat_shortcut, restore_gameplay_input_after_chat, suppress_gameplay_input_for_chat,
    suppress_gameplay_input_for_inventory,
};

pub(crate) fn flush_inventory_network(
    mut player_runtime: bevy::prelude::ResMut<crate::player_runtime::PlayerRuntime>,
    time: Res<Time<Real>>,
    mut runtime: ResMut<UiRuntime>,
    network: Res<crate::runtime::network::NetworkHandle>,
) {
    let now_millis = u64::try_from(time.elapsed().as_millis()).unwrap_or(u64::MAX);
    match flush_inventory_send(&mut player_runtime, &mut runtime, now_millis, |packet| {
        network.send_inventory_packet(packet)
    }) {
        Ok(_) | Err(crate::runtime::network::PacketSendError::Full(_)) => {}
        Err(crate::runtime::network::PacketSendError::Closed(_)) => {
            runtime.inventory_transport_closed(&mut player_runtime);
        }
    }
    while let Some(packet) = runtime.take_client_packet() {
        match network.send_inventory_packet(packet) {
            Ok(()) => {}
            Err(crate::runtime::network::PacketSendError::Full(packet)) => {
                runtime.requeue_client_packet(packet);
                break;
            }
            Err(crate::runtime::network::PacketSendError::Closed(_)) => break,
        }
    }
}

pub(crate) fn flush_chat_network(
    player_runtime: bevy::prelude::Res<crate::player_runtime::PlayerRuntime>,
    mut runtime: ResMut<UiRuntime>,
    network: Res<crate::runtime::network::NetworkHandle>,
    mut client_world: ResMut<crate::runtime::world::ClientWorld>,
) {
    runtime.service_pending_chat_autocomplete(&player_runtime);
    if network.closed_command_has_pending_control() {
        return;
    }
    let runtime_id = runtime.local_runtime_id(&player_runtime);
    runtime.flush_wake_request(runtime_id, |packet| network.send_inventory_packet(packet));
    match flush_chat_sends(
        &mut runtime,
        8,
        |session, sequence, action, packet| match network
            .send_chat_packet(session, sequence, action, packet)
        {
            Err(crate::runtime::network::PacketSendError::Closed(packet))
                if network.closed_command_has_pending_control() =>
            {
                Err(crate::runtime::network::PacketSendError::Full(packet))
            }
            result => result,
        },
    ) {
        Ok(_)
        | Err(ChatFlushError::Transport(crate::runtime::network::PacketSendError::Full(_))) => {}
        Err(ChatFlushError::Transport(crate::runtime::network::PacketSendError::Closed(_))) => {
            crate::runtime::shutdown::record_fatal_error(
                &mut client_world.fatal_error,
                "chat send failed because the network command channel closed".to_owned(),
            );
        }
        Err(ChatFlushError::Packet(error)) => {
            crate::runtime::shutdown::record_fatal_error(
                &mut client_world.fatal_error,
                format!("queued chat packet became invalid: {error}"),
            );
        }
        Err(ChatFlushError::SessionChanged { expected, actual }) => {
            crate::runtime::shutdown::record_fatal_error(
                &mut client_world.fatal_error,
                format!(
                    "queued chat packet crossed a session boundary: expected {expected}, got {actual}"
                ),
            );
        }
    }
}

#[allow(clippy::too_many_arguments)]
pub(crate) fn drive_inventory_ui_actions(
    mut player_runtime: bevy::prelude::ResMut<crate::player_runtime::PlayerRuntime>,
    time: Option<Res<Time<Real>>>,
    window: Single<&Window, With<PrimaryWindow>>,
    menu: Option<Res<crate::menu::MenuRuntime>>,
    mut mouse_buttons: ResMut<ButtonInput<MouseButton>>,
    mouse_messages: Option<Res<Messages<MouseButtonInput>>>,
    mut release_cursor: Local<MessageCursor<MouseButtonInput>>,
    wheel_messages: Option<Res<Messages<MouseWheel>>>,
    mut wheel_cursor: Local<MessageCursor<MouseWheel>>,
    presentation: Res<UiPresentationRuntime>,
    mut runtime: ResMut<UiRuntime>,
) {
    let notches: Vec<(f32, MouseScrollUnit)> = wheel_messages
        .as_deref()
        .map(|messages| {
            wheel_cursor
                .read(messages)
                .map(|event| (event.y, event.unit))
                .collect()
        })
        .unwrap_or_default();
    // Button state is reset below while the inventory owns the pointer, so a later
    // physical release no longer surfaces as `just_released`; read raw releases too.
    let (mut raw_primary_release, mut raw_secondary_release) = (false, false);
    if let Some(messages) = mouse_messages.as_deref() {
        for input in release_cursor.read(messages) {
            if input.state == ButtonState::Released {
                match input.button {
                    MouseButton::Left => raw_primary_release = true,
                    MouseButton::Right => raw_secondary_release = true,
                    _ => {}
                }
            }
        }
    }
    // Presses are this frame's only; modifiers stay held across frames.
    let (presses, shift, control) = runtime.inventory_keys_mut().take_frame();
    if runtime.credits().owns_input() || runtime.server_forms().owns_input() {
        return;
    }
    if menu.as_ref().is_some_and(|menu| menu.is_visible())
        || !runtime.inventory_open()
        || !window.focused
    {
        runtime.set_inventory_pointer_gui(None);
        runtime.screen_state_mut().hover = None;
        runtime.screen_state_mut().pointer.reset();
        if !runtime.inventory_open() {
            runtime.screen_state_mut().book = None;
        }
        return;
    }
    let now_millis = time.map_or(0, |time| {
        u64::try_from(time.elapsed().as_millis()).unwrap_or(u64::MAX)
    });
    let primary_pressed = mouse_buttons.just_pressed(MouseButton::Left);
    let secondary_pressed = mouse_buttons.just_pressed(MouseButton::Right);
    let primary_released = mouse_buttons.just_released(MouseButton::Left) || raw_primary_release;
    let secondary_released =
        mouse_buttons.just_released(MouseButton::Right) || raw_secondary_release;
    let mouse_hotbar = mouse_buttons.get_just_pressed().find_map(|button| {
        let code = crate::semantic_controls::physical::mouse_button_code(*button)?;
        hotbar_control_slot(
            menu.as_deref(),
            semantic_input::PhysicalControl::MouseButton(code),
        )
    });
    // The inventory owns pointer buttons while open. Preserve the edges long
    // enough to resolve their cell, then clear every button before gameplay
    // systems can observe this frame.
    mouse_buttons.reset_all();
    let generation = runtime
        .inventory_ledger(&player_runtime)
        .storage_generation();
    runtime.screen_state_mut().observe_window(generation);
    let screen = InventoryScreen::of_runtime(&player_runtime, &runtime);
    if screen != InventoryScreen::Creative {
        runtime.screen_state_mut().search_focused = false;
    }
    let Some(position) = window.cursor_position() else {
        runtime.set_inventory_pointer_gui(None);
        runtime.screen_state_mut().hover = None;
        return;
    };
    let Ok(point) = UiPoint::new(position.x, position.y) else {
        runtime.set_inventory_pointer_gui(None);
        runtime.screen_state_mut().hover = None;
        return;
    };
    let physical_size = [window.physical_width(), window.physical_height()];
    let gui = presentation.inventory_gui_point(point, physical_size, window.scale_factor());
    runtime.set_inventory_pointer_gui(gui);
    let book_open = runtime.screen_state().book_open;
    let reader_mode = runtime
        .screen_state()
        .book
        .as_ref()
        .map(|book| (book.editable, book.signing));
    let hit = gui.and_then(|gui| {
        if let Some((editable, signing)) = reader_mode {
            return presentation.inventory_reader_hit(
                gui,
                physical_size,
                window.scale_factor(),
                editable,
                signing,
            );
        }
        presentation
            .inventory_book_hit(gui, physical_size, window.scale_factor(), screen, book_open)
            .or_else(|| {
                presentation.inventory_cell_hit(gui, physical_size, window.scale_factor(), screen)
            })
    });
    runtime.screen_state_mut().hover = hit;
    if let (Some(gui), Some(frame)) = (gui, presentation.engine_container_frame()) {
        scroll_container(&mut runtime, frame, gui, &notches);
    }
    for key in presses {
        let slot = crate::semantic_controls::keyboard_usage(key).and_then(|code| {
            hotbar_control_slot(
                menu.as_deref(),
                semantic_input::PhysicalControl::KeyboardUsage(code),
            )
        });
        let drop = binding_key(menu.as_deref(), "key.drop", key);
        let _ = dispatch_inventory_key(
            &mut player_runtime,
            runtime.as_mut(),
            hit,
            key,
            control,
            slot,
            drop,
        );
    }
    if let Some(slot) = mouse_hotbar {
        let _ = dispatch_inventory_hotbar(&mut player_runtime, runtime.as_mut(), hit, slot);
    }
    let frame = super::inventory_drag::PointerFrame {
        primary_pressed,
        primary_released,
        secondary_pressed,
        secondary_released,
        shift,
        holding: runtime
            .inventory_ledger(&player_runtime)
            .cursor_stack()
            .is_some(),
        hit,
        now_millis,
    };
    let actions = runtime.screen_state_mut().pointer.step(frame);
    for action in actions {
        runtime.perform_pointer_action(&mut player_runtime, action);
    }
    if hit.is_none() {
        // A held stack released outside the panel is dropped: all of it on a
        // primary click, one item on a secondary click.
        let outside = gui.is_some_and(|gui| {
            !presentation.inventory_panel_contains(
                gui,
                physical_size,
                window.scale_factor(),
                screen,
            )
        });
        if outside && (primary_pressed || secondary_pressed) {
            let amount = (!primary_pressed).then_some(1);
            let _ = runtime
                .inventory_ledger_mut(&mut player_runtime)
                .begin_drop(DropSource::Cursor, amount);
        }
    }
}

/// Wheel notches over an engine-drawn screen scroll the view under the pointer.
fn scroll_container(
    runtime: &mut UiRuntime,
    frame: &client_ui::ui_runtime::forms::EngineFrame,
    gui: [f32; 2],
    notches: &[(f32, MouseScrollUnit)],
) {
    let point = [f64::from(gui[0]), f64::from(gui[1])];
    let Some(view) = json_ui::scroll_target(&frame.hits, &frame.report, point) else {
        return;
    };
    let Some(metrics) = frame.report.scrolls.get(&view.key) else {
        return;
    };
    let mut offset = runtime
        .screen_state()
        .container_scroll
        .get(&view.key)
        .copied()
        .unwrap_or(metrics.offset);
    for (notch, unit) in notches {
        let at = json_ui::ScrollMetrics {
            offset,
            ..metrics.clone()
        };
        offset = match unit {
            MouseScrollUnit::Line => at.offset_for_wheel(f64::from(*notch)),
            MouseScrollUnit::Pixel => {
                (offset - f64::from(*notch / frame.scale)).clamp(0.0, metrics.max_offset())
            }
        };
    }
    if !notches.is_empty() {
        runtime
            .screen_state_mut()
            .container_scroll
            .insert(view.key.clone(), offset);
    }
}

/// In-world inventory keys: Q drops one item from the selected hotbar cell
/// (Control+Q the whole stack) and a right-click with a book in hand opens it.
#[allow(
    clippy::too_many_arguments,
    reason = "Player authority is borrowed separately from UI state."
)]
pub(crate) fn drive_world_inventory_keys(
    mut player_runtime: bevy::prelude::ResMut<crate::player_runtime::PlayerRuntime>,
    gamepads: Query<&Gamepad>,
    window: Single<&Window, With<PrimaryWindow>>,
    keys: Res<ButtonInput<KeyCode>>,
    mouse: Res<ButtonInput<MouseButton>>,
    menu: Option<Res<crate::menu::MenuRuntime>>,
    presentation: Option<Res<UiPresentationRuntime>>,
    mut runtime: ResMut<UiRuntime>,
    emote_input: Option<Res<super::emotes::EmoteInputConsumed>>,
) {
    if emote_input.is_some_and(|consumed| consumed.0) {
        return;
    }
    let drop = binding_pressed(menu.as_deref(), "key.drop", &keys, &mouse)
        || binding_gamepad(menu.as_deref(), "key.drop", &gamepads);
    let use_book = binding_pressed(menu.as_deref(), "key.use", &keys, &mouse);
    if !window.focused
        || crate::screen_policy::absorbs_input(
            &player_runtime,
            Some(&runtime),
            menu.as_deref(),
            presentation.as_deref(),
        )
        || !(drop || use_book)
        || player_runtime
            .facts
            .player_game_mode()
            .is_some_and(|mode| !mode.shows_hotbar())
    {
        return;
    }
    if use_book {
        runtime.open_held_book(&player_runtime);
        return;
    }
    let Some(slot) = player_runtime.selected_hotbar_slot() else {
        return;
    };
    let control = keys.pressed(KeyCode::ControlLeft) || keys.pressed(KeyCode::ControlRight);
    let _ = runtime
        .inventory_ledger_mut(&mut player_runtime)
        .begin_world_drop(slot, (!control).then_some(1));
}

#[allow(clippy::too_many_arguments)]
pub(crate) fn drive_chat_keyboard_input(
    mut player_runtime: bevy::prelude::ResMut<crate::player_runtime::PlayerRuntime>,
    gamepads: Query<&Gamepad>,
    mut keyboard_messages: MessageReader<KeyboardInput>,
    time: Res<Time<Real>>,
    window: Single<(&Window, &mut CursorOptions), With<PrimaryWindow>>,
    menu: Option<Res<crate::menu::MenuRuntime>>,
    mut keys: ResMut<ButtonInput<KeyCode>>,
    mut mouse_buttons: ResMut<ButtonInput<MouseButton>>,
    mut mouse_motion: ResMut<AccumulatedMouseMotion>,
    mut runtime: ResMut<UiRuntime>,
    mut presentation: Option<ResMut<UiPresentationRuntime>>,
    mut clipboard: Option<ResMut<crate::menu::MenuClipboard>>,
    mut modifiers: Local<ButtonInput<KeyCode>>,
    emote_input: Option<Res<super::emotes::EmoteInputConsumed>>,
) {
    let (window, mut cursor) = window.into_inner();
    if runtime.credits().owns_input() {
        let now = u64::try_from(time.elapsed().as_millis()).unwrap_or(u64::MAX);
        let finished = runtime.credits().active().is_some_and(|active| {
            presentation
                .as_ref()
                .is_some_and(|view| view.credits_finished(runtime.session_id(), active.sequence))
        });
        runtime.credits_mut().observe(now, finished);
        if window.focused {
            let cancel = keys.just_pressed(KeyCode::Escape)
                || gamepads
                    .iter()
                    .any(|pad| pad.just_pressed(GamepadButton::East));
            let select = keys.just_pressed(KeyCode::Enter)
                || keys.just_pressed(KeyCode::Space)
                || gamepads
                    .iter()
                    .any(|pad| pad.just_pressed(GamepadButton::South));
            if mouse_buttons.just_pressed(MouseButton::Left) {
                let skip_hit = runtime.credits().active().is_some_and(|active| {
                    active.skip_visible(now)
                        && window.cursor_position().is_some_and(|point| {
                            presentation.as_ref().is_some_and(|view| {
                                view.credits_skip_contains(
                                    runtime.session_id(),
                                    active.sequence,
                                    point.to_array(),
                                )
                            })
                        })
                });
                if skip_hit {
                    runtime.credits_mut().skip(now);
                } else {
                    runtime.credits_mut().select(now, false);
                }
            } else if cancel || select {
                runtime.credits_mut().select(now, cancel);
            }
        }
        modifiers.reset_all();
        keyboard_messages.clear();
        keys.reset_all();
        mouse_buttons.reset_all();
        mouse_motion.delta = Vec2::ZERO;
        cursor.grab_mode = CursorGrabMode::None;
        cursor.visible = true;
        return;
    }
    if !window.focused {
        modifiers.reset_all();
        keyboard_messages.clear();
        keys.reset_all();
        mouse_buttons.reset_all();
        mouse_motion.delta = Vec2::ZERO;
        cursor.grab_mode = CursorGrabMode::None;
        cursor.visible = true;
        return;
    }
    if runtime.server_forms().owns_input()
        || runtime.emotes().is_open()
        || emote_input.is_some_and(|consumed| consumed.0)
    {
        modifiers.reset_all();
        keyboard_messages.clear();
        return;
    }
    if menu.as_ref().is_some_and(|menu| menu.is_visible()) {
        modifiers.reset_all();
        if runtime.inventory_open() {
            runtime.close_inventory(&mut player_runtime);
        }
        keyboard_messages.clear();
        // The menu system runs next and must see the original button state.
        // It consumes keyboard/pointer input after handling its own actions.
        mouse_motion.delta = Vec2::ZERO;
        cursor.grab_mode = CursorGrabMode::None;
        cursor.visible = true;
        return;
    }
    // An already-open inventory owns this frame's pointer edge. Keyboard
    // transitions below may close it or open a new UI, so both sides of the
    // transition are checked before preserving that edge for the inventory
    // system later in the production chain.
    let inventory_owned_pointer = runtime.inventory_open();
    let mut inventory_ownership_changed = false;
    let mut consumed_gameplay = runtime.ui_focused(&player_runtime);
    if !runtime.chat_focused() && !runtime.screen_state().text_focused() {
        if binding_mouse(menu.as_deref(), "key.inventory", &mouse_buttons)
            || binding_gamepad(menu.as_deref(), "key.inventory", &gamepads)
        {
            runtime.toggle_inventory(&mut player_runtime);
            inventory_ownership_changed = true;
            consumed_gameplay = true;
        } else if !runtime.inventory_open()
            && (binding_mouse(menu.as_deref(), "key.chat", &mouse_buttons)
                || binding_gamepad(menu.as_deref(), "key.chat", &gamepads))
        {
            runtime.open_chat(&mut player_runtime);
            consumed_gameplay = true;
        } else if !runtime.inventory_open()
            && binding_mouse(menu.as_deref(), "key.command", &mouse_buttons)
        {
            runtime.open_chat(&mut player_runtime);
            let _ = runtime.insert_chat_text("/");
            consumed_gameplay = true;
        }
    }
    chat_modifiers::capture(&mut modifiers, &keys);
    for input in keyboard_messages.read() {
        chat_modifiers::track(&mut modifiers, input);
        runtime.inventory_keys_mut().track_modifier(input);
        if input.state != ButtonState::Pressed {
            continue;
        }
        if let Some(presentation) = presentation.as_deref_mut()
            && presentation.chat_link_confirmation_open()
        {
            consumed_gameplay = true;
            if input.key_code == KeyCode::Escape {
                presentation.cancel_chat_link();
            }
            continue;
        }
        if let Some(presentation) = presentation.as_deref_mut()
            && presentation.chat_settings_open()
        {
            consumed_gameplay = true;
            if input.key_code == KeyCode::Escape {
                presentation.set_chat_settings_open(false);
            }
            continue;
        }
        if runtime.inventory_open() {
            consumed_gameplay = true;
            if runtime.screen_state().text_focused() {
                // A text field owns typed text, including `e`.
                match input.key_code {
                    KeyCode::Escape => {
                        if runtime.commit_book(false) {
                            runtime.close_inventory(&mut player_runtime);
                            inventory_ownership_changed = true;
                        }
                    }
                    KeyCode::Backspace => runtime.screen_state_mut().backspace_text(),
                    key if runtime.book_key(&mut player_runtime, key) => {}
                    _ => {
                        let modified = chat_modifiers::text_modified(&modifiers);
                        if !modified && let Some(text) = input.text.as_deref() {
                            runtime.screen_state_mut().type_text(text);
                        }
                    }
                }
                continue;
            }
            match input.key_code {
                key if binding_key(menu.as_deref(), "key.inventory", key) => {
                    runtime.toggle_inventory(&mut player_runtime);
                    inventory_ownership_changed = true;
                }
                KeyCode::Escape => {
                    runtime.close_inventory(&mut player_runtime);
                    inventory_ownership_changed = true;
                }
                key => runtime.inventory_keys_mut().press(key),
            }
            continue;
        }
        if runtime.local_sleeping() && !runtime.chat_focused() {
            // The bed screen: Escape leaves the bed, T opens chat over it.
            match input.key_code {
                KeyCode::Escape => runtime.request_wake(),
                key if binding_key(menu.as_deref(), "key.chat", key) => {
                    runtime.open_chat(&mut player_runtime);
                }
                key if binding_key(menu.as_deref(), "key.command", key) => {
                    runtime.open_chat(&mut player_runtime);
                    let _ = runtime.insert_chat_text("/");
                }
                _ => {}
            }
            continue;
        }
        if !runtime.chat_focused() {
            match input.key_code {
                key if binding_key(menu.as_deref(), "key.inventory", key) => {
                    runtime.toggle_inventory(&mut player_runtime);
                    inventory_ownership_changed = true;
                    consumed_gameplay = true;
                }
                key if binding_key(menu.as_deref(), "key.chat", key) => {
                    runtime.open_chat(&mut player_runtime);
                    consumed_gameplay = true;
                }
                key if binding_key(menu.as_deref(), "key.command", key) => {
                    runtime.open_chat(&mut player_runtime);
                    let _ = runtime.insert_chat_text("/");
                    consumed_gameplay = true;
                }
                _ => {}
            }
            continue;
        }

        consumed_gameplay = true;
        let selecting =
            modifiers.pressed(KeyCode::ShiftLeft) || modifiers.pressed(KeyCode::ShiftRight);
        if is_chat_edit_shortcut(&modifiers) {
            match input.key_code {
                KeyCode::KeyA => {
                    runtime.mutate_chat_editor(|editor| {
                        editor.move_home(false);
                        editor.move_end(true);
                    });
                    continue;
                }
                KeyCode::KeyC => {
                    if let Some(selection) = runtime.chat_editor().selection() {
                        let text = runtime.chat_editor().as_str()[selection].to_owned();
                        if let Some(clipboard) = clipboard.as_deref_mut() {
                            clipboard.write_text(text);
                        } else {
                            let _ = PlatformClipboard.write_text(text);
                        }
                    }
                    continue;
                }
                _ => {}
            }
        }
        let pasted = if let Some(clipboard) = clipboard.as_deref_mut() {
            paste_chat_shortcut(&mut runtime, input.key_code, &modifiers, clipboard)
        } else {
            paste_chat_shortcut(
                &mut runtime,
                input.key_code,
                &modifiers,
                &mut PlatformClipboard,
            )
        };
        if pasted {
            continue;
        }
        match input.key_code {
            KeyCode::Escape => {
                runtime.close_chat();
            }
            KeyCode::Enter | KeyCode::NumpadEnter => {
                // Enter always sends; Tab completes.
                let now_millis = u64::try_from(time.elapsed().as_millis()).unwrap_or(u64::MAX);
                if runtime.queue_chat_send(now_millis).is_ok() {
                    runtime.close_chat();
                }
            }
            KeyCode::Backspace => runtime.backspace_chat_text(),
            KeyCode::Delete => runtime.delete_chat_text(),
            KeyCode::ArrowLeft => {
                if selecting {
                    runtime.mutate_chat_editor(ChatEditor::select_left);
                } else {
                    runtime.move_chat_cursor_left();
                }
            }
            KeyCode::ArrowRight => {
                if selecting {
                    runtime.mutate_chat_editor(ChatEditor::select_right);
                } else {
                    runtime.move_chat_cursor_right();
                }
            }
            KeyCode::Home => runtime.move_chat_cursor_home(selecting),
            KeyCode::End => runtime.move_chat_cursor_end(selecting),
            KeyCode::ArrowUp => {
                if runtime.chat_suggestions().is_empty() {
                    runtime.show_older_chat_history();
                } else {
                    runtime.handle_chat_ui_action(UiAction::Navigate([0, -1]));
                }
            }
            KeyCode::ArrowDown => {
                if runtime.chat_suggestions().is_empty() {
                    runtime.show_newer_chat_history();
                } else {
                    runtime.handle_chat_ui_action(UiAction::Navigate([0, 1]));
                }
            }
            KeyCode::Tab => {
                runtime.handle_chat_ui_action(if selecting {
                    UiAction::TabPrevious
                } else {
                    UiAction::TabNext
                });
            }
            _ => {
                let modified = chat_modifiers::text_modified(&modifiers);
                if !modified
                    && let Some(text) = input.text.as_deref()
                    && !text.chars().any(char::is_control)
                {
                    let _ = runtime.insert_chat_text(text);
                }
            }
        }
    }

    if consumed_gameplay {
        if inventory_owned_pointer && !inventory_ownership_changed && runtime.inventory_open() {
            suppress_gameplay_input_for_inventory(
                &runtime,
                &mut cursor,
                &mut keys,
                &mut mouse_motion,
            );
        } else {
            suppress_gameplay_input_for_chat(
                &player_runtime,
                &runtime,
                &mut cursor,
                &mut keys,
                &mut mouse_buttons,
                &mut mouse_motion,
            );
        }
        // A send/cancel closes chat before suppression, but that same physical
        // key must still be consumed for the current frame.
        if !runtime.ui_focused(&player_runtime) {
            restore_gameplay_input_after_chat(
                &mut cursor,
                &mut keys,
                &mut mouse_buttons,
                &mut mouse_motion,
            );
        }
    }
}
