//! Permission enforcement for frame input, retained edges and extension jump pulses.

use super::{LocalPhysicsController, PhysicsSampleContext};
use protocol::ClientInputLock;
use sim::MovementInput;

impl LocalPhysicsController {
    /// Drops unconsumed mod input when gameplay ownership or its world changes.
    pub fn set_jump_pulse_scope(&mut self, scope: Option<(u64, i32)>) {
        if scope.is_none() || scope != self.jump_pulse_scope {
            self.jump_pulse_pending = false;
        }
        self.jump_pulse_scope = scope;
    }

    /// Queues one ordinary jump input for the next successful fixed tick.
    pub fn request_jump_pulse(&mut self) {
        if self.jump_pulse_scope.is_some() && self.is_active() {
            self.jump_pulse_pending = true;
        }
    }

    /// Whether the retained simulation is ordinary walking outside liquids.
    pub fn jump_pulse_eligible(&self) -> bool {
        self.is_active()
            && self.modes.mode().is_walking()
            && !self.last_environment.in_water
            && !self.last_environment.in_lava
            && !self.dimension_waiting
    }

    /// Revokes blocked requests before observing edges, including frames without a tick.
    pub(super) fn filter_input_requests(
        &mut self,
        input: &mut MovementInput,
        context: &mut PhysicsSampleContext,
    ) {
        let locks = context.input_locks;
        self.input_edges.apply_input_locks(locks);
        if locks.blocks(ClientInputLock::Jump) {
            input.jumping = false;
            input.jump_pressed = false;
            context.input.jump = Default::default();
            self.jump_edge_pending = false;
            self.previous_jump_held = false;
            self.jump_pulse_pending = false;
            self.jump_pulse_held = false;
        }
        if locks.blocks(ClientInputLock::Sneak) {
            input.sneaking = false;
            context.input.sneak = Default::default();
            context.input.sneak_down = false;
        }
    }
}
