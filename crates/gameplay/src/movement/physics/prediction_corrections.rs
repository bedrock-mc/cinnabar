//! Missing-frame prediction corrections retained for a later rewind.

use super::*;
use crate::movement::PhysicsAnchor;

/// Corrections attach to the current captured frame without changing live prediction.
#[derive(Debug, Clone, Default)]
pub(super) struct DeferredPredictionCorrections {
    frames: VecDeque<PhysicsAnchor>,
    dirty: bool,
}

impl DeferredPredictionCorrections {
    /// Keeps the last complete position/motion correction for each retained frame.
    pub(super) fn record(
        &mut self,
        mut anchor: PhysicsAnchor,
        tick: u64,
        oldest: u64,
        capacity: usize,
    ) {
        self.frames
            .retain(|frame| frame.tick >= oldest && frame.tick != tick);
        anchor.tick = tick;
        if self.frames.len() >= capacity {
            self.frames.pop_front();
        }
        self.frames.push_back(anchor);
        self.dirty = true;
    }

    /// A newer correction of the same frame replaces all earlier spatial correction values.
    pub(super) fn supersede(&mut self, corrected_tick: u64) {
        self.frames
            .retain(|frame| Some(frame.tick) != corrected_tick.checked_add(1));
    }

    /// A later velocity-only correction wins while preserving the earlier position and ground state.
    pub(super) fn replace_motion(&mut self, tick: u64, velocity: [f32; 3]) {
        if let Some(anchor) = self.frames.iter_mut().find(|frame| frame.tick == tick) {
            anchor.velocity = Some(velocity);
        }
    }

    /// Pending history corrections prevent an otherwise matching echo from skipping replay.
    pub(super) fn needs_replay(&self) -> bool {
        self.dirty
    }

    /// Clears the dirty marker while retaining corrections for repeated rewinds.
    pub(super) fn mark_replayed(&mut self) {
        self.dirty = false;
    }

    /// Applies retained spatial state before the input of its captured frame is simulated.
    pub(super) fn apply_before(&self, state: &mut PlayerState) {
        let Some(anchor) = self
            .frames
            .iter()
            .find(|frame| Some(frame.tick) == state.tick.checked_add(1))
        else {
            return;
        };
        state.position = Vec3::new(
            f64::from(anchor.network_position[0]),
            f64::from(anchor.network_position[1] - PLAYER_NETWORK_OFFSET),
            f64::from(anchor.network_position[2]),
        );
        state.on_ground = anchor.on_ground;
        // The previous location's collisions cannot authorize a climb at the corrected position.
        state.collisions = sim::AxisCollisions::default();
        if let Some(velocity) = anchor
            .velocity
            .filter(|velocity| super::timeline::motion_is_simulable(*velocity))
        {
            state.velocity = Vec3::new(
                f64::from(velocity[0]),
                f64::from(velocity[1]),
                f64::from(velocity[2]),
            );
        }
    }
}

impl LocalPhysicsController {
    /// Preserves later authoritative state when a failed replay must discard speculative history.
    pub(in crate::movement) fn replay_fallback_anchor(
        &self,
        anchor: PhysicsAnchor,
    ) -> PhysicsAnchor {
        if !self.retains_tick(anchor.tick) {
            return anchor;
        }
        let Some(superseded_boundary) = anchor.tick.checked_add(1) else {
            return anchor;
        };
        let Some(mut latest) = self
            .deferred_corrections
            .frames
            .iter()
            .filter(|frame| frame.tick > superseded_boundary && self.retains_tick(frame.tick))
            .max_by_key(|frame| frame.tick)
            .copied()
        else {
            return anchor;
        };
        // Same-frame motion was merged on arrival; later frames replace only velocity.
        if let Some(motion) = self
            .server_motions
            .iter()
            .filter(|motion| motion.tick > latest.tick)
            .max_by_key(|motion| motion.tick)
        {
            latest.velocity = Some([
                motion.velocity.x as f32,
                motion.velocity.y as f32,
                motion.velocity.z as f32,
            ]);
        }
        latest
    }

    /// Prediction corrections require a nonzero tick at or above the retained history floor.
    pub(in crate::movement) fn prediction_correction_is_eligible(&self, tick: u64) -> bool {
        tick != 0
            && self
                .history
                .oldest_tick()
                .is_some_and(|oldest| tick >= oldest)
    }

    /// Stores a missing tick's correction on the current frame, without initiating a rewind.
    pub(in crate::movement) fn defer_prediction_correction(&mut self, anchor: PhysicsAnchor) {
        let Some(tick) = self
            .state
            .as_ref()
            .map(|state| state.tick)
            .filter(|tick| self.retains_tick(*tick))
        else {
            return;
        };
        self.deferred_corrections.record(
            anchor,
            tick,
            self.history.oldest_tick().unwrap_or(tick),
            self.history_capacity,
        );
    }

    /// Reports corrections recorded since the last replay so a matching echo cannot discard them.
    pub(in crate::movement) fn has_pending_prediction_corrections(&self) -> bool {
        self.deferred_corrections.needs_replay()
    }
}
