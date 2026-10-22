//! The inventory facts recipe-book availability depends on, for caching listings.

use std::sync::Arc;

use super::super::PlayerInventoryLedger;

/// What every recipe source cell holds, as recipe-book availability reads it. Together with the
/// item registry this decides each book recipe's availability. Comparing it validates nothing.
#[derive(Debug)]
pub(crate) struct AutoCraftMaterials(Vec<Option<Source>>);

#[derive(Debug)]
struct Source {
    network_id: i32,
    metadata: u32,
    block_runtime_id: i32,
    count: u16,
    /// Compared by identity first, then by content, so restated stacks keep the listing.
    extra_data: Arc<[u8]>,
    /// Source validity compares this digest with the bytes, so a digest-only update matters.
    nbt_digest: [u8; 32],
    awaiting_identity: bool,
}

impl PartialEq for Source {
    fn eq(&self, other: &Self) -> bool {
        self.network_id == other.network_id
            && self.metadata == other.metadata
            && self.block_runtime_id == other.block_runtime_id
            && self.count == other.count
            && (Arc::ptr_eq(&self.extra_data, &other.extra_data)
                || self.extra_data == other.extra_data)
            && self.nbt_digest == other.nbt_digest
            && self.awaiting_identity == other.awaiting_identity
    }
}

impl PartialEq for AutoCraftMaterials {
    fn eq(&self, other: &Self) -> bool {
        self.0 == other.0
    }
}

impl PlayerInventoryLedger {
    /// Every input [`Self::auto_craft_inputs`] and [`Self::holds_any_ingredient`] read from the
    /// grid and inventory cells, captured without validating source bytes.
    pub(crate) fn auto_craft_materials(&self) -> AutoCraftMaterials {
        AutoCraftMaterials(
            self.recipe_source_cells()
                .into_iter()
                .map(|cell| {
                    let held = self.view().get(cell)?;
                    Some(Source {
                        network_id: held.stack.network_id,
                        metadata: held.stack.metadata,
                        block_runtime_id: held.stack.block_runtime_id,
                        count: held.stack.count,
                        extra_data: Arc::clone(&held.stack.extra_data),
                        nbt_digest: held.stack.nbt_digest,
                        awaiting_identity: self.awaiting_identity(held),
                    })
                })
                .collect(),
        )
    }
}
