use std::sync::{Arc, OnceLock};

use assets::{ModelFamily, NetworkIdMode, RegistryRecord};

use super::PhysicsCollisionRegistries;

/// Door, gate and trapdoor types share the native one-way collision tag.
pub(super) fn native_block_tags(record: &RegistryRecord) -> Arc<[Arc<str>]> {
    static ONE_WAY: OnceLock<Arc<[Arc<str>]>> = OnceLock::new();
    match record.model_family {
        ModelFamily::Door | ModelFamily::Gate | ModelFamily::Trapdoor => {
            Arc::clone(ONE_WAY.get_or_init(|| Arc::from([Arc::from("one_way_collidable")])))
        }
        _ => Arc::default(),
    }
}

/// Admitted families and these specialized blocks retain their cube placement intention.
pub(super) fn has_build_intention(record: &RegistryRecord) -> bool {
    matches!(
        record.model_family,
        ModelFamily::Cube
            | ModelFamily::Leaves
            | ModelFamily::Stair
            | ModelFamily::Slab
            | ModelFamily::Pane
            | ModelFamily::Fence
            | ModelFamily::Wall
            | ModelFamily::Carpet
    ) || matches!(
        record.name.as_ref(),
        "minecraft:soul_sand"
            | "minecraft:mud"
            | "minecraft:barrier"
            | "minecraft:chiseled_bookshelf"
    )
}

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
