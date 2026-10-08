//! Drag-distribute over several cells and double-click gather into the cursor.

use protocol::WindowKind;

use super::cells::Cell;
use super::gesture::{
    InventoryTarget, StackRequestActionKind, Submission, counted_merge, counted_transfer,
    has_meaningful_overlay,
};
use super::registry::{OccupiedStackRelation, entry_capacity};
use super::{InventoryGestureError, PlayerInventoryLedger};

mod live;
pub use live::{DragDistribution, MAX_DISTRIBUTION_CELLS};

/// How a drag splits the cursor stack.
#[derive(Debug, Clone, Copy, Eq, PartialEq)]
pub enum DistributeMode {
    /// Primary drag: an even share per cell, the remainder stays held.
    Even,
    /// Secondary drag: one item per cell.
    One,
}

impl PlayerInventoryLedger {
    /// Pulls every compatible stack in the open screen into the cursor stack,
    /// partial stacks first, until the cursor is full.
    pub fn begin_gather(&mut self) -> Result<i32, InventoryGestureError> {
        let personal_generation = self.gesture_preflight(true)?;
        self.check_surfaces([Cell::Cursor])?;
        let mut cursor = self
            .named(self.view().get(Cell::Cursor).cloned())?
            .ok_or(InventoryGestureError::EmptyGesture)?
            .stack;
        let address = self.window_address();
        let capacity = self
            .negotiated_item_entry(cursor.network_id)
            .and_then(entry_capacity)
            .map(u16::from)
            .ok_or(InventoryGestureError::InvalidRequest)?;
        let cells = self.gather_cells();
        let mut actions = Vec::new();
        let mut groups = Vec::new();
        let (mut distinct, mut registry_bound) = (false, false);
        for pass in 0..2 {
            for cell in &cells {
                if cursor.count >= capacity {
                    break;
                }
                let Some(held) = self.view().get(*cell) else {
                    continue;
                };
                if self.awaiting_identity(held) || has_meaningful_overlay(held.overlay.as_ref()) {
                    continue;
                }
                let OccupiedStackRelation::Compatible { capacity: cap } =
                    self.occupied_stack_relation(&held.stack, &cursor)
                else {
                    continue;
                };
                // Partial stacks go first; full ones only top up what is left.
                if (held.stack.count >= cap) != (pass == 1) {
                    continue;
                }
                let amount = held.stack.count.min(cap.saturating_sub(cursor.count));
                if amount == 0 {
                    continue;
                }
                let built = counted_merge(
                    StackRequestActionKind::Take,
                    *cell,
                    Cell::Cursor,
                    &held.stack,
                    &cursor,
                    address,
                    amount,
                    cap,
                )?;
                cursor.count += amount;
                distinct |= built.requires_distinct_stack_ids;
                registry_bound |= built.registry_bound_merge;
                actions.push(built.action);
                groups.push(built.group);
            }
        }
        if actions.is_empty() {
            return Err(InventoryGestureError::InvalidRequest);
        }
        self.submit(Submission {
            actions,
            groups,
            personal_generation,
            requires_distinct_stack_ids: distinct,
            registry_bound_merge: registry_bound,
        })
    }

    /// Cells a gather may pull from, in screen order; outputs are excluded.
    fn gather_cells(&self) -> Vec<Cell> {
        let window = self
            .storage
            .as_ref()
            .filter(|storage| storage.identity.is_some());
        let output_cell = window.and_then(|storage| match storage.kind {
            WindowKind::Furnace | WindowKind::BlastFurnace | WindowKind::Smoker => Some(2),
            _ => None,
        });
        let mut cells: Vec<Cell> = (0..u8::MAX)
            .map(Cell::Storage)
            .take_while(|cell| self.confirmed.contains(*cell))
            .filter(|cell| *cell != Cell::Storage(output_cell.unwrap_or(u8::MAX)))
            .collect();
        cells.extend(
            (0..36u8)
                .filter(|slot| self.known[usize::from(*slot)])
                .map(Cell::Inventory),
        );
        cells
    }
}
