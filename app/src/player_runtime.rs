//! App resource adapter for the shared, engine-independent player state.

use bevy::prelude::Resource;

/// Installs the same player authority for network, UI and movement systems.
#[derive(Clone, Debug, Resource)]
pub struct PlayerRuntime(player_state::PlayerState);

impl PlayerRuntime {
    /// Starts both domain owners at the same session generation.
    pub fn new(session: u64) -> Self {
        Self(player_state::PlayerState::new(session))
    }
}

impl std::ops::Deref for PlayerRuntime {
    type Target = player_state::PlayerState;
    /// Borrows the shared authority without creating another owner.
    fn deref(&self) -> &Self::Target {
        &self.0
    }
}

impl std::ops::DerefMut for PlayerRuntime {
    /// Mutates the shared authority at the caller's existing ordered system boundary.
    fn deref_mut(&mut self) -> &mut Self::Target {
        &mut self.0
    }
}
