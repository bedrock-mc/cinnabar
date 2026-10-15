//! Fixed-tick dragon flap phase and the yaw/height history supplied to its authored rig.
use assets::facts::entity::NativeEntityClass;
use protocol_ids::{ActorFlag, Attribute};

use super::ActorStore;

const HISTORY_LENGTH: usize = 64;
pub(crate) const HISTORICAL_VARIABLES: usize = 24;
const FLAP_GAIN: f32 = 0.2;
const HORIZONTAL_SPEED_GAIN: f32 = 10.0;
const SITTING_FLAP_GAIN: f32 = 0.1;

#[derive(Debug, Clone, PartialEq)]
pub(crate) struct State {
    pub(crate) flap_phase: f32,
    history: [[f32; 2]; HISTORY_LENGTH],
    latest: usize,
    initialized: bool,
}

impl Default for State {
    fn default() -> Self {
        Self {
            flap_phase: 0.0,
            history: [[0.0; 2]; HISTORY_LENGTH],
            latest: 0,
            initialized: false,
        }
    }
}

impl State {
    fn advance(
        &mut self,
        position: [f32; 3],
        delta: [f32; 3],
        yaw: f32,
        sitting: bool,
        dead: bool,
    ) {
        if dead {
            self.flap_phase = 0.0;
            return;
        }
        if !position
            .into_iter()
            .chain(delta)
            .chain([yaw])
            .all(f32::is_finite)
        {
            return;
        }
        let gain = if sitting {
            SITTING_FLAP_GAIN
        } else {
            delta[1].exp2() * FLAP_GAIN
                / ((delta[0] * delta[0] + delta[2] * delta[2]).sqrt() * HORIZONTAL_SPEED_GAIN + 1.0)
        };
        let phase = self.flap_phase + gain;
        if phase.is_finite() {
            self.flap_phase = phase;
        }
        let frame = [(yaw + 180.0).rem_euclid(360.0) - 180.0, position[1]];
        if !self.initialized {
            self.history.fill(frame);
            self.initialized = true;
        }
        self.latest = (self.latest + 1) % HISTORY_LENGTH;
        self.history[self.latest] = frame;
    }

    /// Completed-tick historical yaw/height; death reads one tick farther into retained history.
    pub(crate) fn historical_frame(&self, offset: usize, dead: bool) -> [f32; 2] {
        self.history[(self.latest + HISTORY_LENGTH - (offset + usize::from(dead)) % HISTORY_LENGTH)
            % HISTORY_LENGTH]
    }
}

impl ActorStore {
    pub(super) fn advance_dragon_animation(&mut self) {
        for actor in self.actors.values_mut() {
            if actor.entity_fact.native_class != NativeEntityClass::EnderDragon {
                continue;
            }
            let dead = actor.status.dead
                || actor
                    .attributes
                    .get(Attribute::Health.wire_name())
                    .is_some_and(|health| health.current <= 0.0);
            let sitting = actor.flag(ActorFlag::Sitting.wire_value());
            let delta = std::array::from_fn(|axis| {
                actor.position[axis] - actor.previous_pose.position[axis]
            });
            actor
                .dragon_animation
                .get_or_insert_with(Default::default)
                .advance(actor.position, delta, actor.yaw, sitting, dead);
        }
    }
}

#[cfg(test)]
mod tests;
