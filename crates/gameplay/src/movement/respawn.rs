//! Spawn search suppresses wire input while preserving the global input clock.

use super::{
    HeldInput, LocalPhysicsController, MovementTicker, PhysicsAuthorityFault, PhysicsMovementSample,
};

impl MovementTicker {
    /// Revoke pre-search commands without installing the pending respawn position.
    pub fn begin_respawn_search(&mut self, physics: &mut LocalPhysicsController) {
        self.discard_respawn_actions(physics);
        self.position_authority_changed();
        self.outbox.clear();
        self.clear_pending_teleport_ack();
        self.refresh_outbox_reconciliation();
    }

    /// Revokes old-life releases, temporary use state, launches and input transitions.
    pub(crate) fn discard_respawn_actions(&mut self, physics: &mut LocalPhysicsController) {
        self.held_release = None;
        self.admitted_riptide = None;
        self.written_riptide_tick = None;
        self.previous_input = HeldInput::default();
        self.next_tick_flags = protocol::PlayerInputFlags::NONE;
        self.next_tick_rotation = None;
        physics.restore_riptide_request(None);
        physics.take_server_using_item();
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
