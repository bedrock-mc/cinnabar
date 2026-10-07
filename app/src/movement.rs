//! Bevy resource and system adapters for the gameplay crate.
use bevy::prelude::Resource;
#[cfg(test)]
pub(crate) use gameplay::movement::flush_player_auth_inputs;
pub use gameplay::movement::{
    CorrectionKind, CorrectionShape, InteractionPacketGuard, LocalPhysicsFrame,
    MAX_LOCAL_PHYSICS_TICKS_PER_FRAME, MiningEffects, ModeIntent, MovementOutboxReconciliation,
    MovementSendError, MovementSource, OUTBOX_CAPACITY, PhysicsAnchor, PhysicsAuthorityFault,
    PhysicsCollisionRegistryError, PhysicsCorrectionMode, PhysicsCorrectionOutcome,
    PhysicsMovementSample, PhysicsSampleContext, PhysicsSendIdentity, PhysicsTickEvidence,
    PhysicsTickEvidenceContext, ProcessedMovementState, RideKind, ServerTeleportKind,
    flush_player_auth_inputs_guarded, local_facts, note_click_drop, note_correction, note_motion,
    pending_trace_line, physics_movement_input, reconcile_candidate_physics_correction,
    reconcile_committed_correction, reconcile_move_player_teleport, reconcile_physics_anchor,
    reconcile_prediction_correction, reconcile_timeline_rewind, reset_start_game_prediction, trace,
    trace_local_attributes, trace_server_control, write_trace_line,
};
mod prediction_sync;
mod runtime_system;
pub(crate) use prediction_sync::send_movement_prediction_sync;
pub(crate) use runtime_system::advance_local_physics;

/// Passes application-owned diagnostic names before gameplay resources are created.
fn configure_diagnostics() {
    gameplay::movement::diagnostics_config::configure(
        gameplay::movement::diagnostics_config::MovementDiagnostics {
            movement_trace: diagnostics::markers::MOVEMENT_TRACE,
            teleport_ack: diagnostics::markers::TELEPORT_ACK,
            anchor_probe: diagnostics::markers::ANCHOR_PROBE,
        },
    );
}

/// Bevy resource adapter for the engine-independent MovementTicker owner.
#[derive(Resource, Debug, Clone)]
pub struct MovementTicker(gameplay::movement::MovementTicker);
impl std::ops::Deref for MovementTicker {
    type Target = gameplay::movement::MovementTicker;
    /// Borrows the domain owner at the existing system boundary.
    fn deref(&self) -> &Self::Target {
        &self.0
    }
}
impl std::ops::DerefMut for MovementTicker {
    /// Mutates the single domain owner at the existing system boundary.
    fn deref_mut(&mut self) -> &mut Self::Target {
        &mut self.0
    }
}
/// Bevy resource adapter for the engine-independent LocalPhysicsController owner.
#[derive(Resource, Debug, Clone)]
pub struct LocalPhysicsController(gameplay::movement::LocalPhysicsController);
impl std::ops::Deref for LocalPhysicsController {
    type Target = gameplay::movement::LocalPhysicsController;
    /// Borrows the domain owner at the existing system boundary.
    fn deref(&self) -> &Self::Target {
        &self.0
    }
}
impl std::ops::DerefMut for LocalPhysicsController {
    /// Mutates the single domain owner at the existing system boundary.
    fn deref_mut(&mut self) -> &mut Self::Target {
        &mut self.0
    }
}
/// Bevy resource adapter for the engine-independent LocalMovementEffectTimeline owner.
#[derive(Resource, Debug, Default)]
pub struct LocalMovementEffectTimeline(gameplay::movement::LocalMovementEffectTimeline);
impl std::ops::Deref for LocalMovementEffectTimeline {
    type Target = gameplay::movement::LocalMovementEffectTimeline;
    /// Borrows the domain owner at the existing system boundary.
    fn deref(&self) -> &Self::Target {
        &self.0
    }
}
impl std::ops::DerefMut for LocalMovementEffectTimeline {
    /// Mutates the single domain owner at the existing system boundary.
    fn deref_mut(&mut self) -> &mut Self::Target {
        &mut self.0
    }
}
/// Bevy resource adapter for the engine-independent LocalMovementSpeedAuthority owner.
#[derive(Resource, Debug, Default)]
pub struct LocalMovementSpeedAuthority(gameplay::movement::LocalMovementSpeedAuthority);
impl std::ops::Deref for LocalMovementSpeedAuthority {
    type Target = gameplay::movement::LocalMovementSpeedAuthority;
    /// Borrows the domain owner at the existing system boundary.
    fn deref(&self) -> &Self::Target {
        &self.0
    }
}
impl std::ops::DerefMut for LocalMovementSpeedAuthority {
    /// Mutates the single domain owner at the existing system boundary.
    fn deref_mut(&mut self) -> &mut Self::Target {
        &mut self.0
    }
}
/// Bevy resource adapter for the engine-independent PhysicsCollisionRegistries owner.
#[derive(Resource, Debug)]
pub struct PhysicsCollisionRegistries(gameplay::movement::PhysicsCollisionRegistries);
impl std::ops::Deref for PhysicsCollisionRegistries {
    type Target = gameplay::movement::PhysicsCollisionRegistries;
    /// Borrows the domain owner at the existing system boundary.
    fn deref(&self) -> &Self::Target {
        &self.0
    }
}
impl std::ops::DerefMut for PhysicsCollisionRegistries {
    /// Mutates the single domain owner at the existing system boundary.
    fn deref_mut(&mut self) -> &mut Self::Target {
        &mut self.0
    }
}
/// Bevy resource adapter for the engine-independent PhysicsAuthorityGate owner.
#[derive(Resource, Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct PhysicsAuthorityGate(gameplay::movement::PhysicsAuthorityGate);
impl std::ops::Deref for PhysicsAuthorityGate {
    type Target = gameplay::movement::PhysicsAuthorityGate;
    /// Borrows the domain owner at the existing system boundary.
    fn deref(&self) -> &Self::Target {
        &self.0
    }
}
impl std::ops::DerefMut for PhysicsAuthorityGate {
    /// Mutates the single domain owner at the existing system boundary.
    fn deref_mut(&mut self) -> &mut Self::Target {
        &mut self.0
    }
}
impl MovementTicker {
    /// Binds prediction to the transport-owned position-authority publisher.
    pub(crate) fn with_epoch_publisher(publisher: tokio::sync::watch::Sender<u64>) -> Self {
        configure_diagnostics();
        Self(gameplay::movement::MovementTicker::with_epoch_publisher(
            publisher,
        ))
    }
}
#[cfg(test)]
impl Default for MovementTicker {
    /// Constructs an isolated publisher for app integration tests.
    fn default() -> Self {
        Self::with_epoch_publisher(tokio::sync::watch::channel(0).0)
    }
}
impl Default for LocalPhysicsController {
    /// Installs diagnostics before creating the local prediction controller.
    fn default() -> Self {
        configure_diagnostics();
        Self(gameplay::movement::LocalPhysicsController::default())
    }
}
impl LocalPhysicsController {
    /// Projects the latest stance for presentation without exposing prediction storage.
    pub fn latest_sneak_sprint(&self) -> Option<(bool, bool)> {
        self.0.latest_sneak_sprint()
    }
}
#[allow(non_upper_case_globals)]
impl PhysicsAuthorityGate {
    pub const ProductionDisabled: Self =
        Self(gameplay::movement::PhysicsAuthorityGate::ProductionDisabled);
    pub const CandidateEvidence: Self =
        Self(gameplay::movement::PhysicsAuthorityGate::CandidateEvidence);
    pub const ProductionEnabled: Self =
        Self(gameplay::movement::PhysicsAuthorityGate::ProductionEnabled);
}
impl PhysicsCollisionRegistries {
    /// Validates coherent startup carriers before installing them as a Bevy resource.
    pub fn bind_coherent_assets(
        breg: &[u8],
        preg: &[u8],
        preg_path: &std::path::Path,
        world_path: &std::path::Path,
        protocol: u32,
    ) -> Result<Self, gameplay::movement::PhysicsCollisionRegistryError> {
        gameplay::movement::PhysicsCollisionRegistries::bind_coherent_assets(
            breg, preg, preg_path, world_path, protocol,
        )
        .map(Self)
    }
    /// Binds decoded registry records to the supplied physics carrier.
    pub fn from_assets(
        breg: &[u8],
        records: &[assets::RegistryRecord],
        preg: &[u8],
        protocol: u32,
    ) -> Result<Self, gameplay::movement::PhysicsCollisionRegistryError> {
        gameplay::movement::PhysicsCollisionRegistries::from_assets(breg, records, preg, protocol)
            .map(Self)
    }
}

#[cfg(test)]
mod integration_tests;
#[cfg(test)]
#[path = "movement/local_facts/owner_tests.rs"]
mod local_facts_owner_tests;
#[cfg(test)]
mod teleport_ack_wiring_tests;

mod world_view;
pub(crate) use world_view::GameplayWorldView;

mod fault_observation;
pub use fault_observation::PhysicsAuthorityFaultRecord;
