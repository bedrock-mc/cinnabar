//! The inventory facts recipe-book availability depends on, for caching listings.

use super::super::cells::Cell;
use super::super::{PLAYER_INVENTORY_SLOT_COUNT, PlayerInventoryLedger};

/// Per inventory cell: the held item, its metadata and count, and whether it can supply an
/// auto-craft. Together with the item registry this decides every book recipe's availability.
pub(crate) type AutoCraftMaterials = [Option<(i32, u32, u16, bool)>; PLAYER_INVENTORY_SLOT_COUNT];

impl PlayerInventoryLedger {
    /// What [`Self::can_auto_craft`] and [`Self::holds_any_ingredient`] read from the inventory.
    pub(crate) fn auto_craft_materials(&self) -> AutoCraftMaterials {
        std::array::from_fn(|slot| {
            let held = self.view().get(Cell::Inventory(slot as u8))?;
            Some((
                held.stack.network_id,
                held.stack.metadata,
                held.stack.count,
                self.auto_craft_stack(slot as u8).is_some(),
            ))
        })
    }
}
