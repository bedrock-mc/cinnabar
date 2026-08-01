use bevy::prelude::Resource;
use sim::SimulationError;

use super::MovementSource;

#[derive(Resource, Debug, Clone, Copy, Default, PartialEq, Eq)]
pub enum PhysicsAuthorityGate {
    #[default]
    ProductionDisabled,
    CandidateEvidence,
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
    PhysicsTickOverflow {
        due: u64,
        dropped: u64,
    },
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
}
