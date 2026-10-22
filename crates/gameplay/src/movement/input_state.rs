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
    /// Server sprint state applied before this tick admits local sprint input.
    pub server_sprinting: Option<bool>,
    /// Server sneak state applied before this tick admits local sneak input.
    pub server_sneaking: Option<bool>,
    /// Locomotion mode a server pose flag left before this tick, which vanilla
    /// changes without a start or stop action.
    pub server_mode: Option<sim::MovementMode>,
    /// Riptide spin a server flag started or cancelled before this tick.
    pub server_spinning: Option<bool>,
}

/// Button edges and server stance updates wait for a completed tick.
#[derive(Debug, Clone, Copy, Default)]
pub(super) struct PendingInputEdges {
    jump: ActionPhase,
    sneak: ActionPhase,
    server_sprinting: Option<bool>,
    server_sneaking: Option<bool>,
    server_mode: Option<sim::MovementMode>,
    server_spinning: Option<bool>,
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

    /// Accumulates button edges and keeps the latest server stance, pose and spin.
    pub(super) fn observe(&mut self, input: TickInput) {
        self.jump.pressed |= input.jump.pressed;
        self.jump.released |= input.jump.released;
        self.sneak.pressed |= input.sneak.pressed;
        self.sneak.released |= input.sneak.released;
        self.server_sprinting = input.server_sprinting.or(self.server_sprinting);
        self.server_sneaking = input.server_sneaking.or(self.server_sneaking);
        self.server_mode = input.server_mode.or(self.server_mode);
        self.server_spinning = input.server_spinning.or(self.server_spinning);
    }

    /// Combines retained updates with the latest held requests for a simulation attempt.
    pub(super) fn sample(self, mut input: TickInput) -> TickInput {
        input.jump.pressed = self.jump.pressed;
        input.jump.released = self.jump.released;
        input.sneak.pressed = self.sneak.pressed;
        input.sneak.released = self.sneak.released;
        input.server_sprinting = self.server_sprinting;
        input.server_sneaking = self.server_sneaking;
        input.server_mode = self.server_mode;
        input.server_spinning = self.server_spinning;
        input
    }
}
