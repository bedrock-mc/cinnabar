//! Mining components bound to the session's runtime block identities.

impl super::PhysicsCollisionRegistries {
    /// Borrows the resolved mining component and tool mask independently of session artwork.
    pub fn custom_block_mining(
        &self,
        mode: assets::NetworkIdMode,
        runtime_id: u32,
    ) -> Option<(&protocol::CustomBlockMining, u8)> {
        let entries = match mode {
            assets::NetworkIdMode::Sequential => &self.interaction_blocks,
            assets::NetworkIdMode::Hashed => &self.hashed_interaction_blocks,
        };
        let (definition, state) = entries.get(&runtime_id)?.custom_state.as_ref()?;
        let mining = definition.mining_for_state(*state)?;
        let tools = assets::facts::block::custom_block_effective_tools(
            definition.tags.iter().map(AsRef::as_ref),
        );
        Some((mining, tools))
    }
}
