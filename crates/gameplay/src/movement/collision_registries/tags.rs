use super::PhysicsCollisionRegistries;
use assets::NetworkIdMode;
use std::sync::Arc;

impl PhysicsCollisionRegistries {
    /// Whether this admitted block family can retain a held placement direction.
    #[must_use]
    pub fn block_has_build_intention(&self, mode: NetworkIdMode, runtime_id: u32) -> bool {
        let entries = match mode {
            NetworkIdMode::Sequential => &self.interaction_blocks,
            NetworkIdMode::Hashed => &self.hashed_interaction_blocks,
        };
        entries
            .get(&runtime_id)
            .is_some_and(|block| block.build_intention)
    }

    /// Borrows native and server-declared tags in the active runtime-ID space.
    #[must_use]
    pub fn block_tags(&self, mode: NetworkIdMode, runtime_id: u32) -> &[Arc<str>] {
        let entries = match mode {
            NetworkIdMode::Sequential => &self.interaction_blocks,
            NetworkIdMode::Hashed => &self.hashed_interaction_blocks,
        };
        entries
            .get(&runtime_id)
            .map_or(&[], |block| block.tags.as_ref())
    }
}
