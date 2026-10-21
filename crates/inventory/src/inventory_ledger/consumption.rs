//! Admitted gameplay consumption shared by retained response and rollback history.

use super::cells::{Cell, Held};
use super::overlay::DeltaGroup;
use super::queue::{CellPrediction, RollbackAuthority};
use super::{InventoryPendingState, PendingRequest, PlayerInventoryLedger};

/// Later consumption belonging to one retained request cell, independent of its screen scope.
#[derive(Debug, Clone, Copy)]
pub(super) struct SubsequentConsumption {
    pub(super) cell: Cell,
    pub(super) amount: u16,
    pub(super) stack_network_id: i32,
}

impl PlayerInventoryLedger {
    /// Resolves a gesture cell with admitted consumption, preserving its pending wire owner.
    pub(super) fn transaction_held(&self, cell: Cell) -> Option<Held> {
        self.queue
            .iter()
            .rev()
            .find_map(|request| {
                request
                    .predicted
                    .iter()
                    .find(|prediction| prediction.cell == cell && prediction.active)
                    .map(|prediction| request.transaction_result(prediction))
            })
            .unwrap_or_else(|| self.confirmed.get(cell).cloned())
    }

    /// Decrements each retained backing alternative once, preserving other cells' wire owners.
    pub(super) fn consume_retained_source(&mut self, cell: Cell, source: &Held, name: i32) {
        let mut positions = Vec::new();
        let mut replacements = Vec::new();
        for request in self
            .queue
            .iter_mut()
            .filter(|request| request.state == InventoryPendingState::AwaitingResponse)
        {
            let before: Vec<_> = request
                .predicted
                .iter()
                .filter(|prediction| prediction.current)
                .map(|prediction| (prediction.cell, request.transaction_result(prediction)))
                .collect();
            request.note_consumption(cell, source, name, &mut positions);
            for (cell, from) in before {
                let prediction = request.predicted.iter().find(|p| p.cell == cell).unwrap();
                let to = request.transaction_result(prediction);
                if from != to {
                    let authority = if request.locally_committed() {
                        RollbackAuthority::Known
                    } else {
                        RollbackAuthority::Pending(request.request_id)
                    };
                    replacements.push((request.request_id, cell, from, to, authority));
                }
            }
        }
        let changes: Vec<_> = self
            .confirmed
            .occupied()
            .filter(|(candidate, held)| {
                *candidate == cell
                    || held.same_source_item(source)
                    || positions.iter().any(|position| {
                        position.cell == *candidate
                            && position.stack_network_id == held.stack.stack_network_id
                    })
            })
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
        for (request, cell, from, to, authority) in replacements {
            self.rebase_predictions(request, cell, from.as_ref(), to, authority);
        }
        self.refold();
    }
}

/// The one-based position of a consumed unit in a retained result's stack.
/// Transfers take the first units, so consuming the last unit exhausts residuals first.
struct ConsumptionPosition {
    cell: Cell,
    stack_network_id: i32,
    ordinal: u16,
}

impl PendingRequest {
    /// Removes consumption from a retained screen result once; local drops already include it.
    fn transaction_result(&self, prediction: &CellPrediction) -> Option<Held> {
        let amount = if self.locally_committed() {
            0
        } else {
            self.consumed_after
                .iter()
                .find(|consumed| consumed.cell == prediction.cell)
                .map_or(0, |consumed| consumed.amount)
        };
        consumed_stack(prediction.held.as_ref(), amount, None)
    }

    /// Routes one consumed unit through this request and retains its successor dependencies.
    fn note_consumption(
        &mut self,
        cell: Cell,
        source: &Held,
        name: i32,
        positions: &mut Vec<ConsumptionPosition>,
    ) {
        if self.state != InventoryPendingState::AwaitingResponse {
            return;
        }
        let inputs: Vec<_> = self
            .predicted
            .iter()
            .filter(|prediction| prediction.current)
            .filter_map(|prediction| {
                let before = prediction.before.as_ref()?;
                let inherited = positions.iter().rev().find(|position| {
                    position.cell == prediction.cell
                        && position.stack_network_id == before.stack.stack_network_id
                });
                let ordinal = inherited.map(|position| position.ordinal).or_else(|| {
                    before
                        .same_source_item(source)
                        .then_some(before.stack.count)
                })?;
                Some((prediction.cell, ordinal))
            })
            .collect();
        let mut results = self.route_consumption(&inputs);
        // A split can create the backing source after this request was transported.
        // Its result identifies that unit even though no original input does.
        for prediction in &self.predicted {
            if inputs.is_empty()
                && prediction
                    .held
                    .as_ref()
                    .is_some_and(|held| held.same_source_item(source))
                && !results.iter().any(|(cell, _)| *cell == prediction.cell)
            {
                let held = prediction.held.as_ref().expect("matching held value");
                let consumed = if self.locally_committed() {
                    0
                } else {
                    self.consumed_after
                        .iter()
                        .find(|entry| entry.cell == prediction.cell)
                        .map_or(0, |entry| entry.amount)
                };
                results.push((prediction.cell, held.stack.count.saturating_sub(consumed)));
            }
        }
        if let Some(mining) = &self.mining
            && mining.retains_source_item(source)
        {
            results.push((Cell::Inventory(mining.slot), source.stack.count));
        }
        let published_result = self.locally_committed();
        for prediction in self
            .predicted
            .iter_mut()
            .filter(|prediction| prediction.current)
        {
            if inputs.iter().any(|(cell, _)| *cell == prediction.cell) {
                let rename = prediction.cell == cell;
                let id = prediction.before.as_ref().and_then(|held| {
                    (rename && held.stack.stack_network_id == source.stack.stack_network_id)
                        .then_some(name)
                });
                prediction.before = consumed_stack(prediction.before.as_ref(), 1, id);
            }
        }
        let mut recorded = Vec::new();
        for (result_cell, ordinal) in results {
            if recorded.contains(&result_cell) {
                continue;
            }
            recorded.push(result_cell);
            if let Some(prediction) = self.predicted.iter_mut().find(|p| p.cell == result_cell)
                && prediction.current
                && let Some(held) = &prediction.held
            {
                positions.push(ConsumptionPosition {
                    cell: result_cell,
                    stack_network_id: held.stack.stack_network_id,
                    ordinal,
                });
                if published_result && prediction.current {
                    let rename = result_cell == cell
                        && held.stack.stack_network_id == source.stack.stack_network_id;
                    prediction.held = consumed_stack(Some(held), 1, rename.then_some(name));
                }
            }
            if let Some(consumed) = self
                .consumed_after
                .iter_mut()
                .find(|entry| entry.cell == result_cell)
            {
                consumed.amount = consumed.amount.saturating_add(1);
                consumed.stack_network_id = name;
            } else {
                self.consumed_after.push(SubsequentConsumption {
                    cell: result_cell,
                    amount: 1,
                    stack_network_id: name,
                });
            }
        }
    }

    /// Follows input units through transfers, prioritizing residuals over transferred items.
    fn route_consumption(&self, inputs: &[(Cell, u16)]) -> Vec<(Cell, u16)> {
        let mut results = inputs.to_vec();
        let mut counts: Vec<_> = self
            .predicted
            .iter()
            .map(|prediction| {
                (
                    prediction.cell,
                    prediction
                        .before
                        .as_ref()
                        .map_or(0, |held| held.stack.count),
                )
            })
            .collect();
        for group in &self.groups {
            match group {
                DeltaGroup::Transfer {
                    source,
                    destination,
                    amount,
                    ..
                } => {
                    let into = counts
                        .iter()
                        .find(|(cell, _)| cell == destination)
                        .map_or(0, |(_, count)| *count);
                    for (cell, ordinal) in &mut results {
                        if cell == source {
                            if *ordinal <= *amount {
                                *cell = *destination;
                                *ordinal = into.saturating_add(*ordinal);
                            } else {
                                *ordinal -= amount;
                            }
                        }
                    }
                    for (cell, count) in &mut counts {
                        if cell == source {
                            *count = count.saturating_sub(*amount);
                        } else if cell == destination {
                            *count = count.saturating_add(*amount);
                        }
                    }
                }
                DeltaGroup::Swap {
                    source,
                    destination,
                    ..
                } => {
                    for (cell, _) in results.iter_mut().chain(counts.iter_mut()) {
                        if cell == source {
                            *cell = *destination;
                        } else if cell == destination {
                            *cell = *source;
                        }
                    }
                }
                DeltaGroup::Shrink { source, amount, .. } => {
                    for (cell, count) in results.iter_mut().chain(counts.iter_mut()) {
                        if cell == source {
                            *count = count.saturating_sub(*amount);
                        }
                    }
                }
                DeltaGroup::Set { cell, held } => {
                    results.retain(|(result, _)| result != cell);
                    if let Some((_, count)) =
                        counts.iter_mut().find(|(candidate, _)| candidate == cell)
                    {
                        *count = held.stack.count;
                    }
                }
            }
        }
        results
    }
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
