use protocol::{
    NetworkItemStack, NormalInventoryChange, NormalInventorySource, Packet, StackRequestAction,
    VerifiedNetworkItemStack, inventory_drop_transaction_packet,
    normal_inventory_transaction_packet,
};

use super::cells::Held;
use super::gesture::Submission;
use super::queue::RollbackAuthority;
use super::{
    Cell, InventoryGestureError, InventoryPendingState, PendingRequest, PlayerInventoryLedger,
};

impl PlayerInventoryLedger {
    /// Cancels unsent transactions whose exact source was authoritatively rewritten.
    pub(super) fn retire_legacy_write(&mut self, cell: Cell) {
        let client_authority = self.authority == Some(protocol::InventoryAuthority::Client);
        let cancels = |request: &PendingRequest| {
            request.state == InventoryPendingState::AwaitingTransport
                && request.touches(cell)
                && (client_authority || request.world_drop.is_some())
        };
        if self.queue.iter().any(cancels) {
            self.abandon_requests(cancels);
        }
    }

    /// Retires obsolete transactions and rollback sources before a server writes this cell.
    pub(super) fn prepare_authoritative_cell(&mut self, cell: Cell) {
        self.retire_legacy_write(cell);
        self.invalidate_request_sources(cell, i32::MIN);
    }

    /// Writes backing authority and records which personal observation cells changed.
    pub(super) fn set_authoritative_cell(&mut self, cell: Cell, held: Option<Held>) -> bool {
        self.prepare_authoritative_cell(cell);
        let written = self.confirmed.set(cell, held);
        if written {
            self.offhand_written |= cell == Cell::Offhand;
            if let Some(index) = cell.personal_index() {
                self.personal_writes |= 1 << index;
            }
            self.note_observed_gear(cell);
        }
        written
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
        normal_inventory_transaction_packet(self.legacy_changes(request, None)?)
            .map(Some)
            .map_err(|_| InventoryGestureError::InvalidRequest)
    }

    /// A drop key press as the native client sends it even to a server that owns the
    /// inventory: a normal transaction whose remainder carries the drop's legacy id.
    pub(super) fn world_drop_packet(
        &self,
        request: &PendingRequest,
    ) -> Result<Packet, InventoryGestureError> {
        let Some(Cell::Inventory(slot)) = request.predicted.first().map(|p| p.cell) else {
            return Err(InventoryGestureError::InvalidRequest);
        };
        let remainder = request.predicted[0].held.as_ref().map_or(0, |_| {
            request.world_drop.expect("world drop").legacy_request_id
        });
        let changes = self.legacy_changes(request, Some(remainder))?;
        inventory_drop_transaction_packet(slot, (remainder != 0).then_some(remainder), changes)
            .map_err(|_| InventoryGestureError::InvalidRequest)
    }

    /// The transaction legs of `request`: each predicted cell's old and new stack, its new
    /// stacks renamed `stamp` when given, and a world leg for each drop.
    fn legacy_changes(
        &self,
        request: &PendingRequest,
        stamp: Option<i32>,
    ) -> Result<Vec<NormalInventoryChange>, InventoryGestureError> {
        let mut changes = Vec::with_capacity(request.predicted.len() + request.actions.len());
        for prediction in &request.predicted {
            let (window_id, slot) = self.legacy_cell_address(prediction.cell)?;
            changes.push(NormalInventoryChange {
                source: NormalInventorySource::Container(window_id as i8),
                slot: u32::from(slot),
                from: descriptor(prediction.before.as_ref(), None)?,
                to: descriptor(prediction.held.as_ref(), stamp)?,
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
                let mut dropped = request
                    .predicted
                    .iter()
                    .find(|prediction| prediction.cell == cell)
                    .and_then(|prediction| prediction.before.clone())
                    .ok_or(InventoryGestureError::InvalidRequest)?;
                dropped.stack.count = u16::from(*amount);
                changes.push(NormalInventoryChange {
                    source: NormalInventorySource::World {
                        randomly: *randomly,
                    },
                    slot: 0,
                    from: verified(None)?,
                    to: descriptor(Some(&dropped), None)?,
                });
            }
        }
        Ok(changes)
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

    /// Commits a normal drop in place, retaining even empty results as supersession history.
    pub(super) fn commit_world_drop(&mut self, index: usize) -> bool {
        let request = &mut self.queue[index];
        let remainder = request.world_drop.expect("world drop").legacy_request_id;
        request.state = InventoryPendingState::AwaitingResponse;
        request.transport_deadline_millis = None;
        // Admission transfers this player's transaction to gameplay backing.
        request.storage_generation = None;
        request.storage_identity = None;
        request.personal_generation = None;
        let request_id = request.request_id;
        let prediction = &mut request.predicted[0];
        let from = prediction.held.clone();
        if let Some(held) = &mut prediction.held {
            held.stack.stack_network_id = remainder;
        }
        let cell = prediction.cell;
        let to = prediction.held.clone();
        self.confirmed.set(cell, to.clone());
        self.rebase_predictions(
            request_id,
            cell,
            from.as_ref(),
            to,
            RollbackAuthority::Known,
        );
        self.refold();
        self.finish_closing();
        true
    }

    /// Publishes admitted consumption or book initialization before exact drop descriptors are built.
    /// The caller's slot revision and source must still match the backing inventory.
    pub fn settle_use_slot_change(
        &mut self,
        slot: u8,
        revision: u64,
        change: &protocol::PredictedSlotChange,
    ) -> bool {
        if change.from.count() == change.to.count() {
            return self.settle_book_init_slot_change(slot, revision, change);
        }
        let cell = Cell::Inventory(slot);
        if self.authoritative_slot_revision(slot) != Some(revision)
            || !protocol::is_legacy_request_id(change.legacy_request_id)
            || change.to != change.from.less_one(change.legacy_request_id)
        {
            return false;
        }
        let Some(before) = self.confirmed.get(cell).cloned() else {
            return false;
        };
        if verified(Some(&before)).ok().as_ref() != Some(&change.from) {
            return false;
        }
        self.consume_retained_source(cell, &before, change.legacy_request_id);
        true
    }

    /// Settles the current writable book's empty-pages update, preserving sparse screen ownership.
    fn settle_book_init_slot_change(
        &mut self,
        slot: u8,
        revision: u64,
        change: &protocol::PredictedSlotChange,
    ) -> bool {
        let cell = Cell::Inventory(slot);
        if self.authoritative_slot_revision(slot) != Some(revision)
            || !protocol::is_legacy_request_id(change.legacy_request_id)
            || self.slot_backing_transport_pending(slot)
            || self
                .negotiated_item_entry(change.from.network_id())
                .and_then(|entry| {
                    assets::facts::ItemFacts::builtin()
                        .ok()?
                        .book_editable(&entry.identifier)
                })
                != Some(true)
            || protocol::writable_book_init_change(&change.from, change.legacy_request_id).as_ref()
                != Some(change)
        {
            return false;
        }
        let Some(mut held) = self.confirmed.get(cell).cloned() else {
            return false;
        };
        if verified(Some(&held)).ok().as_ref() != Some(&change.from) {
            return false;
        }
        let before = held.clone();
        held.stack.extra_data = change.to.extra_data().into();
        held.stack.nbt_digest = change.to.nbt_digest();
        self.confirmed.set(cell, Some(held.clone()));
        self.rebase_predictions(
            i32::MAX,
            cell,
            Some(&before),
            Some(held),
            RollbackAuthority::Known,
        );
        self.refold();
        true
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
            for prediction in &request.predicted {
                self.confirmed.set(prediction.cell, prediction.held.clone());
                self.rebase_predictions(
                    request.request_id,
                    prediction.cell,
                    prediction.held.as_ref(),
                    prediction.held.clone(),
                    RollbackAuthority::Known,
                );
            }
        }
        self.finish_request_retirement(request.request_id, false);
        self.refold();
        self.finish_closing();
        true
    }
}

/// `held` as a transaction leg named `name`, else by its own id, which may be a pending
/// request's or a sent drop's rather than a server stack id.
fn descriptor(
    held: Option<&Held>,
    name: Option<i32>,
) -> Result<VerifiedNetworkItemStack, InventoryGestureError> {
    let Some(held) = held else {
        return verified(None);
    };
    let name = name.unwrap_or(held.stack.stack_network_id);
    if name >= -1 && name == held.stack.stack_network_id {
        return verified(Some(held));
    }
    // Validate the retained item bytes before applying a pending request's client-side name.
    let mut unnamed = held.clone();
    unnamed.stack.stack_network_id = -1;
    Ok(verified(Some(&unnamed))?.renamed(name))
}

fn verified(held: Option<&Held>) -> Result<VerifiedNetworkItemStack, InventoryGestureError> {
    let stack = held.map_or_else(NetworkItemStack::empty, |held| held.stack.clone());
    let digest = stack.nbt_digest;
    VerifiedNetworkItemStack::try_new(stack, digest)
        .map_err(|_| InventoryGestureError::InvalidRequest)
}
