mod chat;
mod chat_coordinates;
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

use crate::acceptance::markers::FAST_TRANSFER_ACTION;
use crate::menu::settings_options::{binding_gamepad, binding_key, binding_mouse, binding_pressed};
use protocol::{ChatPacketError, Packet};
use ui::{ChatClipboard, ChatEditor, PointerPhase, UiAction, UiPoint};

#[cfg(test)]
use super::inventory_ledger::CellGesture;
use super::inventory_ledger::{DropSource, InventoryGestureError};
use super::{PlatformClipboard, UiRuntime, presentation};
use presentation::inventory_pointer::InventoryCellHit;

/// Admits every ready inventory packet in queue order, stopping at the first
/// transport refusal. Returns whether anything was admitted.
pub fn flush_inventory_send<E>(
    runtime: &mut UiRuntime,
    now_millis: u64,
    mut send: impl FnMut(Packet) -> Result<(), E>,
) -> Result<bool, E> {
    runtime.poll_inventory_timeout(now_millis);
    let mut admitted_any = false;
    for _ in 0..MAX_INVENTORY_PACKETS_PER_FLUSH {
        let Some((packet, entries)) = runtime
            .inventory_ledger()
            .pending_batch()
            .expect("the ledger retains only validated protocol requests")
        else {
            break;
        };
        if let Err(error) = send(packet) {
            runtime
                .inventory_ledger_mut()
                .note_transport_pressure(now_millis);
            return Err(error);
        }
        for _ in 0..entries {
            let admitted = runtime
                .inventory_ledger_mut()
                .mark_transport_enqueued(now_millis);
            debug_assert!(admitted, "only an awaiting request can be transported");
        }
        admitted_any = true;
    }
    Ok(admitted_any)
}

/// Bounds one frame's inventory transport work.
const MAX_INVENTORY_PACKETS_PER_FLUSH: usize = 32;

pub(crate) fn flush_inventory_network(
    time: Res<Time<Real>>,
    mut runtime: ResMut<UiRuntime>,
    network: Res<crate::runtime::network::NetworkHandle>,
) {
    let now_millis = u64::try_from(time.elapsed().as_millis()).unwrap_or(u64::MAX);
    match flush_inventory_send(&mut runtime, now_millis, |packet| {
        network.send_inventory_packet(packet)
    }) {
        Ok(_) | Err(crate::runtime::network::PacketSendError::Full(_)) => {}
        Err(crate::runtime::network::PacketSendError::Closed(_)) => {
            runtime.inventory_transport_closed();
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

#[derive(Debug, PartialEq, Eq)]
pub enum ChatFlushError<E> {
    Packet(ChatPacketError),
    Transport(E),
    SessionChanged { expected: u64, actual: u64 },
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum FastTransferAction {
    TransferSm3,
}

impl FastTransferAction {
    fn classify(message: &str) -> Option<Self> {
        (message == "/transfer sm3").then_some(Self::TransferSm3)
    }

    pub(crate) fn marker(
        self,
        session_generation: u64,
        action_ordinal: u64,
        sent_unix_ms: u64,
    ) -> String {
        let command = match self {
            Self::TransferSm3 => "/transfer sm3",
        };
        format!(
            "{FAST_TRANSFER_ACTION}={}",
            serde_json::json!({
                "schema": "rust-mcbe-fast-transfer-action-v1",
                "kind": "command_sent",
                "session_generation": session_generation,
                "action_ordinal": action_ordinal,
                "command": command,
                "sent_unix_ms": sent_unix_ms,
            })
        )
    }
}

pub fn flush_chat_sends<E>(
    runtime: &mut UiRuntime,
    budget: usize,
    mut send: impl FnMut(u64, u64, Option<FastTransferAction>, Packet) -> Result<(), E>,
) -> Result<usize, ChatFlushError<E>> {
    if budget == 0 || runtime.in_flight_chat_send().is_some() {
        return Ok(0);
    }
    let mut sent = 0;
    for _ in 0..budget.min(1) {
        let Some(request) = runtime.pending_chat_sends().front() else {
            break;
        };
        if request.session != runtime.session_id() {
            return Err(ChatFlushError::SessionChanged {
                expected: runtime.session_id(),
                actual: request.session,
            });
        }
        let (sequence, packet) = runtime
            .front_chat_packet()
            .map_err(ChatFlushError::Packet)?
            .expect("the pending front was observed above");
        send(
            request.session,
            sequence,
            FastTransferAction::classify(&request.message),
            packet,
        )
        .map_err(ChatFlushError::Transport)?;
        let enqueued = runtime.mark_chat_send_enqueued(request.session, sequence);
        debug_assert!(
            enqueued,
            "only the observed FIFO front can become in flight"
        );
        sent += 1;
    }
    Ok(sent)
}

pub(crate) fn flush_chat_network(
    mut runtime: ResMut<UiRuntime>,
    network: Res<crate::runtime::network::NetworkHandle>,
    mut client_world: ResMut<crate::runtime::world::ClientWorld>,
) {
    runtime.service_pending_chat_autocomplete();
    if network.closed_command_has_pending_control() {
        return;
    }
    if runtime.take_wake_request()
        && let Some(runtime_id) = runtime.local_runtime_id()
    {
        let _ = network.send_inventory_packet(protocol::stop_sleeping_packet(runtime_id));
    }
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

/// Inventory keyboard input captured before gameplay suppression resets the
/// frame's key state: this frame's presses and the held modifiers.
#[derive(Debug, Clone, Default)]
pub(crate) struct InventoryKeys {
    presses: Vec<KeyCode>,
    shift: bool,
    control: bool,
}

impl InventoryKeys {
    /// Bounds one frame's buffered presses.
    const MAX_PRESSES: usize = 16;

    fn track_modifier(&mut self, input: &KeyboardInput) {
        let pressed = input.state == ButtonState::Pressed;
        match input.key_code {
            KeyCode::ShiftLeft | KeyCode::ShiftRight => self.shift = pressed,
            KeyCode::ControlLeft | KeyCode::ControlRight => self.control = pressed,
            _ => {}
        }
    }

    fn press(&mut self, key: KeyCode) {
        if self.presses.len() < Self::MAX_PRESSES {
            self.presses.push(key);
        }
    }
}

#[allow(clippy::too_many_arguments)]
pub(crate) fn drive_inventory_ui_actions(
    time: Option<Res<Time<Real>>>,
    window: Single<&Window, With<PrimaryWindow>>,
    menu: Option<Res<crate::menu::MenuRuntime>>,
    mut mouse_buttons: ResMut<ButtonInput<MouseButton>>,
    mouse_messages: Option<Res<Messages<MouseButtonInput>>>,
    mut release_cursor: Local<MessageCursor<MouseButtonInput>>,
    wheel_messages: Option<Res<Messages<MouseWheel>>>,
    mut wheel_cursor: Local<MessageCursor<MouseWheel>>,
    presentation: Res<presentation::UiPresentationRuntime>,
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
    let presses = std::mem::take(&mut runtime.inventory_keys.presses);
    let (shift, control) = (runtime.inventory_keys.shift, runtime.inventory_keys.control);
    if runtime.server_forms().owns_input() {
        return;
    }
    if menu.as_ref().is_some_and(|menu| menu.is_visible())
        || !runtime.inventory_open()
        || !window.focused
    {
        runtime.set_inventory_pointer_gui(None);
        runtime.screen_state_mut().hover = None;
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
    // The inventory owns pointer buttons while open. Preserve the edges long
    // enough to resolve their cell, then clear every button before gameplay
    // systems can observe this frame.
    mouse_buttons.reset_all();
    let generation = runtime.inventory_ledger().storage_generation();
    runtime.screen_state_mut().observe_window(generation);
    let screen = presentation::inventory_pointer::InventoryScreen::of_runtime(&runtime);
    if screen != presentation::inventory_pointer::InventoryScreen::Creative {
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
        let _ = dispatch_inventory_key(runtime.as_mut(), hit, key, control);
    }
    let frame = super::inventory_drag::PointerFrame {
        primary_pressed,
        primary_released,
        secondary_pressed,
        secondary_released,
        shift,
        holding: runtime.inventory_ledger().cursor_stack().is_some(),
        hit,
        now_millis,
    };
    let actions = runtime.screen_state_mut().pointer.step(frame);
    for action in actions {
        runtime.perform_pointer_action(action);
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
                .inventory_ledger_mut()
                .begin_drop(DropSource::Cursor, amount);
        }
    }
}

/// Wheel notches over an engine-drawn screen scroll the view under the pointer.
fn scroll_container(
    runtime: &mut UiRuntime,
    frame: &super::forms::EngineFrame,
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
pub(crate) fn drive_world_inventory_keys(
    gamepads: Query<&Gamepad>,
    window: Single<&Window, With<PrimaryWindow>>,
    keys: Res<ButtonInput<KeyCode>>,
    mouse: Res<ButtonInput<MouseButton>>,
    menu: Option<Res<crate::menu::MenuRuntime>>,
    presentation: Option<Res<presentation::UiPresentationRuntime>>,
    mut runtime: ResMut<UiRuntime>,
) {
    let drop = binding_pressed(menu.as_deref(), "key.drop", &keys, &mouse)
        || binding_gamepad(menu.as_deref(), "key.drop", &gamepads);
    let use_book = binding_pressed(menu.as_deref(), "key.use", &keys, &mouse);
    if !window.focused
        || crate::screen_policy::absorbs_input(
            Some(&runtime),
            menu.as_deref(),
            presentation.as_deref(),
        )
        || !(drop || use_book)
        || runtime
            .player_game_mode()
            .is_some_and(|mode| !mode.shows_hotbar())
    {
        return;
    }
    if use_book {
        runtime.open_held_book();
        return;
    }
    let Some(slot) = runtime.selected_hotbar_slot() else {
        return;
    };
    let control = keys.pressed(KeyCode::ControlLeft) || keys.pressed(KeyCode::ControlRight);
    let _ = runtime
        .inventory_ledger_mut()
        .begin_world_drop(slot, (!control).then_some(1));
}

/// Keyboard gestures over the hovered cell: digits swap with that hotbar
/// cell (or craft into it over the result), Q drops one item and Control+Q
/// the whole stack; arrows scroll the creative grid.
pub(crate) fn dispatch_inventory_key(
    runtime: &mut UiRuntime,
    hit: Option<InventoryCellHit>,
    key: KeyCode,
    control: bool,
) -> Option<Result<i32, InventoryGestureError>> {
    let hotbar = [
        KeyCode::Digit1,
        KeyCode::Digit2,
        KeyCode::Digit3,
        KeyCode::Digit4,
        KeyCode::Digit5,
        KeyCode::Digit6,
        KeyCode::Digit7,
        KeyCode::Digit8,
        KeyCode::Digit9,
    ]
    .iter()
    .position(|digit| *digit == key);
    if let (Some(InventoryCellHit::CraftOutput), Some(slot)) = (hit, hotbar) {
        return Some(runtime.craft_into_hotbar(slot as u8));
    }
    if runtime.screen_state().book.is_some() && runtime.book_key(key) {
        return None;
    }
    let scroll = match key {
        KeyCode::ArrowUp | KeyCode::PageUp => Some(-1),
        KeyCode::ArrowDown | KeyCode::PageDown => Some(1),
        _ => None,
    };
    if let Some(rows) = scroll
        && presentation::inventory_pointer::InventoryScreen::of_runtime(runtime)
            == presentation::inventory_pointer::InventoryScreen::Creative
    {
        let total = super::inventory_actions::visible_creative_entries(
            runtime.inventory_ledger(),
            runtime.screen_state(),
        )
        .len();
        runtime.screen_state_mut().scroll_creative(rows, total);
        return None;
    }
    if let Some(rows) = scroll
        && runtime.inventory_ledger().window_kind() == Some(protocol::WindowKind::Loom)
    {
        runtime
            .screen_state_mut()
            .scroll_loom(rows, super::screen_recipes::LOOM_PATTERNS.len());
        return None;
    }
    let target = super::inventory_actions::gesture_target(hit?)?;
    let ledger = runtime.inventory_ledger_mut();
    match (hotbar, key) {
        (Some(slot), _) => Some(ledger.begin_hotbar_swap(target, slot as u8)),
        (None, KeyCode::KeyQ) => {
            let amount = (!control).then_some(1);
            Some(ledger.begin_drop(DropSource::Target(target), amount))
        }
        _ => None,
    }
}

/// Routes one resolved pointer gesture; the output cell crafts once.
#[cfg(test)]
pub(crate) fn dispatch_inventory_click(
    runtime: &mut UiRuntime,
    hit: InventoryCellHit,
    gesture: CellGesture,
) -> Result<i32, InventoryGestureError> {
    match super::inventory_actions::gesture_target(hit) {
        Some(target) => runtime
            .inventory_ledger_mut()
            .begin_target_gesture(target, gesture),
        None if gesture == CellGesture::Click => runtime.begin_crafting(),
        None => Err(InventoryGestureError::InvalidRequest),
    }
}

pub(crate) const fn gamepad_chat_action(button: GamepadButton) -> Option<UiAction> {
    match button {
        GamepadButton::DPadUp => Some(UiAction::Navigate([0, -1])),
        GamepadButton::DPadDown => Some(UiAction::Navigate([0, 1])),
        GamepadButton::South => Some(UiAction::Accept),
        GamepadButton::East => Some(UiAction::Cancel),
        GamepadButton::RightTrigger => Some(UiAction::TabNext),
        GamepadButton::LeftTrigger => Some(UiAction::TabPrevious),
        _ => None,
    }
}

pub(crate) fn dispatch_chat_ui_action(
    runtime: &mut UiRuntime,
    action: UiAction,
    suggestion_hit: Option<usize>,
    now_millis: u64,
) -> bool {
    match action {
        UiAction::Cancel => {
            runtime.close_chat();
            true
        }
        UiAction::Accept if runtime.chat_suggestions().is_empty() => {
            if runtime.queue_chat_send(now_millis).is_err() {
                return false;
            }
            runtime.close_chat();
            true
        }
        _ => runtime.handle_chat_ui_action_with_suggestion_hit(action, suggestion_hit),
    }
}

fn is_chat_paste_shortcut(key: KeyCode, keys: &ButtonInput<KeyCode>) -> bool {
    key == KeyCode::KeyV
        && (keys.pressed(KeyCode::ControlLeft)
            || keys.pressed(KeyCode::ControlRight)
            || keys.pressed(KeyCode::SuperLeft)
            || keys.pressed(KeyCode::SuperRight))
        && !keys.pressed(KeyCode::AltLeft)
        && !keys.pressed(KeyCode::AltRight)
}

pub(crate) fn paste_chat_shortcut<C: ChatClipboard>(
    runtime: &mut UiRuntime,
    key: KeyCode,
    keys: &ButtonInput<KeyCode>,
    clipboard: &mut C,
) -> bool {
    if !is_chat_paste_shortcut(key, keys) {
        return false;
    }
    let _ = runtime.paste_chat_text(clipboard);
    true
}

#[allow(clippy::too_many_arguments)]
pub(crate) fn drive_chat_keyboard_input(
    gamepads: Query<&Gamepad>,
    mut keyboard_messages: MessageReader<KeyboardInput>,
    time: Res<Time<Real>>,
    window: Single<(&Window, &mut CursorOptions), With<PrimaryWindow>>,
    menu: Option<Res<crate::menu::MenuRuntime>>,
    mut keys: ResMut<ButtonInput<KeyCode>>,
    mut mouse_buttons: ResMut<ButtonInput<MouseButton>>,
    mut mouse_motion: ResMut<AccumulatedMouseMotion>,
    mut runtime: ResMut<UiRuntime>,
    mut presentation: Option<ResMut<presentation::UiPresentationRuntime>>,
) {
    let (window, mut cursor) = window.into_inner();
    if runtime.server_forms().owns_input() {
        keyboard_messages.clear();
        return;
    }
    if menu.as_ref().is_some_and(|menu| menu.is_visible()) {
        if runtime.inventory_open() {
            runtime.close_inventory();
        }
        keyboard_messages.clear();
        // The menu system runs next and must see the original button state.
        // It consumes keyboard/pointer input after handling its own actions.
        mouse_motion.delta = Vec2::ZERO;
        cursor.grab_mode = CursorGrabMode::None;
        cursor.visible = true;
        return;
    }
    if !window.focused {
        if runtime.chat_focused() {
            runtime.close_chat();
        }
        if runtime.inventory_open() {
            runtime.close_inventory();
        }
        return;
    }

    // An already-open inventory owns this frame's pointer edge. Keyboard
    // transitions below may close it or open a new UI, so both sides of the
    // transition are checked before preserving that edge for the inventory
    // system later in the production chain.
    let inventory_owned_pointer = runtime.inventory_open();
    let mut inventory_ownership_changed = false;
    let mut consumed_gameplay = runtime.ui_focused();
    if !runtime.chat_focused() && !runtime.screen_state().text_focused() {
        if binding_mouse(menu.as_deref(), "key.inventory", &mouse_buttons)
            || binding_gamepad(menu.as_deref(), "key.inventory", &gamepads)
        {
            runtime.toggle_inventory();
            inventory_ownership_changed = true;
            consumed_gameplay = true;
        } else if !runtime.inventory_open()
            && (binding_mouse(menu.as_deref(), "key.chat", &mouse_buttons)
                || binding_gamepad(menu.as_deref(), "key.chat", &gamepads))
        {
            runtime.open_chat();
            consumed_gameplay = true;
        } else if !runtime.inventory_open()
            && binding_mouse(menu.as_deref(), "key.command", &mouse_buttons)
        {
            runtime.open_chat();
            let _ = runtime.insert_chat_text("/");
            consumed_gameplay = true;
        }
    }
    for input in keyboard_messages.read() {
        runtime.inventory_keys.track_modifier(input);
        if input.state != ButtonState::Pressed {
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
                        runtime.commit_book(false);
                        runtime.close_inventory();
                        inventory_ownership_changed = true;
                    }
                    KeyCode::Backspace => runtime.screen_state_mut().backspace_text(),
                    key if runtime.book_key(key) => {}
                    _ => {
                        let modified = keys.pressed(KeyCode::ControlLeft)
                            || keys.pressed(KeyCode::ControlRight)
                            || keys.pressed(KeyCode::AltLeft)
                            || keys.pressed(KeyCode::AltRight)
                            || keys.pressed(KeyCode::SuperLeft)
                            || keys.pressed(KeyCode::SuperRight);
                        if !modified && let Some(text) = input.text.as_deref() {
                            runtime.screen_state_mut().type_text(text);
                        }
                    }
                }
                continue;
            }
            match input.key_code {
                key if binding_key(menu.as_deref(), "key.inventory", key) => {
                    runtime.toggle_inventory();
                    inventory_ownership_changed = true;
                }
                KeyCode::Escape => {
                    runtime.close_inventory();
                    inventory_ownership_changed = true;
                }
                key if binding_key(menu.as_deref(), "key.drop", key) => {
                    runtime.inventory_keys.press(KeyCode::KeyQ)
                }
                KeyCode::KeyQ => {}
                key => runtime.inventory_keys.press(key),
            }
            continue;
        }
        if runtime.local_sleeping() && !runtime.chat_focused() {
            // The bed screen: Escape leaves the bed, T opens chat over it.
            match input.key_code {
                KeyCode::Escape => runtime.request_wake(),
                key if binding_key(menu.as_deref(), "key.chat", key) => {
                    runtime.open_chat();
                }
                key if binding_key(menu.as_deref(), "key.command", key) => {
                    runtime.open_chat();
                    let _ = runtime.insert_chat_text("/");
                }
                _ => {}
            }
            continue;
        }
        if !runtime.chat_focused() {
            match input.key_code {
                key if binding_key(menu.as_deref(), "key.inventory", key) => {
                    runtime.toggle_inventory();
                    inventory_ownership_changed = true;
                    consumed_gameplay = true;
                }
                key if binding_key(menu.as_deref(), "key.chat", key) => {
                    runtime.open_chat();
                    consumed_gameplay = true;
                }
                key if binding_key(menu.as_deref(), "key.command", key) => {
                    runtime.open_chat();
                    let _ = runtime.insert_chat_text("/");
                    consumed_gameplay = true;
                }
                _ => {}
            }
            continue;
        }

        consumed_gameplay = true;
        let selecting = keys.pressed(KeyCode::ShiftLeft) || keys.pressed(KeyCode::ShiftRight);
        if paste_chat_shortcut(&mut runtime, input.key_code, &keys, &mut PlatformClipboard) {
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
                let modified = keys.pressed(KeyCode::ControlLeft)
                    || keys.pressed(KeyCode::ControlRight)
                    || keys.pressed(KeyCode::AltLeft)
                    || keys.pressed(KeyCode::AltRight)
                    || keys.pressed(KeyCode::SuperLeft)
                    || keys.pressed(KeyCode::SuperRight);
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
                &runtime,
                &mut cursor,
                &mut keys,
                &mut mouse_buttons,
                &mut mouse_motion,
            );
        }
        // A send/cancel closes chat before suppression, but that same physical
        // key must still be consumed for the current frame.
        if !runtime.ui_focused() {
            restore_gameplay_input_after_chat(
                &mut cursor,
                &mut keys,
                &mut mouse_buttons,
                &mut mouse_motion,
            );
        }
    }
}

fn suppress_gameplay_input_for_inventory(
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
    mouse_motion.delta = bevy::math::Vec2::ZERO;
}

pub(crate) fn suppress_gameplay_input_for_chat(
    runtime: &UiRuntime,
    cursor: &mut CursorOptions,
    keys: &mut ButtonInput<KeyCode>,
    mouse_buttons: &mut ButtonInput<MouseButton>,
    mouse_motion: &mut AccumulatedMouseMotion,
) {
    if !runtime.ui_focused() {
        return;
    }
    cursor.grab_mode = CursorGrabMode::None;
    cursor.visible = true;
    keys.reset_all();
    mouse_buttons.reset_all();
    mouse_motion.delta = bevy::math::Vec2::ZERO;
}
