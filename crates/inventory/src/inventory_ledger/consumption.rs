//! Admitted gameplay consumption shared by retained response and rollback history.

use super::cells::{Cell, Held};
use super::queue::RollbackAuthority;
use super::{InventoryPendingState, PendingRequest, PlayerInventoryLedger};

/// Later consumption belonging to one retained request cell, independent of its screen scope.
#[derive(Debug, Clone, Copy)]
pub(super) struct SubsequentConsumption {
    pub(super) cell: Cell,
    pub(super) amount: u16,
    pub(super) stack_network_id: i32,
}

impl PlayerInventoryLedger {
    /// Decrements each retained backing alternative once, preserving other cells' wire owners.
    pub(super) fn consume_retained_source(&mut self, cell: Cell, source: &Held, name: i32) {
        for request in &mut self.queue {
            request.note_consumption(cell, source, name);
        }
        let changes: Vec<_> = self
            .confirmed
            .occupied()
            .filter(|(candidate, held)| *candidate == cell || held.same_source_item(source))
            .map(|(candidate, held)| {
                let after = consumed_stack(Some(held), 1, (candidate == cell).then_some(name));
                (candidate, held.clone(), after)
            })
            .collect();
        // Install all alternatives before rebuilding any unsent request that spans them.
        for (cell, _, after) in &changes {
            self.confirmed.set(*cell, after.clone());
        }
        for (cell, before, after) in changes {
            self.rebase_predictions(
                i32::MAX,
                cell,
                Some(&before),
                after,
                RollbackAuthority::Known,
            );
        }
        self.refold();
    }
}

impl PendingRequest {
    /// Preserves consumption in every retained result and current rollback input of this source.
    pub(super) fn note_consumption(&mut self, cell: Cell, source: &Held, name: i32) {
        if self.state != InventoryPendingState::AwaitingResponse {
            return;
        }
        let same_source = |held: &Held| held.same_source_item(source);
        let has_result = self
            .predicted
            .iter()
            .any(|prediction| prediction.held.as_ref().is_some_and(same_source));
        let mut result_cells: Vec<_> = self
            .predicted
            .iter()
            .filter(|prediction| {
                prediction.held.as_ref().is_some_and(same_source)
                    || (!has_result
                        && prediction.held.is_none()
                        && prediction.before.as_ref().is_some_and(same_source)
                        && self
                            .groups
                            .iter()
                            .filter(|group| {
                                group.touched().any(|touched| touched == prediction.cell)
                            })
                            .all(|group| group.retains_item_at(prediction.cell)))
            })
            .map(|prediction| prediction.cell)
            .collect();
        if let Some(mining) = &self.mining
            && mining.retains_source_item(source)
        {
            let mining_cell = Cell::Inventory(mining.slot);
            if !result_cells.contains(&mining_cell) {
                result_cells.push(mining_cell);
            }
        }
        for cell in result_cells {
            if let Some(consumed) = self
                .consumed_after
                .iter_mut()
                .find(|entry| entry.cell == cell)
            {
                consumed.amount = consumed.amount.saturating_add(1);
                consumed.stack_network_id = name;
            } else {
                self.consumed_after.push(SubsequentConsumption {
                    cell,
                    amount: 1,
                    stack_network_id: name,
                });
            }
        }
        let published_result = self.locally_committed();
        for prediction in self
            .predicted
            .iter_mut()
            .filter(|prediction| prediction.current)
        {
            let rename = prediction.cell == cell;
            consume_snapshot(&mut prediction.before, source, rename, name);
            // Screen results keep their absolute presentation until answered. Admitted
            // local drops have already published their result into backing truth.
            if published_result {
                consume_snapshot(&mut prediction.held, source, rename, name);
            }
        }
    }
}

/// Decrements a matching snapshot, renaming only the exact backing owner that was consumed.
fn consume_snapshot(snapshot: &mut Option<Held>, source: &Held, rename: bool, name: i32) {
    let Some(held) = snapshot
        .as_ref()
        .filter(|held| held.same_source_item(source))
    else {
        return;
    };
    let name =
        (rename && held.stack.stack_network_id == source.stack.stack_network_id).then_some(name);
    *snapshot = consumed_stack(Some(held), 1, name);
}

/// Removes admitted consumption from a snapshot, optionally renaming its surviving stack.
pub(super) fn consumed_stack(held: Option<&Held>, amount: u16, name: Option<i32>) -> Option<Held> {
    let mut held = held?.clone();
    held.stack.count = held.stack.count.saturating_sub(amount);
    if held.stack.count == 0 {
        return None;
    }
    if let Some(name) = name {
        held.stack.stack_network_id = name;
    }
    Some(held)
}
