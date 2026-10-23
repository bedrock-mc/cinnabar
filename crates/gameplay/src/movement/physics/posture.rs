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
    /// Completed block-climber flags with newer accepted server edits; absent
    /// when the server's flags stand (see [`ClimberFlags::after_tick`]).
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
    /// Associates each owned flag with its wire identity in one place.
    fn fields_mut(&mut self) -> [(protocol_ids::ActorFlag, &mut bool); 5] {
        use protocol_ids::ActorFlag;
        [
            (ActorFlag::InScaffolding, &mut self.in_scaffolding),
            (ActorFlag::OverScaffolding, &mut self.over_scaffolding),
            (
                ActorFlag::DescendThroughBlock,
                &mut self.descending_through_block,
            ),
            (ActorFlag::InAscendingBlock, &mut self.in_ascending_block),
            (
                ActorFlag::OverDescendingBlock,
                &mut self.over_descending_block,
            ),
        ]
    }

    /// Encodes only the five climber flags in the second actor flag word.
    pub(in crate::movement) fn word(mut self) -> u64 {
        self.fields_mut()
            .into_iter()
            .fold(0, |word, (flag, value)| {
                word | (u64::from(*value) << (flag.wire_value() % u64::BITS))
            })
    }

    /// Returns the second-word mask owned by the climber and descent action.
    pub(in crate::movement) fn mask() -> u64 {
        Self::default()
            .fields_mut()
            .into_iter()
            .fold(0, |mask, (flag, _)| {
                assert_eq!(flag.wire_value() / u64::BITS, 1);
                mask | (1_u64 << (flag.wire_value() % u64::BITS))
            })
    }

    /// Decodes the climber flags without adopting unrelated actor flags.
    pub(super) fn from_word(word: u64) -> Self {
        let mut flags = Self::default();
        for (flag, value) in flags.fields_mut() {
            *value = word & (1_u64 << (flag.wire_value() % u64::BITS)) != 0;
        }
        flags
    }

    /// Makes accepted contact available to the next descent and climb-jump actions.
    pub(super) fn apply(self, state: &mut PlayerState, mask: u64) {
        let mask = Self::from_word(mask);
        state.update_climber_contact(
            mask.in_scaffolding.then_some(self.in_scaffolding),
            mask.over_scaffolding.then_some(self.over_scaffolding),
            mask.in_ascending_block.then_some(self.in_ascending_block),
            mask.over_descending_block
                .then_some(self.over_descending_block),
        );
    }

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
            climber: self.live_climber,
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
