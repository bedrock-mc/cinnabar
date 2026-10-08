//! Spawn search suppresses wire input while preserving the global input clock.

use super::{HeldInput, MovementTicker, PhysicsAuthorityFault, PhysicsMovementSample};

impl MovementTicker {
    /// Revoke pre-search commands without installing the pending respawn position.
    pub fn begin_respawn_search(&mut self) {
        self.position_authority_changed();
        self.outbox.clear();
        self.previous_input = HeldInput::default();
        self.clear_pending_teleport_ack();
        self.refresh_outbox_reconciliation();
    }

    /// Advance the global input clock through a stationary tick without a wire packet.
    pub(super) fn withhold_respawn_input(
        &mut self,
        completed: PhysicsMovementSample,
    ) -> Result<(), PhysicsAuthorityFault> {
        if !self.accepting_physics_admissions() {
            return Err(PhysicsAuthorityFault::Unauthorized);
        }
        if completed.tick != self.next_tick {
            let fault = PhysicsAuthorityFault::TickMismatch {
                expected: self.next_tick,
                actual: completed.tick,
            };
            self.fail_physics_authority(&fault);
            return Err(fault);
        }
        self.snapshot(&completed);
        Ok(())
    }
}
