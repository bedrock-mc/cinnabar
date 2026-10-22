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
    /// Block-climber flags of the latest completed tick, absent when the
    /// server's flags stand (see [`ClimberFlags::after_tick`]).
    pub climber: Option<ClimberFlags>,
}

/// Block-climber flags that vanilla rewrites locally every movement tick.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub(in crate::movement) struct ClimberFlags {
    pub in_scaffolding: bool,
    pub over_scaffolding: bool,
    pub descending_through_block: bool,
    pub in_ascending_block: bool,
    pub over_descending_block: bool,
}

impl ClimberFlags {
    /// Climber flags after one completed tick. Spectator flags stay the
    /// server's. A tick that published contact (measured, or retained by the
    /// simulator) reports it; one with no contact known keeps the contact of
    /// `previous`, the prior tick's flags, with this tick's descent.
    pub(in crate::movement) fn after_tick(
        input: &sim::MovementInput,
        environment: &sim::MovementEnvironment,
        previous: Option<Self>,
    ) -> Option<Self> {
        if input.spectator {
            None
        } else if environment.climber_unsampled {
            // Unmeasured contact stays as before, but the descent action still
            // reads this tick's request; in place, it reads the kept contact.
            previous.map(|previous| Self {
                descending_through_block: if input.immobile {
                    sim::descends_through_block(input, previous.over_descending_block)
                } else {
                    environment.descending_through_block
                },
                ..previous
            })
        } else {
            Some(Self::from(environment))
        }
    }
}

impl From<&sim::MovementEnvironment> for ClimberFlags {
    /// Copies the climber flags a completed tick published.
    fn from(environment: &sim::MovementEnvironment) -> Self {
        Self {
            in_scaffolding: environment.in_scaffolding,
            over_scaffolding: environment.over_scaffolding,
            descending_through_block: environment.descending_through_block,
            in_ascending_block: environment.in_ascending_block,
            over_descending_block: environment.over_descending_block,
        }
    }
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
    /// ended a swim, crawl or glide after that sample), movement input,
    /// terrain collision and the block-climber flags.
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
            climber: self
                .controller_history
                .back()
                .and_then(|frame| frame.climber),
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
