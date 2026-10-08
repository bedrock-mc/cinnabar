//! Incremental splitting, following vanilla's multi-slot split;
//! rebalance the contributions, never the pre-existing
//! destination stacks, on entry into each new slot. The cursor keeps the remainder.

use protocol::NetworkItemStack;

use super::*;
use crate::inventory_ledger::cells::Cells;

/// Bound shared with pointer collection; no unbounded gesture history.
pub const MAX_DISTRIBUTION_CELLS: usize = 54;

#[derive(Clone, Debug)]
struct Contribution {
    cell: Cell,
    baseline: u16,
    placed: u16,
}

/// Only the items this drag contributed are available for subsequent rebalancing.
#[derive(Clone, Debug)]
pub struct DragDistribution {
    template: NetworkItemStack,
    cells: Vec<Contribution>,
    session: u64,
    personal: Option<u64>,
    storage: Option<u64>,
    mode: DistributeMode,
    remaining: u16,
}

impl PlayerInventoryLedger {
    /// Accounts a press-time placement into `target` as the first cell of a drag, so a second
    /// cell rebalances it like any split; `None` when the press did not place `template` there.
    #[must_use]
    pub fn distribution_after_place(
        &self,
        template: &NetworkItemStack,
        target: InventoryTarget,
        before: Option<&NetworkItemStack>,
        mode: DistributeMode,
    ) -> Option<DragDistribution> {
        let cell = target.cell();
        if matches!(cell, Cell::Armor(_) | Cell::Offhand)
            || before.is_some_and(|before| !PlayerInventoryLedger::same_item(before, template))
        {
            return None;
        }
        let baseline = before.map_or(0, |before| before.count);
        let held = self.view().get(cell)?;
        let placed = held.stack.count.checked_sub(baseline).filter(|placed| {
            *placed > 0 && PlayerInventoryLedger::same_item(&held.stack, template)
        })?;
        let remaining = template.count.checked_sub(placed)?;
        let cursor = self.view().get(Cell::Cursor);
        if cursor.map_or(0, |held| held.stack.count) != remaining
            || cursor.is_some_and(|held| !PlayerInventoryLedger::same_item(&held.stack, template))
        {
            return None;
        }
        Some(DragDistribution {
            template: template.clone(),
            cells: vec![Contribution {
                cell,
                baseline,
                placed,
            }],
            session: self.session_generation,
            personal: self
                .gesture_preflight(!matches!(target, InventoryTarget::Storage(_)))
                .ok()?,
            storage: self.storage.as_ref().map(|window| window.generation),
            mode,
            remaining,
        })
    }

    /// Updates the real predicted ledger before mouse release. Each atomic
    /// transfer has its own request scope, as in vanilla,
    /// so subsequent transfers name the sparse cell, not a cloned donor ID.
    pub fn advance_distribute(
        &mut self,
        retained: &mut Option<DragDistribution>,
        targets: &[InventoryTarget],
        mode: DistributeMode,
    ) -> Result<Option<i32>, InventoryGestureError> {
        if targets.len() > MAX_DISTRIBUTION_CELLS {
            return Err(InventoryGestureError::InvalidRequest);
        }
        let personal = self.gesture_preflight(
            targets
                .iter()
                .any(|t| !matches!(t, InventoryTarget::Storage(_))),
        )?;
        self.check_surfaces([Cell::Cursor])?;
        let mut state = match retained {
            Some(state) => state.clone(),
            None => DragDistribution {
                template: self
                    .named(self.view().get(Cell::Cursor).cloned())?
                    .ok_or(InventoryGestureError::EmptyGesture)?
                    .stack,
                cells: Vec::new(),
                session: self.session_generation,
                personal,
                storage: self.storage.as_ref().map(|window| window.generation),
                mode,
                remaining: self
                    .view()
                    .get(Cell::Cursor)
                    .map_or(0, |held| held.stack.count),
            },
        };
        if state.session != self.session_generation
            || state.personal != personal
            || state.storage != self.storage.as_ref().map(|window| window.generation)
            || state.mode != mode
        {
            return Err(InventoryGestureError::InvalidRequest);
        }
        let capacity = self
            .negotiated_item_entry(state.template.network_id)
            .and_then(entry_capacity)
            .map(u16::from)
            .unwrap_or(state.template.count);
        let cursor = self.named(self.view().get(Cell::Cursor).cloned())?;
        if cursor.as_ref().is_some_and(|held| {
            !PlayerInventoryLedger::same_item(&held.stack, &state.template)
                || has_meaningful_overlay(held.overlay.as_ref())
        }) {
            return Err(InventoryGestureError::InvalidRequest);
        }
        for contribution in &state.cells {
            self.check_target(contribution.cell)?;
            self.check_surfaces([contribution.cell])?;
            let held = self.named(self.view().get(contribution.cell).cloned())?;
            if held.as_ref().map_or(0, |held| held.stack.count)
                != contribution.baseline + contribution.placed
                || held.as_ref().is_some_and(|held| {
                    !PlayerInventoryLedger::same_item(&held.stack, &state.template)
                        || has_meaningful_overlay(held.overlay.as_ref())
                })
            {
                // A server correction or another gesture invalidates this drag's
                // accounting; never take unrelated items to repair it.
                return Err(InventoryGestureError::InvalidRequest);
            }
        }
        for target in targets {
            let cell = target.cell();
            if matches!(cell, Cell::Armor(_) | Cell::Offhand)
                || state.cells.iter().any(|entry| entry.cell == cell)
                || self.check_target(cell).is_err()
                || self.check_surfaces([cell]).is_err()
            {
                continue;
            }
            let held = self.view().get(cell);
            if held.is_some_and(|held| {
                self.awaiting_identity(held)
                    || has_meaningful_overlay(held.overlay.as_ref())
                    || !PlayerInventoryLedger::same_item(&held.stack, &state.template)
                    || held.stack.count >= capacity
            }) {
                continue;
            }
            state.cells.push(Contribution {
                cell,
                baseline: held.map_or(0, |held| held.stack.count),
                placed: 0,
            });
        }
        if state.cells.len() < 2 {
            return Err(InventoryGestureError::InvalidRequest);
        }
        let total = state
            .cells
            .iter()
            .try_fold(
                state
                    .remaining
                    .min(cursor.as_ref().map_or(0, |held| held.stack.count)),
                |sum, contribution| sum.checked_add(contribution.placed),
            )
            .ok_or(InventoryGestureError::InvalidRequest)?;
        let share = match mode {
            DistributeMode::Even => (total / state.cells.len() as u16).max(1),
            DistributeMode::One => 1,
        };
        let mut left = total;
        let desired: Vec<_> = state
            .cells
            .iter()
            .map(|entry| {
                let count = share.min(capacity.saturating_sub(entry.baseline)).min(left);
                left -= count;
                count
            })
            .collect();
        // Stage the bounded hover atomically. A capacity or identity failure
        // cannot leave a partially admitted drag on the real ledger.
        let mut staged = self.clone();
        let mut operations = 0;
        let mut request = None;
        let mut remaining = state
            .remaining
            .min(cursor.as_ref().map_or(0, |held| held.stack.count));
        for destination in 0..state.cells.len() {
            for source in 0..=state.cells.len() {
                let need = desired[destination].saturating_sub(state.cells[destination].placed);
                if need == 0 {
                    break;
                }
                let (source_cell, available) = if source == state.cells.len() {
                    (Cell::Cursor, remaining)
                } else {
                    (
                        state.cells[source].cell,
                        state.cells[source].placed.saturating_sub(desired[source]),
                    )
                };
                let amount = need.min(available);
                if amount == 0 {
                    continue;
                }
                let built = staged.distribution_transfer(
                    staged.view(),
                    source_cell,
                    state.cells[destination].cell,
                    amount,
                )?;
                staged.ensure_queue_capacity()?;
                operations += 1;
                if operations > protocol::MAX_STACK_REQUEST_ACTIONS {
                    return Err(InventoryGestureError::InvalidRequest);
                }
                request = Some(staged.submit_built(built, personal)?);
                state.cells[destination].placed += amount;
                if source < state.cells.len() {
                    state.cells[source].placed -= amount;
                } else {
                    remaining -= amount;
                }
            }
        }
        for (index, desired) in desired.into_iter().enumerate() {
            let amount = state.cells[index].placed.saturating_sub(desired);
            if amount == 0 {
                continue;
            }
            let built = staged.distribution_transfer(
                staged.view(),
                state.cells[index].cell,
                Cell::Cursor,
                amount,
            )?;
            staged.ensure_queue_capacity()?;
            operations += 1;
            if operations > protocol::MAX_STACK_REQUEST_ACTIONS {
                return Err(InventoryGestureError::InvalidRequest);
            }
            request = Some(staged.submit_built(built, personal)?);
            remaining = remaining
                .checked_add(amount)
                .ok_or(InventoryGestureError::InvalidRequest)?;
            state.cells[index].placed -= amount;
        }
        state.remaining = remaining;
        *self = staged;
        *retained = Some(state);
        Ok(request)
    }

    fn distribution_transfer(
        &self,
        working: &Cells,
        source: Cell,
        destination: Cell,
        amount: u16,
    ) -> Result<super::super::gesture::Built, InventoryGestureError> {
        let from = working
            .get(source)
            .ok_or(InventoryGestureError::EmptyGesture)?;
        let kind = if destination == Cell::Cursor {
            StackRequestActionKind::Take
        } else {
            StackRequestActionKind::Place
        };
        match working.get(destination) {
            None => counted_transfer(
                kind,
                source,
                destination,
                &from.stack,
                self.window_address(),
                Some(amount),
            ),
            Some(into) => {
                let OccupiedStackRelation::Compatible { capacity } =
                    self.occupied_stack_relation(&from.stack, &into.stack)
                else {
                    return Err(InventoryGestureError::InvalidRequest);
                };
                counted_merge(
                    kind,
                    source,
                    destination,
                    &from.stack,
                    &into.stack,
                    self.window_address(),
                    amount,
                    capacity,
                )
            }
        }
    }
}
