//! Places one request's created results into cells: compatible stacks first, then empty
//! cells, tracking each cell's remaining room across results and keeping every transfer
//! within one action's amount.

use protocol::NetworkItemStack;

use super::super::PlayerInventoryLedger;
use super::super::cells::{Cell, Held};

/// `(cell, amount, occupied destination stack id and its count before this transfer)`.
pub(in crate::inventory_ledger) type OutputPlacement = (Cell, u16, Option<(i32, u16)>);

/// The most one transfer moves: a whole stack, bounded by what one action encodes.
pub(in crate::inventory_ledger) fn action_amount_limit(capacity: u16) -> u16 {
    capacity.min(u16::from(protocol::MAX_STACK_REQUEST_AMOUNT))
}

/// Splits `total` into the amounts of consecutive actions, each one action's encodable most.
pub(in crate::inventory_ledger) fn action_amounts(total: u16) -> impl Iterator<Item = u16> {
    let limit = action_amount_limit(u16::MAX);
    (0..total.div_ceil(limit)).map(move |index| (total - index * limit).min(limit))
}

/// One request's destinations: cells it must not use, and what it already merged elsewhere.
#[derive(Debug, Default)]
pub(in crate::inventory_ledger) struct OutputPlanner {
    /// Cells the request consumes from, or that one of its results filled from empty; a
    /// queued earlier request may still rename either, so neither takes more output.
    closed: Vec<Cell>,
    /// Amounts the request already merged into occupied cells.
    merged: Vec<(Cell, u16)>,
}

impl OutputPlanner {
    /// A planner that never uses `closed` cells.
    pub(in crate::inventory_ledger) fn new(closed: impl IntoIterator<Item = Cell>) -> Self {
        Self {
            closed: closed.into_iter().collect(),
            merged: Vec::new(),
        }
    }

    /// Plans `created` into `cells` in order, as they stood before the request plus what it
    /// already merged: compatible stacks first, then empty cells (only the first with
    /// `first_empty_only`). Each entry is one action's amount at most; the plan stops short
    /// when the cells cannot hold everything.
    pub(in crate::inventory_ledger) fn plan(
        &mut self,
        ledger: &PlayerInventoryLedger,
        created: &NetworkItemStack,
        capacity: u8,
        cells: &[Cell],
        first_empty_only: bool,
    ) -> Vec<OutputPlacement> {
        let capacity = u16::from(capacity);
        let limit = action_amount_limit(capacity);
        let mut remaining = created.count;
        let mut plan = Vec::new();
        for &cell in cells {
            if remaining == 0 {
                break;
            }
            if self.closed.contains(&cell) {
                continue;
            }
            let Some(into) = ledger.view().get(cell) else {
                continue;
            };
            if !ledger.merges_into(into, created) {
                continue;
            }
            let count = into.stack.count.saturating_add(self.merged_into(cell));
            let amount = remaining.min(capacity.saturating_sub(count)).min(limit);
            if amount > 0 {
                remaining -= amount;
                self.merged.push((cell, amount));
                plan.push((cell, amount, Some((into.stack.stack_network_id, count))));
            }
        }
        for &cell in cells {
            if remaining == 0 {
                break;
            }
            if self.closed.contains(&cell) || ledger.view().get(cell).is_some() {
                continue;
            }
            let amount = remaining.min(limit);
            remaining -= amount;
            self.closed.push(cell);
            plan.push((cell, amount, None));
            if first_empty_only {
                break;
            }
        }
        plan
    }

    fn merged_into(&self, cell: Cell) -> u16 {
        self.merged
            .iter()
            .filter(|(merged, _)| *merged == cell)
            .fold(0, |total, (_, amount)| total.saturating_add(*amount))
    }
}

impl PlayerInventoryLedger {
    /// Whether one player cell can take all of `created` before anything is consumed: an
    /// empty cell, or a compatible partial stack with room for the whole count.
    pub(in crate::inventory_ledger) fn player_cell_fits(
        &self,
        created: &NetworkItemStack,
        capacity: u8,
    ) -> bool {
        self.player_cells().into_iter().any(|cell| {
            self.view().get(cell).is_none_or(|into| {
                self.merges_into(into, created)
                    && into.stack.count.saturating_add(created.count) <= u16::from(capacity)
            })
        })
    }

    /// Known, confirmed player cells in hotbar-first order.
    pub(in crate::inventory_ledger) fn player_cells(&self) -> Vec<Cell> {
        (0..protocol::PLAYER_INVENTORY_SLOTS)
            .filter(|slot| {
                self.known[usize::from(*slot)] && self.confirmed.contains(Cell::Inventory(*slot))
            })
            .map(Cell::Inventory)
            .collect()
    }

    /// Whether `created` may merge into the settled stack `into`: the same item with equal
    /// item data.
    pub(in crate::inventory_ledger) fn merges_into(
        &self,
        into: &Held,
        created: &NetworkItemStack,
    ) -> bool {
        into.stack.network_id == created.network_id
            && into.stack.metadata == created.metadata
            && into.stack.block_runtime_id == created.block_runtime_id
            && Held::new(created).is_some_and(|created| {
                super::super::registry::held_item_data_equal(&created, into) == Some(true)
            })
            && !self.awaiting_identity(into)
    }
}
