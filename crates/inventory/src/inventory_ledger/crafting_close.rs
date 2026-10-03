//! Native close-time return-to-player, then Drop of any remainder.
//!
//! ContainerManagerController::_closeContainers (current 038f37b0) invokes
//! _returnToPlayerOrDrop (038e9c40) for every return-on-close input and cursor.
//! Close transport waits for our retained sparse requests to settle; that
//! bounded admission policy is not a claim about native packet timing.

use protocol::StackRequestAction;

use super::cells::Cell;
use super::gesture::{
    StackRequestActionKind, Submission, counted_merge, counted_transfer, has_meaningful_overlay,
};
use super::helpers::request_slot;
use super::overlay::DeltaGroup;
use super::registry::{OccupiedStackRelation, entry_capacity};
use super::{
    InventoryGestureError, PLAYER_INVENTORY_SLOT_COUNT, PendingClose, PendingCloseOwner,
    PlayerInventoryLedger,
};

impl PlayerInventoryLedger {
    /// Build every return against one staged ledger, so an identity, pressure,
    /// or capacity failure cannot publish a partial cleanup or lose an input.
    pub(super) fn return_crafting_on_close(&mut self) -> Result<bool, InventoryGestureError> {
        let cells: Vec<Cell> = self
            .crafting_grid()
            .slots()
            .map(Cell::Craft)
            .chain(std::iter::once(Cell::Cursor))
            .filter(|cell| self.view().get(*cell).is_some())
            .collect();
        if cells.is_empty() {
            // A consume/transfer can predict an empty grid while its backing
            // still holds ingredients. Do not cancel that request on close.
            return Ok(self.close_return_needed());
        }
        if self.known.iter().any(|known| !known) {
            return Err(InventoryGestureError::ResyncRequired);
        }
        let mut staged = self.clone();
        for cell in cells {
            staged.return_close_cell(cell)?;
        }
        *self = staged;
        Ok(true)
    }

    fn return_close_cell(&mut self, source: Cell) -> Result<(), InventoryGestureError> {
        let personal_generation = self.gesture_preflight(true)?;
        self.check_surfaces([source])?;
        let from = self
            .named(self.view().get(source).cloned())?
            .ok_or(InventoryGestureError::EmptyGesture)?;
        let address = self.window_address();
        let mut remaining = from.stack.count;
        let mut actions = Vec::new();
        let mut groups = Vec::new();
        let (mut distinct, mut registry_bound) = (false, false);
        let candidates: Vec<Cell> = (0..PLAYER_INVENTORY_SLOT_COUNT)
            .map(|slot| Cell::Inventory(slot as u8))
            .collect();
        for cell in &candidates {
            if remaining == 0 {
                break;
            }
            let Some(into) = self.view().get(*cell) else {
                continue;
            };
            if self.awaiting_identity(into)
                || has_meaningful_overlay(from.overlay.as_ref())
                || has_meaningful_overlay(into.overlay.as_ref())
            {
                continue;
            }
            let OccupiedStackRelation::Compatible { capacity } =
                self.occupied_stack_relation(&from.stack, &into.stack)
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
        })?;
        Ok(())
    }

    pub(super) fn close_ready(&self, close: PendingClose) -> bool {
        if !close.returning_inputs {
            return true;
        }
        !self.close_requests_pending(close.owner)
            && self.confirmed.get(Cell::Cursor).is_none()
            && self
                .crafting_grid()
                .slots()
                .all(|slot| self.confirmed.get(Cell::Craft(slot)).is_none())
    }

    fn close_requests_pending(&self, owner: PendingCloseOwner) -> bool {
        self.queue.iter().any(|request| match owner {
            PendingCloseOwner::Personal(generation) => {
                request.personal_generation == Some(generation)
            }
            PendingCloseOwner::Storage => self
                .storage
                .as_ref()
                .is_some_and(|storage| request.storage_generation == Some(storage.generation)),
            PendingCloseOwner::Cleanup => false,
        })
    }

    pub(super) fn close_return_needed(&self) -> bool {
        self.view().get(Cell::Cursor).is_some()
            || self
                .crafting_grid()
                .slots()
                .any(|slot| self.view().get(Cell::Craft(slot)).is_some())
            || self.queue.iter().any(|request| {
                request
                    .touched()
                    .any(|cell| matches!(cell, Cell::Craft(_) | Cell::Cursor))
            })
    }

    /// Refused or incomplete return answers leave real backing inputs. Reopen
    /// the retained surface rather than sending Close and erasing those items.
    pub(super) fn reconcile_crafting_close(&mut self) {
        let failed: Vec<PendingClose> = self
            .pending_closes
            .iter()
            .copied()
            .filter(|close| {
                close.returning_inputs
                    && !self.close_requests_pending(close.owner)
                    && !self.close_ready(*close)
            })
            .collect();
        for close in failed {
            self.pending_closes
                .retain(|pending| pending.owner != close.owner);
            match close.owner {
                PendingCloseOwner::Personal(generation) => {
                    if let Some(super::PersonalWindow::Closing {
                        generation: current,
                        window_id,
                        window_type,
                        ..
                    }) = self.personal
                        && current == generation
                    {
                        self.personal = Some(super::PersonalWindow::Open {
                            generation,
                            window_id,
                            window_type,
                        });
                    }
                }
                PendingCloseOwner::Storage => {
                    if let Some(storage) = &mut self.storage {
                        storage.closing = false;
                    }
                }
                PendingCloseOwner::Cleanup => {}
            }
            tracing::warn!(target: "bedrock_client::inventory_requests",
                "inventory close cancelled: server did not return every input");
        }
    }

    pub(super) fn retain_close_returns(&mut self, owner: PendingCloseOwner) {
        if let Some(close) = self
            .pending_closes
            .iter_mut()
            .find(|close| close.owner == owner)
        {
            close.returning_inputs = true;
        }
    }

    pub(super) fn note_close_return_failure(&self, error: InventoryGestureError) {
        tracing::warn!(target: "bedrock_client::inventory_requests",
            ?error, "inventory close deferred: inputs could not be returned");
    }
}
