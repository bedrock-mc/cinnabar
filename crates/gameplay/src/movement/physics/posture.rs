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
    /// A riptide spin is active.
    pub spinning: bool,
    /// Terrain collision, absent until a controller frame exists.
    pub has_collision: Option<bool>,
    /// Scaffolding at the feet layer.
    pub in_scaffolding: bool,
    /// A sneak descent through supported scaffolding.
    pub descending_through_scaffolding: bool,
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
    /// accepted posture, the current mode (including server flag updates that
    /// ended a swim or crawl after that sample), movement input, riptide spin,
    /// terrain collision and scaffolding contact.
    pub(in crate::movement) fn prediction_sync_pose(&self) -> Option<SyncPose> {
        let sample = self.sample_history.back()?;
        let (sneaking, sprinting) = self.prediction_sync_posture()?;
        Some(SyncPose {
            sneaking,
            sprinting,
            mode: self.modes.mode(),
            moving: sample.move_vector != [0.0; 2],
            spinning: sample.processed.spinning,
            has_collision: self
                .controller_history
                .back()
                .map(|frame| !frame.input.no_clip),
            in_scaffolding: self.last_environment.in_scaffolding,
            descending_through_scaffolding: self.last_environment.descending_through_scaffolding,
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
