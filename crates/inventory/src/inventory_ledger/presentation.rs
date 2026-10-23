//! Retained display inputs for detecting inventory changes without cell probes.

use std::sync::Arc;

use super::{Cells, CraftingGrid, PlayerInventoryLedger};
use protocol::NetworkItemStack;

/// Retains shared cells so subsequent writes detach their allocations.
#[derive(Clone, Debug)]
pub struct InventoryPresentationSnapshot {
    cells: Cells,
    session: u64,
    storage: (Option<u64>, Option<protocol::ContainerIdentity>),
    grid: CraftingGrid,
    registry: Option<Arc<std::collections::BTreeMap<i32, protocol::ItemRegistryEntry>>>,
    ghosts: [Option<NetworkItemStack>; 3],
    crafting_ghosts: Vec<Option<NetworkItemStack>>,
}

impl InventoryPresentationSnapshot {
    /// Checks display sources without traversing player, storage or UI cells.
    pub fn matches(&self, ledger: &PlayerInventoryLedger) -> bool {
        self.session == ledger.session_generation
            && self.storage == (ledger.storage_generation(), ledger.storage_identity())
            && self.grid == ledger.crafting_grid()
            && same(&self.registry, &ledger.item_registry)
            && self.cells.shares_presentation(ledger.view())
            && self
                .ghosts
                .iter()
                .enumerate()
                .all(|(slot, stack)| stack.as_ref() == ledger.furnace_ghost_stack(slot as u8))
            && self.crafting_ghosts == crafting_ghosts(ledger)
    }
}

impl PlayerInventoryLedger {
    /// Retains only the cells and identities that inventory projections consume.
    pub fn presentation_snapshot(&self) -> InventoryPresentationSnapshot {
        InventoryPresentationSnapshot {
            cells: self.view().clone(),
            session: self.session_generation,
            storage: (self.storage_generation(), self.storage_identity()),
            grid: self.crafting_grid(),
            registry: self.item_registry.clone(),
            ghosts: std::array::from_fn(|slot| self.furnace_ghost_stack(slot as u8).cloned()),
            crafting_ghosts: crafting_ghosts(self),
        }
    }
}

/// The grid's ghost cells followed by the ghost result.
fn crafting_ghosts(ledger: &PlayerInventoryLedger) -> Vec<Option<NetworkItemStack>> {
    ledger
        .crafting_grid()
        .slots()
        .map(|slot| ledger.crafting_ghost_stack(slot).cloned())
        .chain(std::iter::once(ledger.crafting_ghost_output().cloned()))
        .collect()
}

/// Compares retained optional allocation identities, including absence.
fn same<T>(left: &Option<Arc<T>>, right: &Option<Arc<T>>) -> bool {
    match (left, right) {
        (Some(left), Some(right)) => Arc::ptr_eq(left, right),
        (None, None) => true,
        _ => false,
    }
}

#[cfg(test)]
mod tests;
