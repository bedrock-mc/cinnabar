//! Current posture for correction syncs, independent of uncompleted local input.

use super::*;

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

/// Minimum squared displacement that marks the completed actor tick as moving.
const MOVING_FLAG_DISTANCE_SQUARED: f32 = 0.001;

/// Accepted flag writes and size, independent of replay history and travel-mode selection.
#[derive(Debug, Clone, Copy, Default)]
pub(super) struct SyncState {
    pub flags: client_world::MovementFlagUpdate,
    pub size: Option<[f32; 2]>,
    explicit_size: bool,
    dimensions_revision: Option<u64>,
    last_pose: (bool, bool),
    previous_mode: sim::MovementMode,
    pending: client_world::MovementFlagUpdate,
    local_spinning: bool,
}

impl SyncState {
    /// Applies only supplied server bits, preserving independent pose flags.
    pub(super) fn server_flags(&mut self, update: client_world::MovementFlagUpdate) {
        self.flags.merge(update);
        self.pending.merge(update);
        if !self.explicit_size
            && [
                update.sneaking,
                update.gliding,
                update.swimming,
                update.crawling,
                update.spinning,
            ]
            .iter()
            .any(Option::is_some)
        {
            self.resize();
        }
    }

    /// Observes a new explicit request, independently of its numerical dimensions.
    fn server_dimensions(&mut self, revision: Option<u64>) {
        if revision.is_some() && revision != self.dimensions_revision {
            self.dimensions_revision = revision;
            self.size = None;
            self.explicit_size = true;
        }
    }

    /// Associates server writes with the next retained tick for correction replay.
    pub(super) fn take_pending(&mut self) -> client_world::MovementFlagUpdate {
        std::mem::take(&mut self.pending)
    }

    /// Reapplies server writes received after the last completed tick to a replay result.
    pub(super) fn restore_pending(&mut self, live: Self) {
        self.server_flags(live.pending);
        self.server_dimensions(live.dimensions_revision);
    }

    /// Records producer writes after a successful movement tick.
    pub(super) fn complete_tick(&mut self, frame: &ControllerFrame, state: &PlayerState) {
        self.server_dimensions(frame.intent.dimensions_revision);
        self.flags.sneaking = Some(frame.input.sneaking);
        self.flags.sprinting = Some(frame.input.sprinting);
        self.flags.has_collision = Some(!frame.input.no_clip);
        if !frame.intent.vehicle_controlled_locally || frame.input.mode != sim::MovementMode::Riding
        {
            self.flags.moving = Some(moving_between(frame.position_before_tick, state.position));
        }
        for (slot, mode) in [
            (&mut self.flags.gliding, sim::MovementMode::Gliding),
            (&mut self.flags.swimming, sim::MovementMode::Swimming),
            (&mut self.flags.crawling, sim::MovementMode::Crawling),
        ] {
            *slot = Some(frame.input.mode == mode);
        }
        self.previous_mode = frame.input.mode;
        self.local_spinning = state.spin_attack_ticks.is_some();
        self.resize();
    }

    /// A pose change replaces an explicit request; unchanged poses retain its dimensions.
    fn resize(&mut self) {
        let horizontal = self.local_spinning
            || self.flags.spinning == Some(true)
            || [self.flags.gliding, self.flags.swimming, self.flags.crawling].contains(&Some(true));
        let pose = (
            self.previous_mode == sim::MovementMode::Walking
                && self.flags.sneaking.unwrap_or(false),
            horizontal,
        );
        if pose != self.last_pose {
            self.explicit_size = false;
        }
        self.last_pose = pose;
        if !self.explicit_size {
            self.size = Some([
                sim::PLAYER_WIDTH as f32,
                if horizontal {
                    sim::PLAYER_WIDTH as f32
                } else {
                    sim::MovementMode::Walking.hitbox_height(pose.0) as f32
                },
            ]);
        }
    }
}

impl LocalPhysicsController {
    /// Returns the live contact flags after accepted writes and completed local producers.
    pub(in crate::movement) fn prediction_sync_climber(&self) -> Option<ClimberFlags> {
        self.live_climber
    }

    /// Returns effective flight independently of the current travel mode.
    pub(in crate::movement) fn prediction_sync_flying(&self) -> bool {
        self.modes.flying()
    }

    /// Overlays server posture accepted after the latest completed movement sample.
    #[cfg(test)]
    pub(in crate::movement) fn prediction_sync_posture(&self) -> Option<(bool, bool)> {
        let (sneaking, sprinting) = self.latest_sneak_sprint()?;
        Some((
            self.live_posture.sneaking.unwrap_or(sneaking),
            self.live_posture.sprinting.unwrap_or(sprinting),
        ))
    }

    /// Returns the current accepted writes, including newer server changes.
    pub(in crate::movement) fn prediction_sync_flags(&self) -> client_world::MovementFlagUpdate {
        let mut flags = self.sync_state.flags;
        flags.extended_flags = self.prediction_sync_climber().map(ClimberFlags::word);
        flags
    }

    /// Records a server dimension write before any later local resize is evaluated.
    pub(in crate::movement) fn note_server_dimensions(&mut self, revision: Option<u64>) {
        self.sync_state.server_dimensions(revision);
    }

    /// Whether a newer explicit dimension request still owns the reported size.
    pub(in crate::movement) fn prediction_sync_explicit_size(&self) -> bool {
        self.sync_state.explicit_size
    }

    /// Returns the unscaled size written by the most recent local resize.
    pub(in crate::movement) fn prediction_sync_size(&self) -> Option<[f32; 2]> {
        self.sync_state.size
    }

    /// Uses the current accepted local size for walking posture observations.
    #[cfg(test)]
    pub(in crate::movement) fn prediction_sync_walking_height(&self) -> Option<f32> {
        self.sync_state.size.map(|size| size[1])
    }
}

/// Tests completed position displacement with the actor flag's f32 threshold.
fn moving_between(before: Vec3, after: Vec3) -> bool {
    let dx = after.x as f32 - before.x as f32;
    let dy = after.y as f32 - before.y as f32;
    let dz = after.z as f32 - before.z as f32;
    dz * dz + (dy * dy + dx * dx) > MOVING_FLAG_DISTANCE_SQUARED
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn moving_threshold_keeps_the_small_vertical_and_horizontal_contributions() {
        assert!(moving_between(
            Vec3::ZERO,
            Vec3::new(0.005, 0.025, 0.01870828866958618),
        ));
    }
}
