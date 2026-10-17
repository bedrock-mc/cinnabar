//! Admitted gameplay consumption shared by retained response and rollback history.

use super::cells::{Cell, Held};
use super::{InventoryPendingState, PendingRequest};

/// Later consumption belonging to one retained request cell, independent of its screen scope.
#[derive(Debug, Clone, Copy)]
pub(super) struct SubsequentConsumption {
    pub(super) cell: Cell,
    pub(super) amount: u16,
    pub(super) stack_network_id: i32,
}

impl PendingRequest {
    /// Preserves admitted consumption in sent response history and current rollback sources.
    pub(super) fn note_consumption(&mut self, cell: Cell, source: &Held, stack_network_id: i32) {
        if self.state != InventoryPendingState::AwaitingResponse {
            return;
        }
        let same_source = |held: &Held| held.same_source_item(source);
        let retained = self.mining.as_ref().is_some_and(|mining| {
            cell == Cell::Inventory(mining.slot) && mining.source.as_ref().is_some_and(same_source)
        }) || self.predicted.iter().any(|prediction| {
            prediction.cell == cell
                && prediction
                    .held
                    .as_ref()
                    .or(prediction.before.as_ref())
                    .is_some_and(same_source)
                && self
                    .groups
                    .iter()
                    .filter(|group| group.touched().any(|touched| touched == cell))
                    .all(|group| group.retains_item_at(cell))
        });
        if retained {
            if let Some(consumed) = self
                .consumed_after
                .iter_mut()
                .find(|consumed| consumed.cell == cell)
            {
                consumed.amount = consumed.amount.saturating_add(1);
                consumed.stack_network_id = stack_network_id;
            } else {
                self.consumed_after.push(SubsequentConsumption {
                    cell,
                    amount: 1,
                    stack_network_id,
                });
            }
        }
        let published_result = self.locally_committed();
        // A rejected replacement restores its input, which can be the consumed item.
        for prediction in self
            .predicted
            .iter_mut()
            .filter(|prediction| prediction.cell == cell && prediction.current)
        {
            consume_snapshot(&mut prediction.before, source, stack_network_id);
            // Screen results keep their absolute presentation until answered. Admitted
            // local drops have already published their result into backing truth.
            if published_result {
                consume_snapshot(&mut prediction.held, source, stack_network_id);
            }
        }
    }
}

/// Decrements a matching snapshot, renaming only the exact backing owner that was consumed.
fn consume_snapshot(snapshot: &mut Option<Held>, source: &Held, stack_network_id: i32) {
    let Some(held) = snapshot
        .as_ref()
        .filter(|held| held.same_source_item(source))
    else {
        return;
    };
    let name =
        (held.stack.stack_network_id == source.stack.stack_network_id).then_some(stack_network_id);
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
