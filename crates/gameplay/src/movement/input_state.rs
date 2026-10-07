//! Input request lanes and button edges retained independently of actor movement outcomes.

use semantic_input::{ActionPhase, MovementButtons};

/// The device/request state consumed by one fixed tick and retained through replay and send retries.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct TickInput {
    pub movement_buttons: MovementButtons,
    pub jump: ActionPhase,
    pub sneak: ActionPhase,
    /// Sprint control after the hold/toggle setting, before sprint admission.
    pub sprint_down: bool,
    /// Sneak control after the hold/toggle setting, before forced-pose admission.
    pub sneak_down: bool,
}

/// Raw press/release events wait for a completed tick, including events in tickless frames.
#[derive(Debug, Clone, Copy, Default)]
pub(super) struct PendingInputEdges {
    jump: ActionPhase,
    sneak: ActionPhase,
}

impl PendingInputEdges {
    /// Accumulates both edges of a tap without changing its current held value.
    pub(super) fn observe(&mut self, input: TickInput) {
        self.jump.pressed |= input.jump.pressed;
        self.jump.released |= input.jump.released;
        self.sneak.pressed |= input.sneak.pressed;
        self.sneak.released |= input.sneak.released;
    }

    /// Combines retained edges with the latest held/request state for a simulation attempt.
    pub(super) fn sample(self, mut input: TickInput) -> TickInput {
        input.jump.pressed = self.jump.pressed;
        input.jump.released = self.jump.released;
        input.sneak.pressed = self.sneak.pressed;
        input.sneak.released = self.sneak.released;
        input
    }
}
