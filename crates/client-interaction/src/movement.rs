//! Bevy resources for movement authority and prediction.
use bevy::prelude::Resource;

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
    pub fn with_epoch_publisher(publisher: tokio::sync::watch::Sender<u64>) -> Self {
        configure_diagnostics();
        Self(gameplay::movement::MovementTicker::with_epoch_publisher(
            publisher,
        ))
    }
}
#[cfg(any(test, feature = "test-support"))]
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

pub mod fault_observation;
pub mod world_view;

#[cfg(test)]
mod local_facts_owner_tests;
