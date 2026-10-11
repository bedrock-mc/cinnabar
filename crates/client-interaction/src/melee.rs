//! Bevy owners for attack admission and arm swings.
use bevy::prelude::Resource;

/// Bevy resource adapter for the gameplay melee owner.
#[derive(Resource, Debug, Default, Clone)]
pub struct SwingTracker(gameplay::melee::SwingTracker);
impl std::ops::Deref for SwingTracker {
    type Target = gameplay::melee::SwingTracker;
    /// Borrows the gameplay owner at the existing ordered system boundary.
    fn deref(&self) -> &Self::Target {
        &self.0
    }
}
impl std::ops::DerefMut for SwingTracker {
    /// Mutates the gameplay owner without duplicating its state.
    fn deref_mut(&mut self) -> &mut Self::Target {
        &mut self.0
    }
}
/// Bevy resource adapter for the gameplay melee owner.
#[derive(Resource, Debug, Default, Clone)]
pub struct MeleeRuntime(gameplay::melee::MeleeRuntime);
impl std::ops::Deref for MeleeRuntime {
    type Target = gameplay::melee::MeleeRuntime;
    /// Borrows the gameplay owner at the existing ordered system boundary.
    fn deref(&self) -> &Self::Target {
        &self.0
    }
}
impl std::ops::DerefMut for MeleeRuntime {
    /// Mutates the gameplay owner without duplicating its state.
    fn deref_mut(&mut self) -> &mut Self::Target {
        &mut self.0
    }
}
