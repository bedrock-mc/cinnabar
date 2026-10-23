//! Current posture for correction syncs, independent of uncompleted local input.

use super::*;

/// Minimum squared displacement that marks the completed actor tick as moving.
const MOVING_FLAG_DISTANCE_SQUARED: f32 = 0.001;

/// Accepted flag writes and size, independent of replay history and travel-mode selection.
#[derive(Debug, Clone, Copy, Default)]
pub(super) struct SyncState {
    pub flags: client_world::MovementFlagUpdate,
    pub size: Option<[f32; 2]>,
    explicit_size: bool,
    dimensions: client_world::ExplicitDimensions,
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

    /// Applies newly supplied components to the current size, preserving the other axis.
    fn server_dimensions(&mut self, dimensions: client_world::ExplicitDimensions) {
        if let Some(revision) = dimensions.spawn_revision
            && self.dimensions.spawn_revision != Some(revision)
        {
            if self
                .dimensions
                .spawn_revision
                .is_some_and(|current| revision < current)
            {
                return;
            }
            if self.dimensions.spawn_revision.is_some() {
                self.size = None;
                self.explicit_size = false;
                self.last_pose = (false, false);
            }
            self.dimensions = client_world::ExplicitDimensions::for_spawn(revision);
        }
        let mut size = self
            .size
            .unwrap_or([sim::PLAYER_WIDTH as f32, sim::PLAYER_HEIGHT as f32]);
        let mut changed = false;
        for ((previous, received), value) in
            [&mut self.dimensions.width, &mut self.dimensions.height]
                .into_iter()
                .zip([dimensions.width, dimensions.height])
                .zip(&mut size)
        {
            if let Some(write) = received.filter(|write| Some(*write) != *previous) {
                *previous = Some(write);
                *value = write.value;
                changed = true;
            }
        }
        if changed {
            self.size = Some(size);
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
        self.server_dimensions(live.dimensions);
    }

    /// Records producer writes after a successful movement tick.
    pub(super) fn complete_tick(&mut self, frame: &ControllerFrame, state: &PlayerState) {
        self.server_dimensions(frame.intent.explicit_dimensions);
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
        self.sync_state.flags
    }

    /// Records a server dimension write before any later local resize is evaluated.
    pub(in crate::movement) fn note_server_dimensions(
        &mut self,
        dimensions: client_world::ExplicitDimensions,
    ) {
        self.sync_state.server_dimensions(dimensions);
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
