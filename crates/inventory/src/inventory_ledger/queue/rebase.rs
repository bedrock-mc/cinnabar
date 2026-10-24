//! Reconciliation of rollback authority and item data across retained requests.

use super::{
    Cell, Held, InventoryPendingState, PlayerInventoryLedger, RollbackAuthority, same_name,
};

impl PlayerInventoryLedger {
    /// Rebases later rollback sources and routes their item data through dependent results.
    /// Only requests not yet transported replay actions, counts and wire identities.
    pub(in crate::inventory_ledger) fn rebase_predictions(
        &mut self,
        predecessor: i32,
        cell: Cell,
        from: Option<&Held>,
        to: Option<Held>,
        authority: RollbackAuthority,
    ) {
        let mut replacements = vec![(cell, from.cloned(), to, authority, predecessor)];
        let mut queue = std::mem::take(&mut self.queue);
        let mut invalid = None;
        'requests: for request in queue
            .iter_mut()
            .filter(|request| request.request_id < predecessor)
        {
            let derived_result = request.predicts_derived_result();
            let mut changed = false;
            let mut metadata = Vec::new();
            for prediction in request
                .predicted
                .iter_mut()
                .filter(|prediction| prediction.current)
            {
                if let Some((_, _, replacement, authority, _)) =
                    replacements.iter().rev().find(|(cell, old, _, _, owner)| {
                        *cell == prediction.cell
                            && match prediction.rollback {
                                RollbackAuthority::Pending(source) => source == *owner,
                                RollbackAuthority::Known | RollbackAuthority::AcceptedAlias(_) => {
                                    same_name(prediction.before.as_ref(), old.as_ref())
                                }
                                RollbackAuthority::Unknown => false,
                            }
                    })
                {
                    let same_item = match (prediction.before.as_ref(), replacement.as_ref()) {
                        (Some(before), Some(after)) => {
                            Self::same_item(&before.stack, &after.stack)
                                || (before.same_source(after)
                                    && before.stack.network_id == after.stack.network_id
                                    && before.stack.metadata == after.stack.metadata
                                    && before.stack.block_runtime_id
                                        == after.stack.block_runtime_id)
                        }
                        (None, None) => true,
                        _ => false,
                    };
                    let same_data = match (prediction.before.as_ref(), replacement.as_ref()) {
                        (Some(before), Some(after)) => {
                            before.stack.count == after.stack.count
                                && before.stack.nbt_digest == after.stack.nbt_digest
                        }
                        (None, None) => true,
                        _ => false,
                    };
                    if request.state == InventoryPendingState::AwaitingTransport
                        && (!same_item
                            || *authority == RollbackAuthority::Unknown
                            || (derived_result && !same_data))
                    {
                        invalid = Some(request.request_id);
                        break 'requests;
                    }
                    // This dependency edge also identifies split-off items without an original ID.
                    if same_item && let Some(held) = replacement {
                        metadata.push((prediction.cell, held.clone()));
                    }
                    prediction.before = replacement.clone();
                    prediction.rollback = *authority;
                    changed = true;
                }
            }
            if changed && request.state == InventoryPendingState::AwaitingTransport {
                let mut cells = self.confirmed.clone();
                for prediction in &request.predicted {
                    cells.set(prediction.cell, prediction.before.clone());
                }
                self.rebind_actions_in_order(
                    &mut request.actions,
                    request.request_id,
                    |cell| {
                        Some(
                            cells
                                .get(cell)
                                .map_or(0, |held| held.stack.stack_network_id),
                        )
                    },
                    |cell| cells.get(cell).map(|held| held.stack.count),
                );
                let mut placements = Vec::new();
                if !request.groups.iter_mut().all(|group| {
                    group.rebind(&cells);
                    if self
                        .validate_group(&cells, group, &request.actions)
                        .is_err()
                    {
                        return false;
                    }
                    placements.extend(group.placement_inputs(&cells));
                    group.apply(&mut cells)
                }) {
                    invalid = Some(request.request_id);
                    break;
                }
                request.placements = placements;
                for prediction in &mut request.predicted {
                    let old = prediction.held.take();
                    prediction.held = cells.get(prediction.cell).cloned();
                    if self.authority == Some(protocol::InventoryAuthority::Server)
                        && let Some(held) = &mut prediction.held
                    {
                        held.bind_prediction(request.request_id, prediction.cell);
                    }
                    replacements.push((
                        prediction.cell,
                        old,
                        prediction.held.clone(),
                        RollbackAuthority::Pending(request.request_id),
                        request.request_id,
                    ));
                }
            } else if changed && !request.locally_committed() {
                for prediction in request.predicted.iter_mut().filter(|p| p.current) {
                    let Some(source) = request
                        .groups
                        .iter()
                        .rev()
                        .try_fold(prediction.cell, |cell, group| group.item_data_source(cell))
                        .and_then(|cell| metadata.iter().find(|(source, _)| *source == cell))
                        .map(|(_, held)| held)
                    else {
                        continue;
                    };
                    let old = prediction.held.clone();
                    let Some(held) = &mut prediction.held else {
                        continue;
                    };
                    if !held.inherit_item_data(source) {
                        continue;
                    }
                    replacements.push((
                        prediction.cell,
                        old,
                        Some(held.clone()),
                        RollbackAuthority::Pending(request.request_id),
                        request.request_id,
                    ));
                }
            }
        }
        self.queue = queue;
        if let Some(id) = invalid {
            self.abandon_requests(|request| request.request_id == id);
        }
    }
}
