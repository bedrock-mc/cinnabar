//! Mining components bound to the session's runtime block identities.

impl super::PhysicsCollisionRegistries {
    /// Returns the effective mining component of an admitted custom palette state.
    pub fn custom_block_mining(
        &self,
        mode: assets::NetworkIdMode,
        runtime_id: u32,
    ) -> Option<std::sync::Arc<protocol::CustomBlockMining>> {
        let entries = match mode {
            assets::NetworkIdMode::Sequential => &self.interaction_blocks,
            assets::NetworkIdMode::Hashed => &self.hashed_interaction_blocks,
        };
        let (definition, state) = entries.get(&runtime_id)?.custom_state.as_ref()?;
        let mining = definition
            .state_physics
            .get(*state as usize)
            .map_or(&definition.mining, |physics| &physics.mining);
        Some(std::sync::Arc::clone(mining))
    }
}
