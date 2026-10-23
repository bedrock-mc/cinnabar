//! Current posture for correction syncs, independent of uncompleted local input.

use super::*;

/// Minimum squared displacement that marks the completed actor tick as moving.
const MOVING_FLAG_DISTANCE_SQUARED: f32 = 0.001;

/// Actor flags the client sets on its own flag words, as of the latest completed sample.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(in crate::movement) struct SyncPose {
    pub sneaking: bool,
    pub sprinting: bool,
    pub mode: sim::MovementMode,
    /// Completed displacement exceeds the actor movement threshold.
    pub moving: bool,
    /// Terrain collision, absent until a controller frame exists.
    pub has_collision: Option<bool>,
}

impl LocalPhysicsController {
    /// Returns effective flight independently of the current travel mode.
    pub(in crate::movement) fn prediction_sync_flying(&self) -> bool {
        self.modes.flying()
    }

    /// Overlays server posture accepted after the latest completed movement sample.
    pub(in crate::movement) fn prediction_sync_posture(&self) -> Option<(bool, bool)> {
        let (sneaking, sprinting) = self.latest_sneak_sprint()?;
        Some((
            self.live_posture.sneaking.unwrap_or(sneaking),
            self.live_posture.sprinting.unwrap_or(sprinting),
        ))
    }

    /// Locally owned actor flags once a movement sample has completed: the
    /// accepted posture, the current mode (including server flag updates that
    /// ended a swim, crawl or glide after that sample), displacement and
    /// terrain collision.
    pub(in crate::movement) fn prediction_sync_pose(&self) -> Option<SyncPose> {
        let state = self.state.as_ref()?;
        let (sneaking, sprinting) = self.prediction_sync_posture()?;
        let dx = state.position.x as f32 - self.previous_position.x as f32;
        let dy = state.position.y as f32 - self.previous_position.y as f32;
        let dz = state.position.z as f32 - self.previous_position.z as f32;
        Some(SyncPose {
            sneaking,
            sprinting,
            mode: self.modes.mode(),
            moving: dz * dz + dy * dy + dx * dx > MOVING_FLAG_DISTANCE_SQUARED,
            has_collision: self
                .controller_history
                .back()
                .map(|frame| !frame.input.no_clip),
        })
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
