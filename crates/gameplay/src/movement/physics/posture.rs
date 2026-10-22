//! Current posture for correction syncs, independent of uncompleted local input.

use super::*;

/// Actor flags the client sets on its own flag words, as of the latest completed sample.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(in crate::movement) struct SyncPose {
    pub sneaking: bool,
    pub sprinting: bool,
    pub mode: sim::MovementMode,
    /// Nonzero processed movement input.
    pub moving: bool,
    /// Terrain collision, absent until a controller frame exists.
    pub has_collision: Option<bool>,
    /// A Riptide spin, including one a server flag started or cancelled.
    pub spinning: bool,
}

impl LocalPhysicsController {
    /// Overlays server posture accepted after the latest completed movement sample.
    pub(in crate::movement) fn prediction_sync_posture(&self) -> Option<(bool, bool)> {
        let (sneaking, sprinting) = self.latest_sneak_sprint()?;
        Some((
            self.live_posture.sneaking.unwrap_or(sneaking),
            self.live_posture.sprinting.unwrap_or(sprinting),
        ))
    }

    /// Locally owned actor flags once a movement sample has completed: the
    /// accepted posture, the current mode and Riptide spin (including server
    /// flag updates that started or ended them after that sample), movement
    /// input and terrain collision.
    pub(in crate::movement) fn prediction_sync_pose(&self) -> Option<SyncPose> {
        let sample = self.sample_history.back()?;
        let (sneaking, sprinting) = self.prediction_sync_posture()?;
        Some(SyncPose {
            sneaking,
            sprinting,
            mode: self.modes.mode(),
            moving: sample.move_vector != [0.0; 2],
            has_collision: self
                .controller_history
                .back()
                .map(|frame| !frame.input.no_clip),
            spinning: self
                .state
                .as_ref()
                .is_some_and(|state| state.spin_attack_ticks.is_some()),
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
