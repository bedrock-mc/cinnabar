//! Current posture for correction syncs, independent of uncompleted local input.

use super::*;

impl LocalPhysicsController {
    /// Overlays server posture accepted after the latest completed movement sample.
    pub(in crate::movement) fn prediction_sync_posture(&self) -> Option<(bool, bool)> {
        let (sneaking, sprinting) = self.latest_sneak_sprint()?;
        Some((
            self.live_posture.sneaking.unwrap_or(sneaking),
            self.live_posture.sprinting.unwrap_or(sprinting),
        ))
    }

    /// Accepted locomotion mode once a movement sample has completed, including
    /// server flag updates that ended a swim or crawl after that sample.
    pub(in crate::movement) fn prediction_sync_mode(&self) -> Option<sim::MovementMode> {
        self.sample_history.back().map(|_| self.modes.mode())
    }

    /// Uses completed walking size with any newer accepted server sneak state.
    pub(in crate::movement) fn prediction_sync_walking_height(&self) -> Option<f32> {
        let frame = self.controller_history.back()?;
        let state = self.state.as_ref()?;
        if frame.tick != state.tick
            || frame.input.mode != sim::MovementMode::Walking
            || state.spin_attack_ticks.is_some()
        {
            return None;
        }
        let sneaking = self.live_posture.sneaking.unwrap_or(frame.input.sneaking);
        Some(frame.input.mode.hitbox_height(sneaking) as f32)
    }
}
