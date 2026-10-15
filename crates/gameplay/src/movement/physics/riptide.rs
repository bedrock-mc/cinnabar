//! Released trident requests and cancellation before transport admission.

use super::*;

impl LocalPhysicsController {
    /// Retains release-time facing until a successful tick; unsupported requests are retired.
    pub fn request_riptide(&mut self, level: u8, rotation: [f32; 2]) -> bool {
        let Some(launch) = sim::RiptideLaunch::new(level, rotation) else {
            super::super::diagnostics::note_skipped_authority("riptide", f64::from(level));
            return false;
        };
        if !self.is_active() {
            return false;
        }
        self.riptide_pending = Some(launch);
        true
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
