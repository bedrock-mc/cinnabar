//! The inventory facts recipe-book availability depends on, for caching listings.

use std::sync::Arc;

use super::super::cells::Cell;
use super::super::{PLAYER_INVENTORY_SLOT_COUNT, PlayerInventoryLedger};

/// What every inventory cell holds, as recipe-book availability reads it. Together with the
/// item registry this decides each book recipe's availability. Comparing it validates nothing.
#[derive(Debug)]
pub(crate) struct AutoCraftMaterials([Option<Source>; PLAYER_INVENTORY_SLOT_COUNT]);

#[derive(Debug)]
struct Source {
    network_id: i32,
    metadata: u32,
    count: u16,
    /// Compared by identity first, then by content, so restated stacks keep the listing.
    extra_data: Arc<[u8]>,
    awaiting_identity: bool,
}

impl PartialEq for Source {
    fn eq(&self, other: &Self) -> bool {
        self.network_id == other.network_id
            && self.metadata == other.metadata
            && self.count == other.count
            && (Arc::ptr_eq(&self.extra_data, &other.extra_data)
                || self.extra_data == other.extra_data)
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
    /// inventory cells, captured without validating source bytes.
    pub(crate) fn auto_craft_materials(&self) -> AutoCraftMaterials {
        AutoCraftMaterials(std::array::from_fn(|slot| {
            let held = self.view().get(Cell::Inventory(slot as u8))?;
            Some(Source {
                network_id: held.stack.network_id,
                metadata: held.stack.metadata,
                count: held.stack.count,
                extra_data: Arc::clone(&held.stack.extra_data),
                awaiting_identity: self.awaiting_identity(held),
            })
        }))
    }
}
