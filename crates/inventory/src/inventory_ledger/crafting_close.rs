//! Native close-time return-to-player, then Drop of any remainder.
//!
//! Vanilla returns every return-on-close input and the cursor to the player,
//! dropping what does not fit, then flushes those requests and the close together.

use protocol::StackRequestAction;

use super::cells::Cell;
use super::gesture::{StackRequestActionKind, Submission, counted_merge, counted_transfer};
use super::helpers::request_slot;
use super::overlay::DeltaGroup;
use super::registry::{OccupiedStackRelation, entry_capacity};
use super::{InventoryGestureError, InventoryPendingState, PlayerInventoryLedger};

impl PlayerInventoryLedger {
    /// Enumerates the active returnable inputs followed by the shared cursor.
    fn close_return_cells(&self) -> impl Iterator<Item = Cell> {
        screen_input_cells(self.window_kind()).chain(std::iter::once(Cell::Cursor))
    }

    /// Build every return against one staged ledger, so an identity, pressure,
    /// or capacity failure cannot publish a partial cleanup or lose an input.
    pub(super) fn return_crafting_on_close(&mut self) -> Result<bool, InventoryGestureError> {
        let cells: Vec<Cell> = self
            .close_return_cells()
            .filter(|cell| self.view().get(*cell).is_some())
            .collect();
        if cells.is_empty() {
            // A consume/transfer can predict an empty grid while its backing
            // still holds ingredients. Do not cancel that request on close.
            return Ok(self.close_return_needed());
        }
        if self.known[..self.confirmed.player_len()]
            .iter()
            .any(|known| !known)
        {
            return Err(InventoryGestureError::ResyncRequired);
        }
        let mut staged = self.clone();
        for cell in cells {
            staged.return_close_cell(cell)?;
        }
        *self = staged;
        Ok(true)
    }

    /// Returns `source` to the player, merging first and dropping what does not fit.
    /// Large stacks use several encodable requests; returns the last request's id.
    pub(super) fn return_close_cell(&mut self, source: Cell) -> Result<i32, InventoryGestureError> {
        let mut last = self.return_close_chunk(source)?;
        while self.view().get(source).is_some() {
            last = self.return_close_chunk(source)?;
        }
        Ok(last)
    }

    /// Returns at most one action's quantity so every move or drop stays encodable.
    fn return_close_chunk(&mut self, source: Cell) -> Result<i32, InventoryGestureError> {
        let personal_generation = self.gesture_preflight(true)?;
        self.check_surfaces([source])?;
        let from = self
            .named(self.view().get(source).cloned())?
            .ok_or(InventoryGestureError::EmptyGesture)?;
        let address = self.window_address();
        let mut remaining = from
            .stack
            .count
            .min(u16::from(protocol::MAX_STACK_REQUEST_AMOUNT));
        let mut actions = Vec::new();
        let mut groups = Vec::new();
        let (mut distinct, mut registry_bound) = (false, false);
        let candidates: Vec<Cell> = (0..self.confirmed.player_len())
            .filter(|slot| self.known[*slot])
            .map(|slot| Cell::Inventory(slot as u8))
            .collect();
        for cell in &candidates {
            if remaining == 0 {
                break;
            }
            let Some(into) = self.view().get(*cell) else {
                continue;
            };
            if self.awaiting_identity(into) {
                continue;
            }
            let OccupiedStackRelation::Compatible { capacity } =
                self.occupied_stack_relation(&from, into)
            else {
                continue;
            };
            let amount = remaining.min(capacity.saturating_sub(into.stack.count));
            if amount == 0 {
                continue;
            }
            let built = counted_merge(
                StackRequestActionKind::Place,
                source,
                *cell,
                &from.stack,
                &into.stack,
                address,
                amount,
                capacity,
            )?;
            remaining -= amount;
            distinct |= built.requires_distinct_stack_ids;
            registry_bound |= built.registry_bound_merge;
            actions.push(built.action);
            groups.push(built.group);
        }
        let capacity = self
            .negotiated_item_entry(from.stack.network_id)
            .and_then(entry_capacity)
            .map(u16::from);
        for cell in candidates {
            if remaining == 0 {
                break;
            }
            if self.view().get(cell).is_some() {
                continue;
            }
            let amount = remaining.min(capacity.unwrap_or(remaining));
            let built = counted_transfer(
                StackRequestActionKind::Place,
                source,
                cell,
                &from.stack,
                address,
                Some(amount),
            )?;
            remaining -= amount;
            distinct |= built.requires_distinct_stack_ids;
            actions.push(built.action);
            groups.push(built.group);
        }
        if remaining > 0 {
            actions.push(StackRequestAction::Drop {
                amount: u8::try_from(remaining)
                    .map_err(|_| InventoryGestureError::InvalidRequest)?,
                source: request_slot(source, from.stack.stack_network_id, address)?,
                randomly: false,
            });
            groups.push(DeltaGroup::Shrink {
                source,
                amount: remaining,
                source_id: from.stack.stack_network_id,
            });
        }
        self.submit(Submission {
            actions,
            groups,
            personal_generation,
            requires_distinct_stack_ids: distinct,
            registry_bound_merge: registry_bound,
        })
    }

    /// A close follows every gesture still waiting for transport, so the server
    /// applies the returns first; it never waits for their answers.
    pub(super) fn close_ready(&self) -> bool {
        !self
            .gestures()
            .any(|request| request.state == InventoryPendingState::AwaitingTransport)
    }

    pub(super) fn close_return_needed(&self) -> bool {
        self.close_return_cells().any(|cell| {
            self.view().get(cell).is_some()
                || self.queue.iter().any(|request| request.touches(cell))
        })
    }

    pub(super) fn note_close_return_failure(&self, error: InventoryGestureError) {
        tracing::warn!(target: "cinnabar::inventory_requests",
            ?error, "inventory closed without returning its inputs");
    }
}

/// Enumerates only the temporary inputs belonging to the selected screen.
pub(super) fn screen_input_cells(kind: Option<protocol::WindowKind>) -> impl Iterator<Item = Cell> {
    let grid = match kind {
        None => Some(super::crafting::CraftingGrid::Personal),
        Some(protocol::WindowKind::Workbench) => Some(super::crafting::CraftingGrid::Workbench),
        _ => None,
    };
    let inputs = kind.map_or(&[][..], super::screen_actions::input_slots);
    grid.into_iter()
        .flat_map(super::crafting::CraftingGrid::slots)
        .chain(inputs.iter().copied())
        .map(Cell::Craft)
}
