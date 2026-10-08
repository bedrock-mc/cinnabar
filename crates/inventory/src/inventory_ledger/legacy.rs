use protocol::{
    NetworkItemStack, NormalInventoryChange, NormalInventorySource, Packet, StackRequestAction,
    VerifiedNetworkItemStack, normal_inventory_transaction_packet,
};

use super::cells::Held;
use super::gesture::Submission;
use super::{
    Cell, InventoryGestureError, InventoryPendingState, PendingRequest, PlayerInventoryLedger,
};

impl PlayerInventoryLedger {
    pub(super) fn retire_legacy_write(&mut self, cell: Cell) {
        if self.authority == Some(protocol::InventoryAuthority::Client)
            && self.confirmed.contains(cell)
            && self.queue.iter().any(|request| {
                request.state == InventoryPendingState::AwaitingTransport && request.touches(cell)
            })
        {
            self.abandon_requests(|request| {
                request.state == InventoryPendingState::AwaitingTransport && request.touches(cell)
            });
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
            self.refold();
        }
    }

    pub(super) fn set_authoritative_cell(&mut self, cell: Cell, held: Option<Held>) -> bool {
        self.retire_legacy_write(cell);
        self.confirmed.set(cell, held)
    }

    pub(super) fn validate_legacy_submission(
        &self,
        submission: &Submission,
    ) -> Result<(), InventoryGestureError> {
        if submission.actions.iter().any(|action| {
            !matches!(
                action,
                StackRequestAction::Take { .. }
                    | StackRequestAction::Place { .. }
                    | StackRequestAction::Swap { .. }
                    | StackRequestAction::Drop { .. }
            )
        }) {
            return Err(InventoryGestureError::LegacyActionUnavailable);
        }
        for cell in submission
            .groups
            .iter()
            .flat_map(super::overlay::DeltaGroup::touched)
        {
            self.legacy_cell_address(cell)?;
            verified(self.view().get(cell))?;
        }
        Ok(())
    }

    pub(super) fn legacy_pending_packet(&self) -> Result<Option<Packet>, InventoryGestureError> {
        let Some(request) = self.queue.iter().find(|request| {
            request.mining.is_none() && request.state == InventoryPendingState::AwaitingTransport
        }) else {
            return Ok(None);
        };
        let mut changes = Vec::with_capacity(request.predicted.len() + request.actions.len());
        for prediction in &request.predicted {
            let (window_id, slot) = self.legacy_cell_address(prediction.cell)?;
            changes.push(NormalInventoryChange {
                source: NormalInventorySource::Container(window_id as i8),
                slot: u32::from(slot),
                from: verified(prediction.before.as_ref())?,
                to: verified(prediction.held.as_ref())?,
            });
        }
        for action in &request.actions {
            if let StackRequestAction::Drop {
                amount,
                source,
                randomly,
            } = action
            {
                let cell = self
                    .request_cell(*source)
                    .ok_or(InventoryGestureError::InvalidRequest)?;
                let mut stack = request
                    .predicted
                    .iter()
                    .find(|prediction| prediction.cell == cell)
                    .and_then(|prediction| prediction.before.as_ref())
                    .ok_or(InventoryGestureError::InvalidRequest)?
                    .stack
                    .clone();
                stack.count = u16::from(*amount);
                changes.push(NormalInventoryChange {
                    source: NormalInventorySource::World {
                        randomly: *randomly,
                    },
                    slot: 0,
                    from: verified(None)?,
                    to: VerifiedNetworkItemStack::try_new(stack.clone(), stack.nbt_digest)
                        .map_err(|_| InventoryGestureError::InvalidRequest)?,
                });
            }
        }
        normal_inventory_transaction_packet(changes)
            .map(Some)
            .map_err(|_| InventoryGestureError::InvalidRequest)
    }

    fn legacy_cell_address(&self, cell: Cell) -> Result<(i32, u8), InventoryGestureError> {
        Ok(match cell {
            Cell::Inventory(slot) => (protocol::PLAYER_INVENTORY_WINDOW_ID, slot),
            Cell::Cursor => (protocol::UI_INVENTORY_WINDOW_ID, 0),
            Cell::Armor(slot) => (protocol::ARMOR_WINDOW_ID, slot),
            Cell::Offhand => (protocol::OFFHAND_WINDOW_ID, 0),
            Cell::Craft(slot) => (protocol::UI_INVENTORY_WINDOW_ID, slot),
            Cell::Storage(slot) => (
                self.storage
                    .as_ref()
                    .ok_or(InventoryGestureError::InvalidRequest)?
                    .window_id,
                slot,
            ),
            Cell::CreatedOutput => return Err(InventoryGestureError::LegacyActionUnavailable),
        })
    }

    /// Sent normal transactions become local truth; later slot/content pushes correct it.
    pub(super) fn commit_legacy_transport(&mut self) -> bool {
        let Some(index) = self.queue.iter().position(|request| {
            request.mining.is_none() && request.state == InventoryPendingState::AwaitingTransport
        }) else {
            return false;
        };
        let request: PendingRequest = self.queue.remove(index).expect("index observed");
        if self.request_is_current(&request) {
            for prediction in request.predicted {
                self.confirmed.set(prediction.cell, prediction.held);
            }
        }
        self.refold();
        self.finish_closing();
        true
    }
}

fn verified(held: Option<&Held>) -> Result<VerifiedNetworkItemStack, InventoryGestureError> {
    let stack = held.map_or_else(NetworkItemStack::empty, |held| held.stack.clone());
    let digest = stack.nbt_digest;
    VerifiedNetworkItemStack::try_new(stack, digest)
        .map_err(|_| InventoryGestureError::InvalidRequest)
}
