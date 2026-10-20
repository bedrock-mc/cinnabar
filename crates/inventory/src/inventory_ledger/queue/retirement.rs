//! Dependency retirement without inventing outcomes for unanswered requests.

use super::{
    Cell, Held, InventoryPendingState, PendingRequest, PlayerInventoryLedger, RollbackAuthority,
};

impl PlayerInventoryLedger {
    /// Captures the sparse owner of a rollback, including a predicted empty cell.
    pub(in crate::inventory_ledger) fn rollback_authority(
        &self,
        cell: Cell,
        before: Option<&Held>,
    ) -> RollbackAuthority {
        if let Some(owner) = self.queue.iter().rev().find(|request| {
            request.predicted.iter().any(|prediction| {
                prediction.cell == cell
                    && (prediction.active
                        || (before.is_none()
                            && prediction.current
                            && prediction.owns_empty_backing))
            })
        }) {
            return if owner.locally_committed() {
                RollbackAuthority::Known
            } else {
                RollbackAuthority::Pending(owner.request_id)
            };
        }
        if let Some(held) = before {
            let id = held.stack.stack_network_id;
            if self.authority == Some(protocol::InventoryAuthority::Server)
                && self.queue.iter().any(|request| {
                    request.request_id == id
                        && request
                            .predicted
                            .iter()
                            .any(|prediction| prediction.cell == cell)
                })
            {
                return RollbackAuthority::Pending(id);
            }
            if self.awaiting_identity(held) {
                return RollbackAuthority::Unknown;
            }
        }
        RollbackAuthority::Known
    }

    /// Marks an unresolved restore independently of abandoned requests' eventual replies.
    pub(in crate::inventory_ledger) fn require_rollback_recovery(&mut self, cell: Cell) {
        self.disown_abandoned_recovery(cell.surface());
        self.require_cell_recovery(cell);
    }

    /// Retires local ownership, retaining accepted wire aliases only for queued transport.
    pub(in crate::inventory_ledger) fn detach_request_sources(
        &mut self,
        owner: i32,
        accepted: bool,
    ) {
        let wire_owner = self.authority == Some(protocol::InventoryAuthority::Server);
        for request in self
            .queue
            .iter_mut()
            .filter(|request| request.request_id != owner)
        {
            for prediction in &mut request.predicted {
                if prediction.rollback == RollbackAuthority::Pending(owner)
                    || (wire_owner
                        && !matches!(prediction.rollback, RollbackAuthority::AcceptedAlias(_))
                        && prediction
                            .before
                            .as_ref()
                            .is_some_and(|held| held.stack.stack_network_id == owner))
                {
                    if accepted && wire_owner {
                        prediction.rollback = RollbackAuthority::AcceptedAlias(owner);
                    } else {
                        prediction.rollback = RollbackAuthority::Unknown;
                        if let Some(before) = &mut prediction.before {
                            before.stack.stack_network_id = 0;
                        }
                    }
                }
            }
        }
        if !wire_owner {
            return;
        }
        let cells = self
            .confirmed
            .occupied()
            .filter(|(_, held)| held.stack.stack_network_id == owner)
            .map(|(cell, _)| cell)
            .collect::<Vec<_>>();
        for cell in cells {
            self.unverify_backing(cell);
        }
    }

    /// Retains ambiguous item data without advertising a usable wire identity.
    fn unverify_backing(&mut self, cell: Cell) {
        if cell == Cell::CreatedOutput {
            self.confirmed.clear_result();
        } else {
            self.confirmed
                .get_mut(cell)
                .expect("occupied cell observed")
                .stack
                .stack_network_id = 0;
            self.require_rollback_recovery(cell);
        }
    }

    /// Rechecks retained identities when negotiation changes their ownership rules.
    pub(in crate::inventory_ledger) fn verify_backing_authority(&mut self) {
        let cells = self
            .confirmed
            .occupied()
            .filter(|(_, held)| self.awaiting_identity(held))
            .map(|(cell, _)| cell)
            .collect::<Vec<_>>();
        for cell in cells {
            self.unverify_backing(cell);
        }
    }

    /// Invalidates unresolved dependants before removing one retained prediction owner.
    pub(in crate::inventory_ledger) fn discard_request(&mut self, index: usize) -> PendingRequest {
        let owner = self.queue[index].request_id;
        self.detach_request_sources(owner, false);
        self.queue.remove(index).expect("request index observed")
    }

    /// Discards selected owners and unsent descendants; admitted gameplay history remains.
    pub(in crate::inventory_ledger) fn discard_requests(
        &mut self,
        owns: impl Fn(&PendingRequest) -> bool,
    ) -> bool {
        let mut removed = false;
        let mut rolling_back = false;
        let mut index = 0;
        while index < self.queue.len() {
            let request = &self.queue[index];
            let unsent = request.state == InventoryPendingState::AwaitingTransport;
            let unverified = request
                .predicted
                .iter()
                .any(|prediction| prediction.rollback == RollbackAuthority::Unknown);
            if owns(request) || (unsent && (rolling_back || unverified)) {
                rolling_back |= unsent;
                self.discard_request(index);
                removed = true;
            } else {
                index += 1;
            }
        }
        if rolling_back {
            self.restore_local_prediction_owners();
        }
        removed
    }

    /// Finishes settlement, preserving a current accepted predecessor's queued transport aliases.
    pub(in crate::inventory_ledger) fn finish_request_retirement(
        &mut self,
        owner: i32,
        accepted: bool,
    ) {
        self.detach_request_sources(owner, accepted);
        self.discard_requests(|_| false);
    }

    /// Retires a destroyed screen without cancelling independently admitted gameplay actions.
    pub(in crate::inventory_ledger) fn retire_screen_requests(
        &mut self,
        owns: impl Fn(&PendingRequest) -> bool,
    ) {
        self.discard_requests(|request| {
            request.mining.is_none() && !request.locally_committed() && owns(request)
        });
    }

    /// Restores client-authority views after unsent work is discarded; server owners stay sparse.
    pub(in crate::inventory_ledger) fn restore_local_prediction_owners(&mut self) {
        if self.authority != Some(protocol::InventoryAuthority::Client) {
            return;
        }
        let mut owned = Vec::new();
        for prediction in self
            .queue
            .iter_mut()
            .rev()
            .flat_map(|request| request.predicted.iter_mut().rev())
        {
            prediction.active = !owned.contains(&prediction.cell);
            if prediction.active {
                owned.push(prediction.cell);
            }
        }
    }
}
