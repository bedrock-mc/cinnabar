//! Local cancellation rolls unsent ownership back without changing response semantics.

use super::queue::{PendingRequest, RollbackAuthority};
use super::{InventoryPendingState, PlayerInventoryLedger};

impl PlayerInventoryLedger {
    /// Drops matching requests and later unsent work built on them. Admitted requests
    /// require recovery; unsent writes restore only the sparse owners they replaced.
    pub(super) fn abandon_requests(&mut self, matches: impl Fn(&PendingRequest) -> bool) {
        let mut cancelled = Vec::new();
        let mut rolling_back = false;
        let mut index = 0;
        while index < self.queue.len() {
            let request = &self.queue[index];
            let unsent = request.state == InventoryPendingState::AwaitingTransport;
            if (unsent
                && (rolling_back
                    || request
                        .predicted
                        .iter()
                        .any(|prediction| prediction.rollback == RollbackAuthority::Unknown)))
                || matches(request)
            {
                let request = self.discard_request(index);
                if unsent {
                    rolling_back = true;
                    cancelled.push(request);
                } else if !request.locally_committed() {
                    self.recover_abandoned(&request);
                }
                continue;
            }
            index += 1;
        }
        self.restore_local_prediction_owners();
        if self.authority != Some(protocol::InventoryAuthority::Client) {
            self.restore_cancelled_predictions(&cancelled);
        }
        self.refold();
    }

    /// Follows replaced owners through the cancelled suffix, stopping at a surviving
    /// prediction or a retired owner. An inactive historic snapshot is never a fallback.
    fn restore_cancelled_predictions(&mut self, cancelled: &[PendingRequest]) {
        for prediction in cancelled
            .iter()
            .flat_map(|request| &request.predicted)
            .filter(|prediction| prediction.active)
        {
            let mut owner = prediction.previous_owner;
            while let Some(id) = owner {
                if let Some(previous) = self
                    .queue
                    .iter_mut()
                    .find(|request| request.request_id == id)
                    .and_then(|request| {
                        request
                            .predicted
                            .iter_mut()
                            .find(|p| p.cell == prediction.cell)
                    })
                {
                    previous.active = true;
                    break;
                }
                owner = cancelled
                    .iter()
                    .find(|request| request.request_id == id)
                    .and_then(|request| {
                        request.predicted.iter().find(|p| p.cell == prediction.cell)
                    })
                    .and_then(|previous| previous.previous_owner);
            }
        }
    }
}
