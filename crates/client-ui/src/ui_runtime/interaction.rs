use bevy::{
    input::{
        ButtonState, gamepad::GamepadButton, keyboard::KeyboardInput, mouse::AccumulatedMouseMotion,
    },
    prelude::{ButtonInput, KeyCode, MouseButton, Vec2},
    window::{CursorGrabMode, CursorOptions},
};

use protocol::{ChatPacketError, Packet};
use ui::{ChatClipboard, UiAction};

#[cfg(any(test, feature = "test-support"))]
use super::inventory_ledger::CellGesture;
use super::inventory_ledger::{DropSource, InventoryGestureError};
use super::{UiRuntime, presentation};
use presentation::inventory_pointer::InventoryCellHit;

/// Admits every ready inventory packet in queue order, stopping at the first
/// transport refusal. Returns whether anything was admitted.
pub fn flush_inventory_send<E>(
    player_runtime: &mut player_state::PlayerState,
    runtime: &mut UiRuntime,
    now_millis: u64,
    send: impl FnMut(Packet) -> Result<(), E>,
) -> Result<bool, E> {
    runtime.poll_inventory_timeout(player_runtime, now_millis);
    player_runtime
        .inventory
        .flush_pending_inventory_send(now_millis, send)
}

#[derive(Debug, PartialEq, Eq)]
pub enum ChatFlushError<E> {
    Packet(ChatPacketError),
    Transport(E),
    SessionChanged { expected: u64, actual: u64 },
}

pub use protocol::FastTransferAction;

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

/// Inventory keyboard input captured before gameplay suppression resets the
/// frame's key state: this frame's presses and the held modifiers.
#[derive(Debug, Clone, Default)]
pub struct InventoryKeys {
    presses: Vec<KeyCode>,
    shift: bool,
    control: bool,
    modifier_sides: [bool; 4],
}

impl InventoryKeys {
    /// Drains this frame's presses while retaining the held modifier state.
    pub fn take_frame(&mut self) -> (Vec<KeyCode>, bool, bool) {
        (std::mem::take(&mut self.presses), self.shift, self.control)
    }

    /// Bounds one frame's buffered presses.
    const MAX_PRESSES: usize = 16;

    /// Tracks each physical modifier independently across suppressed gameplay frames.
    pub fn track_modifier(&mut self, input: &KeyboardInput) {
        let index = match input.key_code {
            KeyCode::ShiftLeft => 0,
            KeyCode::ShiftRight => 1,
            KeyCode::ControlLeft => 2,
            KeyCode::ControlRight => 3,
            _ => return,
        };
        self.modifier_sides[index] = input.state == ButtonState::Pressed;
        self.shift = self.modifier_sides[0] || self.modifier_sides[1];
        self.control = self.modifier_sides[2] || self.modifier_sides[3];
    }

    pub fn press(&mut self, key: KeyCode) {
        if self.presses.len() < Self::MAX_PRESSES {
            self.presses.push(key);
        }
    }
}

/// Keyboard gestures over the hovered cell: digits swap with that hotbar
/// cell (or craft into it over the result), Q drops one item and Control+Q
/// the whole stack; arrows scroll the creative grid.
pub fn dispatch_inventory_key(
    player_runtime: &mut player_state::PlayerState,
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
        return Some(runtime.craft_into_hotbar(player_runtime, slot as u8));
    }
    if runtime.screen_state().book.is_some() && runtime.book_key(player_runtime, key) {
        return None;
    }
    let scroll = match key {
        KeyCode::ArrowUp | KeyCode::PageUp => Some(-1),
        KeyCode::ArrowDown | KeyCode::PageDown => Some(1),
        _ => None,
    };
    if let Some(rows) = scroll
        && presentation::inventory_pointer::InventoryScreen::of_runtime(player_runtime, runtime)
            == presentation::inventory_pointer::InventoryScreen::Creative
    {
        let total = super::inventory_actions::visible_creative_entries(
            runtime.inventory_ledger(player_runtime),
            runtime.screen_state(),
        )
        .len();
        runtime.screen_state_mut().scroll_creative(rows, total);
        return None;
    }
    if let Some(rows) = scroll
        && runtime.inventory_ledger(player_runtime).window_kind()
            == Some(protocol::WindowKind::Loom)
    {
        runtime
            .screen_state_mut()
            .scroll_loom(rows, super::screen_recipes::LOOM_PATTERNS.len());
        return None;
    }
    let target = super::inventory_actions::gesture_target(hit?)?;
    let ledger = runtime.inventory_ledger_mut(player_runtime);
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
#[cfg(any(test, feature = "test-support"))]
pub fn dispatch_inventory_click(
    player_runtime: &mut player_state::PlayerState,
    runtime: &mut UiRuntime,
    hit: InventoryCellHit,
    gesture: CellGesture,
) -> Result<i32, InventoryGestureError> {
    match super::inventory_actions::gesture_target(hit) {
        Some(target) => runtime
            .inventory_ledger_mut(player_runtime)
            .begin_target_gesture(target, gesture),
        None if gesture == CellGesture::Click => runtime.begin_crafting(player_runtime),
        None => Err(InventoryGestureError::InvalidRequest),
    }
}

pub const fn gamepad_chat_action(button: GamepadButton) -> Option<UiAction> {
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

pub fn dispatch_chat_ui_action(
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

pub fn is_chat_edit_shortcut(keys: &ButtonInput<KeyCode>) -> bool {
    (keys.pressed(KeyCode::ControlLeft)
        || keys.pressed(KeyCode::ControlRight)
        || keys.pressed(KeyCode::SuperLeft)
        || keys.pressed(KeyCode::SuperRight))
        && !keys.pressed(KeyCode::AltLeft)
        && !keys.pressed(KeyCode::AltRight)
}

pub fn paste_chat_shortcut<C: ChatClipboard>(
    runtime: &mut UiRuntime,
    key: KeyCode,
    keys: &ButtonInput<KeyCode>,
    clipboard: &mut C,
) -> bool {
    if key != KeyCode::KeyV || !is_chat_edit_shortcut(keys) {
        return false;
    }
    let _ = runtime.paste_chat_text(clipboard);
    true
}

pub fn suppress_gameplay_input_for_inventory(
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

pub fn restore_gameplay_input_after_chat(
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

pub fn suppress_gameplay_input_for_chat(
    player_runtime: &player_state::PlayerState,
    runtime: &UiRuntime,
    cursor: &mut CursorOptions,
    keys: &mut ButtonInput<KeyCode>,
    mouse_buttons: &mut ButtonInput<MouseButton>,
    mouse_motion: &mut AccumulatedMouseMotion,
) {
    if !runtime.ui_focused(player_runtime) {
        return;
    }
    cursor.grab_mode = CursorGrabMode::None;
    cursor.visible = true;
    keys.reset_all();
    mouse_buttons.reset_all();
    mouse_motion.delta = bevy::math::Vec2::ZERO;
}

#[cfg(test)]
#[path = "interaction/modifier_tests.rs"]
mod modifier_tests;
