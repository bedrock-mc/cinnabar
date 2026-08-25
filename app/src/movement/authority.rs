use bevy::prelude::Resource;
use sim::SimulationError;

use super::{LocalPhysicsController, MovementSource, MovementTicker};

#[derive(Resource, Debug, Clone, Copy, Default, PartialEq, Eq)]
pub enum PhysicsAuthorityGate {
    #[default]
    ProductionDisabled,
    CandidateEvidence,
    /// Normal gameplay authority after the collision registry has been
    /// validated. `ProductionDisabled` remains available for explicit
    /// free-camera and acceptance paths.
    ProductionEnabled,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum PhysicsAuthorityFault {
    Unauthorized,
    IncompleteCollisionRegistry,
    TickMismatch {
        expected: u64,
        actual: u64,
    },
    OutboxOverflow,
    InvalidCompletedSample,
    PhysicsSimulationError {
        due: u64,
        tick_index: usize,
        error: SimulationError,
    },
    CorrectionNotRetained {
        tick: u64,
    },
    CorrectionReplayFailed,
    ReplayWorldIdentityMismatch {
        tick: u64,
    },
    PendingWorldIdentityMismatch {
        tick: u64,
    },
    PendingTickMismatch {
        expected: u64,
        actual: u64,
    },
    PendingSessionMismatch {
        expected: u64,
        actual: u64,
    },
    IndeterminatePhysicsSend {
        tick: u64,
    },
}

/// One bounded record of a latched physics-authority fault, published through
/// [`MovementTicker::take_authority_fault`] for terminal evidence.
#[derive(Debug, Clone, PartialEq)]
pub struct PhysicsAuthorityFaultRecord {
    pub session_generation: u64,
    pub fault: PhysicsAuthorityFault,
    pub next_tick: u64,
    pub pending_count: usize,
}

/// Identity of one outbound physics transmission attempt, carried through the
/// staged-send, acknowledgement, cancellation, and restore paths.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) struct PhysicsSendIdentity {
    pub(crate) session_generation: u64,
    pub(crate) tick: u64,
    pub(crate) admission_id: u64,
    pub(crate) reanchor_epoch: u64,
}

impl PhysicsAuthorityGate {
    pub const fn authorize(
        self,
        auto_fly: bool,
        collision_registry_complete: bool,
    ) -> Result<MovementSource, PhysicsAuthorityFault> {
        if auto_fly || matches!(self, Self::ProductionDisabled) {
            return Ok(MovementSource::FreeCamera);
        }
        if !collision_registry_complete {
            return Err(PhysicsAuthorityFault::IncompleteCollisionRegistry);
        }
        Ok(MovementSource::Physics)
    }

    /// Installs the StartGame movement authority after the physics anchor is prepared.
    ///
    /// A free camera and local prediction are mutually exclusive: an active
    /// controller suppresses free-camera translation.
    pub(crate) fn apply_start_game(
        self,
        auto_fly: bool,
        collision_registry_complete: bool,
        movement: &mut MovementTicker,
        local_physics: &mut LocalPhysicsController,
    ) -> Result<MovementSource, PhysicsAuthorityFault> {
        let source = self.authorize(auto_fly, collision_registry_complete)?;
        movement.set_source(source);
        movement.enforce_local_physics_authority(local_physics);
        Ok(source)
    }
}
