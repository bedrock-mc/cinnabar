//! Input request lanes and button edges retained independently of actor movement outcomes.

use semantic_input::{ActionPhase, MovementButtons};

/// Requests and server control updates retained through fixed ticks, replay and send retries.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct TickInput {
    pub movement_buttons: MovementButtons,
    pub jump: ActionPhase,
    pub sneak: ActionPhase,
    /// Sprint control after the hold/toggle setting, before sprint admission.
    pub sprint_down: bool,
    /// Sneak control after the hold/toggle setting, before forced-pose admission.
    pub sneak_down: bool,
    /// The sneak control is held while it acts as the toggle-sneak control:
    /// always on a gamepad, otherwise under the toggle-sneak setting.
    pub sneak_toggle_down: bool,
    /// Server sprint state applied before this tick admits local sprint input.
    pub server_sprinting: Option<bool>,
    /// Server sneak state applied before this tick admits local sneak input.
    pub server_sneaking: Option<bool>,
}

/// Button edges and server stance updates wait for a completed tick.
#[derive(Debug, Clone, Copy, Default)]
pub(super) struct PendingInputEdges {
    jump: ActionPhase,
    sneak: ActionPhase,
    server_sprinting: Option<bool>,
    server_sneaking: Option<bool>,
}

impl PendingInputEdges {
    /// Discards blocked presses without suppressing retained release edges.
    pub(super) fn apply_input_locks(&mut self, locks: protocol::ClientInputLocks) {
        if locks.blocks(protocol::ClientInputLock::Jump) {
            self.jump.pressed = false;
        }
        if locks.blocks(protocol::ClientInputLock::Sneak) {
            self.sneak.pressed = false;
        }
    }

    /// Accumulates button edges and keeps the latest server sprint state.
    pub(super) fn observe(&mut self, input: TickInput) {
        self.jump.pressed |= input.jump.pressed;
        self.jump.released |= input.jump.released;
        self.sneak.pressed |= input.sneak.pressed;
        self.sneak.released |= input.sneak.released;
        self.server_sprinting = input.server_sprinting.or(self.server_sprinting);
        self.server_sneaking = input.server_sneaking.or(self.server_sneaking);
    }

    /// Combines retained updates with the latest held requests for a simulation attempt.
    pub(super) fn sample(self, mut input: TickInput) -> TickInput {
        input.jump.pressed = self.jump.pressed;
        input.jump.released = self.jump.released;
        input.sneak.pressed = self.sneak.pressed;
        input.sneak.released = self.sneak.released;
        input.server_sprinting = self.server_sprinting;
        input.server_sneaking = self.server_sneaking;
        input
    }
}
