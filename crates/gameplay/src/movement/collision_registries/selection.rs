//! Runtime selection presentation rules for compiled block facts.

impl super::PhysicsCollisionRegistries {
    /// Whether this held block exposes native light blocks to interaction rays.
    pub fn is_light_block(&self, mode: assets::NetworkIdMode, runtime_id: u32) -> bool {
        let entries = match mode {
            assets::NetworkIdMode::Sequential => &self.interaction_blocks,
            assets::NetworkIdMode::Hashed => &self.hashed_interaction_blocks,
        };
        entries.get(&runtime_id).is_some_and(|block| {
            block
                .shape_flags
                .contains(assets::block_shapes::ShapeFlags::LIGHT_BLOCK)
        })
    }

    /// Native gateways hide their overlay; barriers expose it only in Creative.
    pub fn selection_overlay_visible(
        &self,
        mode: assets::NetworkIdMode,
        runtime_id: u32,
        game_mode: Option<protocol::PlayerGameMode>,
    ) -> bool {
        let entries = match mode {
            assets::NetworkIdMode::Sequential => &self.interaction_blocks,
            assets::NetworkIdMode::Hashed => &self.hashed_interaction_blocks,
        };
        let flags = entries
            .get(&runtime_id)
            .map_or(assets::block_shapes::ShapeFlags::empty(), |block| {
                block.shape_flags
            });
        !flags.contains(assets::block_shapes::ShapeFlags::HIDE_OVERLAY)
            && (!flags.contains(assets::block_shapes::ShapeFlags::CREATIVE_OVERLAY)
                || game_mode == Some(protocol::PlayerGameMode::Creative))
    }
}

#[cfg(test)]
mod coverage_tests;
#[cfg(test)]
mod tests;
