use protocol::{ChatPacketError, Packet};
use semantic_input::KeyboardKey;
use ui::{ChatClipboard, UiAction};

use super::{UiRuntime, presentation};
#[cfg(any(test, feature = "test-support"))]
use inventory::inventory_ledger::CellGesture;
use inventory::inventory_ledger::{DropSource, InventoryGestureError};
use ui_screens::container_actions::InventoryCellHit;

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

/// Hands queued messages to the transport in FIFO order, each in the frame it was queued.
/// A transfer command waits for earlier sends to be acknowledged and holds back later ones.
pub fn flush_chat_sends<E>(
    runtime: &mut UiRuntime,
    budget: usize,
    mut send: impl FnMut(u64, u64, Option<FastTransferAction>, Packet) -> Result<(), E>,
) -> Result<usize, ChatFlushError<E>> {
    let mut sent = 0;
    while sent < budget {
        let in_flight = runtime.in_flight_chat_sends();
        let pending = runtime.pending_chat_sends();
        let Some(request) = pending.get(in_flight) else {
            break;
        };
        let action = FastTransferAction::classify(&request.message);
        let transfer_in_flight = pending
            .iter()
            .take(in_flight)
            .any(|request| FastTransferAction::classify(&request.message).is_some());
        if transfer_in_flight || (action.is_some() && in_flight > 0) {
            break;
        }
        let session = request.session;
        if session != runtime.session_id() {
            return Err(ChatFlushError::SessionChanged {
                expected: runtime.session_id(),
                actual: session,
            });
        }
        let (sequence, packet) = runtime
            .next_chat_packet()
            .map_err(ChatFlushError::Packet)?
            .expect("the next unsent request was observed above");
        send(session, sequence, action, packet).map_err(ChatFlushError::Transport)?;
        let enqueued = runtime.mark_chat_send_enqueued(session, sequence);
        debug_assert!(
            enqueued,
            "only the observed next request can become in flight"
        );
        sent += 1;
    }
    Ok(sent)
}

/// Inventory keyboard input captured before gameplay suppression resets the
/// frame's key state: this frame's presses and the held modifiers.
#[derive(Debug, Clone, Default)]
pub struct InventoryKeys {
    presses: Vec<KeyboardKey>,
    shift: bool,
    control: bool,
    modifier_sides: [bool; 4],
}

impl InventoryKeys {
    /// Drains this frame's presses while retaining the held modifier state.
    pub fn take_frame(&mut self) -> (Vec<KeyboardKey>, bool, bool) {
        (std::mem::take(&mut self.presses), self.shift, self.control)
    }

    /// Bounds one frame's buffered presses.
    const MAX_PRESSES: usize = 16;

    /// Tracks each physical modifier independently across suppressed gameplay frames.
    pub fn track_modifier(&mut self, key: KeyboardKey, pressed: bool) {
        let index = match key {
            KeyboardKey::ShiftLeft => 0,
            KeyboardKey::ShiftRight => 1,
            KeyboardKey::ControlLeft => 2,
            KeyboardKey::ControlRight => 3,
            _ => return,
        };
        self.modifier_sides[index] = pressed;
        self.shift = self.modifier_sides[0] || self.modifier_sides[1];
        self.control = self.modifier_sides[2] || self.modifier_sides[3];
    }

    pub fn press(&mut self, key: KeyboardKey) {
        if self.presses.len() < Self::MAX_PRESSES {
            self.presses.push(key);
        }
    }
}

/// Keyboard gestures over the hovered cell: resolved bindings swap with that hotbar
/// cell (or craft into it over the result); the configured drop key drops one item, Control drops
/// the whole stack; arrows scroll the creative grid.
pub fn dispatch_inventory_key(
    player_runtime: &mut player_state::PlayerState,
    runtime: &mut UiRuntime,
    hit: Option<InventoryCellHit>,
    key: KeyboardKey,
    control: bool,
    hotbar: Option<u8>,
    drop: bool,
) -> Option<Result<i32, InventoryGestureError>> {
    if let Some(slot) = hotbar {
        return dispatch_inventory_hotbar(player_runtime, runtime, hit, slot);
    }
    if runtime.screen_state().book.is_some() && runtime.book_key(player_runtime, key) {
        return None;
    }
    let scroll = match key {
        KeyboardKey::ArrowUp | KeyboardKey::PageUp => Some(-1),
        KeyboardKey::ArrowDown | KeyboardKey::PageDown => Some(1),
        _ => None,
    };
    if let Some(rows) = scroll
        && presentation::inventory_pointer::InventoryScreen::of_runtime(player_runtime, runtime)
            == presentation::inventory_pointer::InventoryScreen::Creative
    {
        let total = super::inventory_actions::visible_creative_entries(
            runtime.inventory_ledger(player_runtime),
            runtime,
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
            .scroll_loom(rows, ui_screens::container_actions::LOOM_PATTERNS.len());
        return None;
    }
    if drop && runtime.screen_state().text_focused() {
        return None;
    }
    if drop {
        if runtime
            .inventory_ledger(player_runtime)
            .cursor_stack()
            .is_some()
        {
            return Some(
                runtime
                    .inventory_ledger_mut(player_runtime)
                    .begin_drop(DropSource::Cursor, (!control).then_some(1)),
            );
        }
        if let Some(outcome) = runtime.drop_creative_hit(player_runtime, hit?, control) {
            return Some(outcome);
        }
    }
    let target = super::inventory_actions::gesture_target(hit?)?;
    let ledger = runtime.inventory_ledger_mut(player_runtime);
    drop.then(|| ledger.begin_drop(DropSource::Target(target), (!control).then_some(1)))
}

/// Applies a resolved hotbar binding directly to the predicted inventory cells.
pub fn dispatch_inventory_hotbar(
    player_runtime: &mut player_state::PlayerState,
    runtime: &mut UiRuntime,
    hit: Option<InventoryCellHit>,
    slot: u8,
) -> Option<Result<i32, InventoryGestureError>> {
    if slot >= protocol::HOTBAR_SLOT_COUNT || runtime.screen_state().text_focused() {
        return None;
    }
    if hit == Some(InventoryCellHit::CraftOutput) {
        return Some(runtime.craft_into_hotbar(player_runtime, slot));
    }
    let target = super::inventory_actions::gesture_target(hit?)?;
    Some(
        runtime
            .inventory_ledger_mut(player_runtime)
            .begin_hotbar_swap(target, slot),
    )
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

/// Whether the held keys make a key press a Control/Command editing shortcut.
pub fn is_chat_edit_shortcut(held: &[KeyboardKey]) -> bool {
    let pressed = |key: KeyboardKey| held.contains(&key);
    (pressed(KeyboardKey::ControlLeft)
        || pressed(KeyboardKey::ControlRight)
        || pressed(KeyboardKey::SuperLeft)
        || pressed(KeyboardKey::SuperRight))
        && !pressed(KeyboardKey::AltLeft)
        && !pressed(KeyboardKey::AltRight)
}

pub fn paste_chat_shortcut<C: ChatClipboard>(
    runtime: &mut UiRuntime,
    key: KeyboardKey,
    held: &[KeyboardKey],
    clipboard: &mut C,
) -> bool {
    if key != KeyboardKey::KeyV || !is_chat_edit_shortcut(held) {
        return false;
    }
    let _ = runtime.paste_chat_text(clipboard);
    true
}

mod pointer_order;
pub use pointer_order::{
    FrameInput, PointerRouter, PressRoute, ScreenKey, route_frame_presses, screen_after,
};

#[cfg(test)]
#[path = "interaction/modifier_tests.rs"]
mod modifier_tests;
