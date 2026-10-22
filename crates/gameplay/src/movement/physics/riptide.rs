//! Released trident requests and cancellation before transport admission.

use super::*;

impl LocalPhysicsController {
    /// Whether a retained tick after `tick` launched a Riptide spin.
    pub(in crate::movement) fn launched_riptide_after(&self, tick: u64) -> bool {
        let Some(current) = self.state.as_ref().map(|state| state.tick) else {
            return false;
        };
        (tick.saturating_add(1)..=current).any(|tick| {
            self.history
                .input_at(tick)
                .is_some_and(|input| input.riptide.is_some())
        })
    }

    /// Ends the spin of a predicted launch whose unsent release a snap
    /// cancelled; neither the launch nor its start action reached the server.
    pub(in crate::movement) fn end_cancelled_launch_spin(&mut self) {
        if let Some(state) = self.state.as_mut() {
            state.spin_attack_ticks = None;
        }
    }

    /// Retains release-time facing until a successful tick; unsupported requests are retired.
    pub fn request_riptide(&mut self, level: u8, rotation: [f32; 2]) -> bool {
        let tick = self.state.as_ref().map_or(0, |state| state.tick);
        self.request_riptide_after_tick(level, rotation, tick)
            .is_some()
    }

    /// Queues a launch after its facing input, without consuming it on that input's tick.
    pub(in crate::movement) fn request_riptide_after_tick(
        &mut self,
        level: u8,
        rotation: [f32; 2],
        after_tick: u64,
    ) -> Option<RiptideRequest> {
        let Some(launch) = sim::RiptideLaunch::new(level, rotation) else {
            super::super::diagnostics::note_skipped_authority("riptide", f64::from(level));
            return None;
        };
        if !self.is_active() {
            return None;
        }
        let request = RiptideRequest { launch, after_tick };
        self.riptide_pending = Some(request);
        Some(request)
    }

    /// Returns an admitted request that has not yet completed its launch tick.
    pub(in crate::movement) fn pending_riptide_request(&self) -> Option<RiptideRequest> {
        self.riptide_pending
    }

    /// Restores only the request whose release survived a spatial correction.
    pub(in crate::movement) fn restore_riptide_request(&mut self, request: Option<RiptideRequest>) {
        self.riptide_pending = request;
    }

    /// Drops a restored launch when its original input has since reached the socket.
    pub(in crate::movement) fn retire_written_riptide_request(&mut self, tick: u64) {
        if self
            .riptide_pending
            .is_some_and(|request| tick > request.after_tick)
        {
            self.riptide_pending = None;
        }
    }

    /// Removes an unsent release's pending and retained launch before correction replay.
    pub(in crate::movement) fn discard_riptide_release_after(
        &mut self,
        tick: u64,
        world: &impl CollisionWorld,
    ) -> Result<(), PhysicsCorrectionError> {
        self.riptide_pending = None;
        let mut changed = false;
        for input in self.history.retained_inputs_after_mut(tick) {
            changed |= input.riptide.take().is_some();
        }
        if changed {
            // Rebuild even when the incoming anchor follows the launch: its retained velocity
            // and spin must no longer contain an action whose transaction was discarded.
            self.replay_retained_from(tick, world)?;
        }
        Ok(())
    }
}
