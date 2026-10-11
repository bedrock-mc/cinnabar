//! Bevy owner for tick-driven mining.
use bevy::prelude::Resource;

/// Bevy resource adapter for the gameplay survival_mining owner.
#[derive(Resource, Debug, Default)]
pub struct SurvivalMiningRuntime(gameplay::survival_mining::SurvivalMiningRuntime);
impl std::ops::Deref for SurvivalMiningRuntime {
    type Target = gameplay::survival_mining::SurvivalMiningRuntime;
    /// Borrows the gameplay owner at the existing ordered system boundary.
    fn deref(&self) -> &Self::Target {
        &self.0
    }
}
impl std::ops::DerefMut for SurvivalMiningRuntime {
    /// Mutates the gameplay owner without duplicating its state.
    fn deref_mut(&mut self) -> &mut Self::Target {
        &mut self.0
    }
}
impl SurvivalMiningRuntime {
    /// Supplies existing audio and particle adapters with the current destroy target.
    pub fn destroying_target(&self) -> Option<([i32; 3], u8)> {
        self.0.destroying_target()
    }
}
