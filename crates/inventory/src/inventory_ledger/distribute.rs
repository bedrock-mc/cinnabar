//! Drag-distribute over several cells and double-click gather into the cursor.

use protocol::WindowKind;

use super::cells::Cell;
use super::gesture::{
    InventoryTarget, StackRequestActionKind, Submission, counted_merge, counted_transfer,
    has_meaningful_overlay,
};
use super::registry::{OccupiedStackRelation, entry_capacity};
use super::{InventoryGestureError, PlayerInventoryLedger};

/// How a drag splits the cursor stack.
#[derive(Debug, Clone, Copy, Eq, PartialEq)]
pub enum DistributeMode {
    /// Primary drag: an even share per cell, the remainder stays held.
    Even,
    /// Secondary drag: one item per cell.
    One,
}

impl PlayerInventoryLedger {
    /// Spreads the cursor stack over the dragged cells that can take it, in
    /// drag order, as one request.
    pub fn begin_distribute(
        &mut self,
        targets: &[InventoryTarget],
        mode: DistributeMode,
    ) -> Result<i32, InventoryGestureError> {
        let personal_generation = self.gesture_preflight(
            targets
                .iter()
                .any(|t| !matches!(t, InventoryTarget::Storage(_))),
        )?;
        self.check_surfaces([Cell::Cursor])?;
        let cursor = self
            .named(self.view().get(Cell::Cursor).cloned())?
            .ok_or(InventoryGestureError::EmptyGesture)?;
        let address = self.window_address();
        let capacity = self
            .negotiated_item_entry(cursor.stack.network_id)
            .and_then(entry_capacity)
            .map(u16::from);
        let mut cells: Vec<Cell> = Vec::new();
        for target in targets {
            let cell = target.cell();
            if matches!(cell, Cell::Armor(_))
                || cells.contains(&cell)
                || self.check_target(cell).is_err()
                || self.check_surfaces([cell]).is_err()
                || self.view().get(cell).is_some_and(|held| {
                    self.awaiting_identity(held) || has_meaningful_overlay(held.overlay.as_ref())
                })
            {
                continue;
            }
            cells.push(cell);
        }
        // A cell takes the stack when it is empty or holds a compatible stack with room.
        let room = |cell: Cell| -> Option<(u16, Option<u16>)> {
            match self.view().get(cell) {
                None => Some((capacity.unwrap_or(cursor.stack.count), None)),
                Some(into) => {
                    let OccupiedStackRelation::Compatible { capacity } =
                        self.occupied_stack_relation(&cursor.stack, &into.stack)
                    else {
                        return None;
                    };
                    let free = capacity.saturating_sub(into.stack.count);
                    (free > 0).then_some((free, Some(capacity)))
                }
            }
        };
        let eligible: Vec<(Cell, u16, Option<u16>)> = cells
            .iter()
            .filter_map(|cell| room(*cell).map(|(free, cap)| (*cell, free, cap)))
            .collect();
        if eligible.len() < 2 {
            return Err(InventoryGestureError::InvalidRequest);
        }
        let share = match mode {
            DistributeMode::Even => {
                (cursor.stack.count / u16::try_from(eligible.len()).unwrap_or(u16::MAX)).max(1)
            }
            DistributeMode::One => 1,
        };
        let mut remaining = cursor.stack.count;
        let mut actions = Vec::new();
        let mut groups = Vec::new();
        let (mut distinct, mut registry_bound) = (false, false);
        for (cell, free, merge_capacity) in eligible {
            if remaining == 0 {
                break;
            }
            let amount = share.min(remaining).min(free);
            let built = match merge_capacity {
                None => counted_transfer(
                    StackRequestActionKind::Place,
                    Cell::Cursor,
                    cell,
                    &cursor.stack,
                    address,
                    Some(amount),
                )?,
                Some(capacity) => {
                    let into = self.view().get(cell).expect("occupied by the room check");
                    counted_merge(
                        StackRequestActionKind::Place,
                        Cell::Cursor,
                        cell,
                        &cursor.stack,
                        &into.stack,
                        address,
                        amount,
                        capacity,
                    )?
                }
            };
            remaining -= amount;
            distinct |= built.requires_distinct_stack_ids;
            registry_bound |= built.registry_bound_merge;
            actions.push(built.action);
            groups.push(built.group);
        }
        self.submit(Submission {
            actions,
            groups,
            personal_generation,
            requires_distinct_stack_ids: distinct,
            registry_bound_merge: registry_bound,
        })
    }

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
