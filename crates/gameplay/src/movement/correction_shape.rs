//! Classification and reconciliation of committed local-player corrections and
//! other tick-stamped timeline edits into prediction and the outbox.

use protocol::PLAYER_NETWORK_OFFSET;
use sim::CollisionWorld;

use super::physics::{self, LocalPhysicsController};
use super::{
    MovementTicker, PhysicsAuthorityFault, PhysicsCorrectionMode, PhysicsCorrectionOutcome,
};

/// How one committed correction must be applied to prediction state.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CorrectionShape {
    /// Position, motion and ground flag match the retained frame within the
    /// vanilla epsilon, so nothing is replayed and no state is touched.
    Confirmed,
    /// Replace the retained authoritative state and replay later inputs,
    /// regardless of the distance between predicted and corrected positions.
    Replay,
}

/// Squared distance within which vanilla treats a correction's position and
/// motion as already matching the retained frame.
const CORRECTION_MATCH_EPSILON_SQUARED: f32 = 1.0e-5;

impl LocalPhysicsController {
    /// Classifies one committed correction against prediction state retained
    /// for the correction's own authoritative tick.
    ///
    /// Matching position, motion (when carried) and ground flag within the
    /// vanilla epsilon needs no replay. A missing retained tick selects replay
    /// so the not-retained policy decides instead of unrelated current state.
    #[must_use]
    pub fn correction_shape(
        &self,
        network_position: [f32; 3],
        correction_tick: u64,
        on_ground: bool,
        velocity: Option<[f32; 3]>,
    ) -> CorrectionShape {
        if !network_position.into_iter().all(f32::is_finite)
            || self.has_pending_prediction_corrections()
        {
            // Invalid anchors are rejected before reconciliation mutates state.
            return CorrectionShape::Replay;
        }
        let Some(state) = self.retained_state(correction_tick) else {
            return CorrectionShape::Replay;
        };
        let current = [
            state.position.x as f32,
            state.position.y as f32 + PLAYER_NETWORK_OFFSET,
            state.position.z as f32,
        ];
        let position_error = squared_distance(current, network_position);
        let velocity_matches = velocity.is_none_or(|velocity| {
            let retained = [
                state.velocity.x as f32,
                state.velocity.y as f32,
                state.velocity.z as f32,
            ];
            squared_distance(retained, velocity) <= CORRECTION_MATCH_EPSILON_SQUARED
        });
        if position_error <= CORRECTION_MATCH_EPSILON_SQUARED
            && velocity_matches
            && state.on_ground == on_ground
        {
            return CorrectionShape::Confirmed;
        }
        CorrectionShape::Replay
    }
}

/// Compares positions or velocities in the packet's float precision.
fn squared_distance(a: [f32; 3], b: [f32; 3]) -> f32 {
    let dx = a[0] - b[0];
    let dy = a[1] - b[1];
    let dz = a[2] - b[2];
    dx * dx + dy * dy + dz * dz
}

/// Applies one committed correction to prediction according to its shape.
///
/// `Ok(None)` means the correction confirmed the current prediction and
/// deliberately mutated nothing — no replay, no interpolation re-anchor, no
/// settle-window engagement. `Ok(Some(_))` reports the applied outcome for
/// evidence attribution.
pub fn reconcile_committed_correction(
    ticker: &mut MovementTicker,
    physics: &mut LocalPhysicsController,
    network_position: [f32; 3],
    correction_tick: u64,
    on_ground: bool,
    velocity: Option<[f32; 3]>,
    world: &impl CollisionWorld,
) -> Result<Option<PhysicsCorrectionOutcome>, PhysicsAuthorityFault> {
    let shape = physics.correction_shape(network_position, correction_tick, on_ground, velocity);
    if shape != CorrectionShape::Confirmed {
        super::diagnostics::note_correction(
            super::diagnostics::CorrectionKind::Correct,
            correction_tick,
            network_position,
            on_ground,
            physics.sample_at(correction_tick),
        );
    }
    let mode = match shape {
        CorrectionShape::Confirmed => return Ok(None),
        CorrectionShape::Replay => PhysicsCorrectionMode::ReplayIfRetained,
    };
    reconcile_physics_anchor(
        ticker,
        physics,
        PhysicsAnchor {
            network_position,
            tick: correction_tick,
            on_ground,
            velocity,
        },
        mode,
        world,
    )
    .map(Some)
}

/// Enters one `CorrectPlayerMovePrediction` into prediction.
///
/// Old and zero ticks are discarded. Missing newer ticks attach to the current
/// captured frame and wait for a later correction to initiate a rewind.
pub fn reconcile_prediction_correction(
    ticker: &mut MovementTicker,
    physics: &mut LocalPhysicsController,
    network_position: [f32; 3],
    correction_tick: u64,
    on_ground: bool,
    velocity: [f32; 3],
    world: &impl CollisionWorld,
) -> Result<Option<PhysicsCorrectionOutcome>, PhysicsAuthorityFault> {
    if !physics.prediction_correction_is_eligible(correction_tick) {
        super::diagnostics::note_dropped_correction(correction_tick);
        return Ok(None);
    }
    if !physics.retains_tick(correction_tick) {
        if !ticker.physics_is_authorized() {
            return Err(PhysicsAuthorityFault::Unauthorized);
        }
        if !network_position.into_iter().all(f32::is_finite) {
            return Err(PhysicsAuthorityFault::CorrectionReplayFailed);
        }
        physics.defer_prediction_correction(PhysicsAnchor {
            network_position,
            tick: correction_tick,
            on_ground,
            velocity: Some(velocity),
        });
        return Ok(None);
    }
    reconcile_committed_correction(
        ticker,
        physics,
        network_position,
        correction_tick,
        on_ground,
        Some(velocity),
        world,
    )
}

/// Live-to-target distance under which vanilla rewinds a tick-stamped teleport
/// `MovePlayer`.
const MOVE_PLAYER_REWIND_DISTANCE: f32 = 16.0;

/// Enters one teleport-mode `MovePlayer`: a nearby, retained, unmounted tick
/// replays from it with motion cleared, as vanilla does; anything
/// else resets history and snaps.
pub fn reconcile_move_player_teleport(
    ticker: &mut MovementTicker,
    physics: &mut LocalPhysicsController,
    network_position: [f32; 3],
    tick: u64,
    on_ground: bool,
    world: &impl CollisionWorld,
) -> Result<PhysicsCorrectionOutcome, PhysicsAuthorityFault> {
    let nearby = physics.network_position().is_some_and(|live| {
        squared_distance(live, network_position)
            < MOVE_PLAYER_REWIND_DISTANCE * MOVE_PLAYER_REWIND_DISTANCE
    });
    let mode =
        if nearby && physics.retains_tick(tick) && physics.mode() != sim::MovementMode::Riding {
            PhysicsCorrectionMode::ReplayIfRetained
        } else {
            PhysicsCorrectionMode::Snap
        };
    reconcile_physics_anchor(
        ticker,
        physics,
        PhysicsAnchor {
            network_position,
            tick,
            on_ground,
            velocity: Some([0.0; 3]),
        },
        mode,
        world,
    )
}

/// Authoritative end-of-tick player state carried by a correction.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct PhysicsAnchor {
    pub network_position: [f32; 3],
    pub tick: u64,
    pub on_ground: bool,
    /// Server velocity; `None` keeps the retained velocity.
    pub velocity: Option<[f32; 3]>,
}

pub fn reconcile_candidate_physics_correction(
    ticker: &mut MovementTicker,
    physics: &mut LocalPhysicsController,
    network_position: [f32; 3],
    tick: u64,
    on_ground: bool,
    mode: PhysicsCorrectionMode,
    world: &impl CollisionWorld,
) -> Result<PhysicsCorrectionOutcome, PhysicsAuthorityFault> {
    reconcile_physics_anchor(
        ticker,
        physics,
        PhysicsAnchor {
            network_position,
            tick,
            on_ground,
            velocity: None,
        },
        mode,
        world,
    )
}

pub fn reconcile_physics_anchor(
    ticker: &mut MovementTicker,
    physics: &mut LocalPhysicsController,
    anchor: PhysicsAnchor,
    mode: PhysicsCorrectionMode,
    world: &impl CollisionWorld,
) -> Result<PhysicsCorrectionOutcome, PhysicsAuthorityFault> {
    if !ticker.physics_is_authorized() {
        return Err(PhysicsAuthorityFault::Unauthorized);
    }

    if !anchor.network_position.into_iter().all(f32::is_finite) {
        return Err(PhysicsAuthorityFault::CorrectionReplayFailed);
    }

    let apply_candidate = |anchor: PhysicsAnchor, mode| {
        let aligned_tick = match mode {
            PhysicsCorrectionMode::ReplayIfRetained => anchor.tick,
            PhysicsCorrectionMode::Snap => ticker
                .next_tick
                .max(anchor.tick.saturating_add(1))
                .saturating_sub(1),
        };
        let mut candidate_physics = physics.clone();
        let mut candidate_ticker = ticker.clone();
        let confirmation = candidate_ticker.sent_confirmation(aligned_tick);
        let plan = candidate_physics
            .apply_correction(
                PhysicsAnchor {
                    tick: aligned_tick,
                    ..anchor
                },
                mode,
                confirmation.as_ref(),
                world,
            )
            .map_err(|error| match error {
                physics::PhysicsCorrectionError::InvalidAnchor
                | physics::PhysicsCorrectionError::ReplayFailed => {
                    PhysicsAuthorityFault::CorrectionReplayFailed
                }
                physics::PhysicsCorrectionError::NotRetained { tick } => {
                    PhysicsAuthorityFault::CorrectionNotRetained { tick }
                }
                physics::PhysicsCorrectionError::WorldIdentityMismatch { tick } => {
                    PhysicsAuthorityFault::ReplayWorldIdentityMismatch { tick }
                }
            })?;
        candidate_ticker.apply_correction_plan(&plan)?;
        Ok((candidate_ticker, candidate_physics, plan.outcome))
    };

    let mut result = apply_candidate(anchor, mode);
    if matches!(mode, PhysicsCorrectionMode::ReplayIfRetained)
        && matches!(
            result,
            Err(PhysicsAuthorityFault::CorrectionNotRetained { .. }
                | PhysicsAuthorityFault::CorrectionReplayFailed
                | PhysicsAuthorityFault::ReplayWorldIdentityMismatch { .. }
                | PhysicsAuthorityFault::PendingWorldIdentityMismatch { .. })
        )
    {
        tracing::warn!(
            tick = anchor.tick,
            error = ?result.as_ref().err(),
            "movement replay failed; snapping correction to the current tick"
        );
        // A delayed correction can outlive local history, and replaying from a
        // changed anchor or after a newly committed subchunk can legitimately
        // encounter different immutable chunk revisions. The server position
        // remains authoritative in each case, so discard speculative history
        // and continue from a current-tick snap instead of silently restoring
        // free-camera movement.
        result = apply_candidate(
            physics.replay_fallback_anchor(anchor),
            PhysicsCorrectionMode::Snap,
        );
    }

    match result {
        Ok((candidate_ticker, candidate_physics, outcome)) => {
            *physics = candidate_physics;
            *ticker = candidate_ticker;
            Ok(outcome)
        }
        Err(fault) => {
            ticker.fail_physics_authority(&fault);
            physics.deactivate();
            Err(fault)
        }
    }
}

/// Replays retained prediction after an authoritative timeline edit at `tick`.
///
/// Failure commits nothing; the caller keeps the edit's live effect.
pub fn reconcile_timeline_rewind(
    ticker: &mut MovementTicker,
    physics: &mut LocalPhysicsController,
    tick: u64,
    world: &impl CollisionWorld,
) -> Result<PhysicsCorrectionOutcome, PhysicsAuthorityFault> {
    if !ticker.physics_is_authorized() {
        return Err(PhysicsAuthorityFault::Unauthorized);
    }
    let mut candidate_physics = physics.clone();
    let mut candidate_ticker = ticker.clone();
    let plan = candidate_physics
        .replay_retained_from(tick, world)
        .map_err(|error| match error {
            physics::PhysicsCorrectionError::NotRetained { tick } => {
                PhysicsAuthorityFault::CorrectionNotRetained { tick }
            }
            physics::PhysicsCorrectionError::WorldIdentityMismatch { tick } => {
                PhysicsAuthorityFault::ReplayWorldIdentityMismatch { tick }
            }
            physics::PhysicsCorrectionError::InvalidAnchor
            | physics::PhysicsCorrectionError::ReplayFailed => {
                PhysicsAuthorityFault::CorrectionReplayFailed
            }
        })?;
    candidate_ticker.apply_correction_plan(&plan)?;
    *physics = candidate_physics;
    *ticker = candidate_ticker;
    Ok(plan.outcome)
}
