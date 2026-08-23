//! Correction-shape classification for committed local-player corrections.
//!
//! Protocol 2168's `CorrectPlayerMovePrediction` carries no shape or mode
//! field, so Cinnabar derives the handling shape from observable field
//! combinations. Every threshold here is explicit client policy and is labeled
//! provisional until a version-matched native reference measures real
//! correction behavior; none of this claims a vanilla contract.

use protocol::PLAYER_NETWORK_OFFSET;
use sim::CollisionWorld;

use super::physics::LocalPhysicsController;
use super::{
    MovementTicker, PhysicsAuthorityFault, PhysicsCorrectionMode, PhysicsCorrectionOutcome,
    reconcile_candidate_physics_correction,
};

/// Largest per-tick displacement still treated as an ordinary reconcilable
/// correction.
///
/// One full chunk column (16 blocks) within a single 20 Hz tick exceeds every
/// vanilla Bedrock locomotion ceiling — terminal fall speed is roughly 3.9
/// blocks per tick and sprint jumping stays far below one block per tick — so a
/// larger server displacement cannot be reproduced by replaying retained inputs
/// and is handled through the existing teleport anchor path instead.
/// Provisional policy pending version-matched native measurement.
pub const CORRECTION_TELEPORT_DISPLACEMENT_BLOCKS: f32 = 16.0;

/// How one committed correction must be applied to prediction state.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CorrectionShape {
    /// The server position equals the current predicted network position
    /// exactly in sent `f32` network space and the ground flag agrees: the
    /// spatial record confirms the local prediction, so only rotation can
    /// carry new information. Velocity, history, overlays, and the outbound
    /// tick stream are left completely untouched.
    Confirmed,
    /// Ordinary small or full correction reconciled by replacing the retained
    /// position/ground at its tick and replaying later inputs. This is today's
    /// established behavior and also covers any correction that is not exactly
    /// confirming and stays within the teleport displacement bound.
    Replay,
    /// Displacement beyond [`CORRECTION_TELEPORT_DISPLACEMENT_BLOCKS`], or an
    /// unresolvable non-finite anchor: snap through the existing teleport
    /// anchor path, including its bounded state clearing and settle window.
    TeleportSnap,
}

impl LocalPhysicsController {
    /// Classifies one committed correction against current prediction state.
    ///
    /// Position agreement is exact — the same comparison already proven by the
    /// transport-confirmation rule in [`LocalPhysicsController::apply_correction`]
    /// — rather than a newly invented epsilon. Anything that does not compare
    /// exactly degrades to the ordinary replay path, so float jitter on live
    /// servers keeps working exactly as before.
    #[must_use]
    pub fn correction_shape(&self, network_position: [f32; 3], on_ground: bool) -> CorrectionShape {
        if !network_position.into_iter().all(f32::is_finite) {
            // Position resolution bounds non-finite input upstream, so this is
            // pure defense: an unresolvable anchor is rejected by the
            // controller's InvalidAnchor guard before any shape-specific path
            // runs, leaving prediction state untouched.
            return CorrectionShape::TeleportSnap;
        }
        let Some(state) = self.state() else {
            return CorrectionShape::Replay;
        };
        let current = [
            state.position.x as f32,
            state.position.y as f32 + PLAYER_NETWORK_OFFSET,
            state.position.z as f32,
        ];
        if current == network_position && state.on_ground == on_ground {
            return CorrectionShape::Confirmed;
        }
        let dx = current[0] - network_position[0];
        let dy = current[1] - network_position[1];
        let dz = current[2] - network_position[2];
        let bound = CORRECTION_TELEPORT_DISPLACEMENT_BLOCKS;
        if dx * dx + dy * dy + dz * dz > bound * bound {
            CorrectionShape::TeleportSnap
        } else {
            CorrectionShape::Replay
        }
    }
}

/// Applies one committed correction to prediction according to its shape.
///
/// `Ok(None)` means the correction confirmed the current prediction and
/// deliberately mutated nothing — no replay, no interpolation re-anchor, no
/// settle-window engagement. `Ok(Some(_))` reports the applied outcome for
/// evidence attribution.
pub(crate) fn reconcile_committed_correction(
    ticker: &mut MovementTicker,
    physics: &mut LocalPhysicsController,
    network_position: [f32; 3],
    correction_tick: u64,
    on_ground: bool,
    world: &impl CollisionWorld,
) -> Result<Option<PhysicsCorrectionOutcome>, PhysicsAuthorityFault> {
    let mode = match physics.correction_shape(network_position, on_ground) {
        CorrectionShape::Confirmed => return Ok(None),
        CorrectionShape::Replay => PhysicsCorrectionMode::ReplayIfRetained,
        CorrectionShape::TeleportSnap => PhysicsCorrectionMode::Snap,
    };
    reconcile_candidate_physics_correction(
        ticker,
        physics,
        network_position,
        correction_tick,
        on_ground,
        mode,
        world,
    )
    .map(Some)
}
