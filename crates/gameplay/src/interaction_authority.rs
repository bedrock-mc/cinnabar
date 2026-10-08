//! Frozen interaction authority and shared range checks.
use crate::mining::{
    FrozenMiningFrame, FrozenMiningRay, FrozenMiningSelection, FrozenMiningTarget,
};
use protocol::PlayerInputMode;
#[cfg(test)]
use std::num::NonZeroU64;

/// Wall-clock millis a deferred press may wait for fresh evidence or a tick, whatever the frame
/// rate, without outliving a stalled simulation.
pub const MAX_PENDING_INTERACTION_MILLIS: u64 = 10 * world::TICK_DURATION.as_millis() as u64;

#[derive(Debug, Clone, PartialEq)]
pub struct FrozenBlockObservation {
    pub frame: FrozenMiningFrame,
    pub ray: FrozenMiningRay,
    pub reach: f64,
    pub input_mode: PlayerInputMode,
    pub selection: FrozenMiningSelection,
    pub target: FrozenMiningTarget,
}

/// Server-side pick checks measure to the block's minimum corner with this slack
/// over the game-mode pick range. Needs independent measurement.
const SERVER_PICK_SLACK: f64 = 0.5;

/// Vanilla limits a pick by the eye-to-block-centre distance, not the ray length.
///
/// Only touch reach (6.7 survival, 12 creative) equals the server's range, so only
/// touch picks can exceed its corner check; those are dropped too.
pub fn within_pick_range(observed: &FrozenBlockObservation) -> bool {
    within_pick_range_of(observed, observed.ray.origin)
}

/// The pick range check measured from `eye` rather than the ray's origin.
pub fn within_pick_range_of(observed: &FrozenBlockObservation, eye: [f32; 3]) -> bool {
    let distance_squared = |offset: f64| {
        observed
            .target
            .position
            .into_iter()
            .zip(eye)
            .map(|(block, eye)| (f64::from(block) + offset - f64::from(eye)).powi(2))
            .sum::<f64>()
    };
    let corner_limit = observed.reach + SERVER_PICK_SLACK;
    distance_squared(0.5) <= observed.reach * observed.reach
        && (observed.input_mode != PlayerInputMode::Touch
            || distance_squared(0.0) <= corner_limit * corner_limit)
}

#[cfg(test)]
impl FrozenBlockObservation {
    /// A top-face hit on `position` holding `item` in slot 2.
    pub fn fixture(position: [i32; 3], face: u8, item: protocol::VerifiedNetworkItemStack) -> Self {
        let identity = sim::CollisionQuery::synthetic(()).identity;
        Self {
            frame: FrozenMiningFrame {
                session_generation: 7,
                position_authority_generation: 0,
                input_authority_generation: NonZeroU64::MIN,
                input_frame_sequence: 1,
                fifo_sequence: 1,
                physics_tick: 101,
                pose_generation: 1,
            },
            ray: FrozenMiningRay {
                origin: [0.5, 65.62, 0.5],
                direction: [0.0, -1.0, 0.0],
                movement_world_identity: identity.clone(),
                world_identity: identity.clone(),
            },
            reach: 5.7,
            input_mode: PlayerInputMode::Mouse,
            selection: FrozenMiningSelection { slot: 2, item },
            target: FrozenMiningTarget {
                position,
                face,
                relative_hit: [0.5, 1.0, 0.5],
                runtime_id: 9,
                identity,
            },
        }
    }
}
