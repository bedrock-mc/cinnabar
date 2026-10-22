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
    /// ended a swim, crawl or glide after that sample), movement input and
    /// terrain collision.
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
        Some(
            frame
                .input
                .mode
                .hitbox_height(sneaking, frame.input.sneak_profile) as f32,
        )
    }

    /// Selects the sneaking box and crouch eye for the level's StartGame base
    /// game version; it survives prediction resets until the next StartGame.
    pub fn set_base_game_version(&mut self, version: protocol::BaseGameVersion) {
        self.modes
            .set_sneak_profile(sim::SneakProfile::for_base_game_version(version.version()));
    }

    /// The level's sneaking dimensions, for queries outside the simulation tick.
    #[must_use]
    pub const fn sneak_profile(&self) -> sim::SneakProfile {
        self.modes.sneak_profile()
    }
}
