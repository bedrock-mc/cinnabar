//! Server-authoritative player-inventory gestures.
//!
//! This first tranche deliberately owns one request at a time. It predicts only
//! the two touched cells and never queues a second gesture behind an in-flight
//! request.

use protocol::{
    InventoryAuthority, InventoryEvent, ItemStackResponseEvent, NetworkItemStack, Packet,
    StackRequestAction, StackRequestContainer, StackRequestSlot, StackResponseStatus,
    item_stack_request_packet,
};
use thiserror::Error;

pub const PLAYER_INVENTORY_SLOT_COUNT: usize = 36;
pub const INVENTORY_REQUEST_TIMEOUT_MILLIS: u64 = 1_500;

#[derive(Debug, Clone, Copy, Eq, PartialEq)]
pub enum InventoryPendingState {
    AwaitingTransport,
    AwaitingResponse,
}

#[derive(Debug, Clone, Copy, Eq, PartialEq)]
enum Cell {
    Inventory(u8),
    Cursor,
}

#[derive(Debug, Clone)]
struct Prediction {
    source: Cell,
    source_stack: Option<NetworkItemStack>,
    source_revision: u64,
    destination: Cell,
    destination_stack: Option<NetworkItemStack>,
    destination_revision: u64,
}

#[derive(Debug, Clone)]
struct PendingRequest {
    request_id: i32,
    action: StackRequestAction,
    prediction: Prediction,
    state: InventoryPendingState,
    transport_deadline_millis: Option<u64>,
    deadline_millis: Option<u64>,
}

#[derive(Debug, Clone, Copy, Eq, PartialEq, Error)]
pub enum InventoryGestureError {
    #[error("server-authoritative inventory is not active")]
    AuthorityUnavailable,
    #[error("player inventory slot {0} is outside 0..36")]
    InvalidSlot(u8),
    #[error("player inventory slot {0} is not known yet")]
    UnknownSlot(u8),
    #[error("an inventory request is already in flight")]
    Busy,
    #[error("both the cursor and selected inventory slot are empty")]
    EmptyGesture,
    #[error("inventory is waiting for an authoritative resync")]
    ResyncRequired,
    #[error("the retained inventory request is invalid")]
    InvalidRequest,
}

#[derive(Debug, Clone)]
pub struct PlayerInventoryLedger {
    authority: Option<InventoryAuthority>,
    slots: [Option<NetworkItemStack>; PLAYER_INVENTORY_SLOT_COUNT],
    known: [bool; PLAYER_INVENTORY_SLOT_COUNT],
    slot_revisions: [u64; PLAYER_INVENTORY_SLOT_COUNT],
    cursor: Option<NetworkItemStack>,
    cursor_revision: u64,
    next_authority_revision: u64,
    pending: Option<PendingRequest>,
    next_request_id: i32,
    player_resync_required: bool,
    cursor_resync_required: bool,
}

impl Default for PlayerInventoryLedger {
    fn default() -> Self {
        Self {
            authority: None,
            slots: std::array::from_fn(|_| None),
            known: [false; PLAYER_INVENTORY_SLOT_COUNT],
            slot_revisions: [0; PLAYER_INVENTORY_SLOT_COUNT],
            cursor: None,
            cursor_revision: 0,
            next_authority_revision: 1,
            pending: None,
            next_request_id: 1,
            player_resync_required: false,
            cursor_resync_required: false,
        }
    }
}

impl PlayerInventoryLedger {
    #[must_use]
    pub fn displayed_stack(&self, slot: u8) -> Option<&NetworkItemStack> {
        if usize::from(slot) >= PLAYER_INVENTORY_SLOT_COUNT {
            return None;
        }
        let cell = Cell::Inventory(slot);
        self.predicted_cell(cell)
            .unwrap_or_else(|| self.cell(cell))
            .filter(|stack| !stack.is_empty())
    }

    #[must_use]
    pub fn cursor_stack(&self) -> Option<&NetworkItemStack> {
        self.predicted_cell(Cell::Cursor)
            .unwrap_or_else(|| self.cell(Cell::Cursor))
            .filter(|stack| !stack.is_empty())
    }

    #[must_use]
    pub fn pending_state(&self) -> Option<InventoryPendingState> {
        self.pending.as_ref().map(|pending| pending.state)
    }

    #[must_use]
    pub fn pending_request_id(&self) -> Option<i32> {
        self.pending.as_ref().map(|pending| pending.request_id)
    }

    #[must_use]
    pub fn slot_pending(&self, slot: u8) -> bool {
        let cell = Cell::Inventory(slot);
        self.pending.as_ref().is_some_and(|pending| {
            pending.prediction.source == cell || pending.prediction.destination == cell
        })
    }

    #[must_use]
    pub const fn resync_required(&self) -> bool {
        self.player_resync_required || self.cursor_resync_required
    }

    pub fn begin_click(&mut self, slot: u8) -> Result<i32, InventoryGestureError> {
        if self.authority != Some(InventoryAuthority::Server) {
            return Err(InventoryGestureError::AuthorityUnavailable);
        }
        if self.resync_required() {
            return Err(InventoryGestureError::ResyncRequired);
        }
        if self.pending.is_some() {
            return Err(InventoryGestureError::Busy);
        }
        let index = usize::from(slot);
        if index >= PLAYER_INVENTORY_SLOT_COUNT {
            return Err(InventoryGestureError::InvalidSlot(slot));
        }
        if !self.known[index] {
            return Err(InventoryGestureError::UnknownSlot(slot));
        }
        let inventory = self.slots[index]
            .as_ref()
            .filter(|stack| !stack.is_empty())
            .cloned();
        let cursor = self
            .cursor
            .as_ref()
            .filter(|stack| !stack.is_empty())
            .cloned();
        let inventory_cell = Cell::Inventory(slot);
        let inventory_revision = self.cell_revision(inventory_cell);
        let cursor_revision = self.cell_revision(Cell::Cursor);
        let (action, prediction) = match (inventory, cursor) {
            (Some(stack), None) => {
                let amount = u8::try_from(stack.count)
                    .ok()
                    .filter(|amount| *amount != 0)
                    .ok_or(InventoryGestureError::InvalidRequest)?;
                (
                    StackRequestAction::Take {
                        amount,
                        source: request_slot(inventory_cell, stack.stack_network_id),
                        destination: request_slot(Cell::Cursor, -1),
                    },
                    Prediction {
                        source: inventory_cell,
                        source_stack: None,
                        source_revision: inventory_revision,
                        destination: Cell::Cursor,
                        destination_stack: Some(stack),
                        destination_revision: cursor_revision,
                    },
                )
            }
            (None, Some(stack)) => {
                let amount = u8::try_from(stack.count)
                    .ok()
                    .filter(|amount| *amount != 0)
                    .ok_or(InventoryGestureError::InvalidRequest)?;
                (
                    StackRequestAction::Place {
                        amount,
                        source: request_slot(Cell::Cursor, stack.stack_network_id),
                        destination: request_slot(inventory_cell, -1),
                    },
                    Prediction {
                        source: Cell::Cursor,
                        source_stack: None,
                        source_revision: cursor_revision,
                        destination: inventory_cell,
                        destination_stack: Some(stack),
                        destination_revision: inventory_revision,
                    },
                )
            }
            (Some(inventory), Some(cursor)) => (
                StackRequestAction::Swap {
                    source: request_slot(Cell::Cursor, cursor.stack_network_id),
                    destination: request_slot(inventory_cell, inventory.stack_network_id),
                },
                Prediction {
                    source: Cell::Cursor,
                    source_stack: Some(inventory),
                    source_revision: cursor_revision,
                    destination: inventory_cell,
                    destination_stack: Some(cursor),
                    destination_revision: inventory_revision,
                },
            ),
            (None, None) => return Err(InventoryGestureError::EmptyGesture),
        };
        let request_id = self.next_request_id;
        self.next_request_id = self.next_request_id.checked_add(1).unwrap_or(1);
        self.pending = Some(PendingRequest {
            request_id,
            action,
            prediction,
            state: InventoryPendingState::AwaitingTransport,
            transport_deadline_millis: None,
            deadline_millis: None,
        });
        Ok(request_id)
    }

    pub fn pending_packet(&self) -> Result<Option<Packet>, InventoryGestureError> {
        self.pending
            .as_ref()
            .filter(|pending| pending.state == InventoryPendingState::AwaitingTransport)
            .map(|pending| {
                item_stack_request_packet(pending.request_id, pending.action)
                    .map_err(|_| InventoryGestureError::InvalidRequest)
            })
            .transpose()
    }

    pub fn mark_transport_enqueued(&mut self, now_millis: u64) -> bool {
        let Some(pending) = self.pending.as_mut() else {
            return false;
        };
        if pending.state != InventoryPendingState::AwaitingTransport {
            return false;
        }
        pending.state = InventoryPendingState::AwaitingResponse;
        pending.transport_deadline_millis = None;
        pending.deadline_millis = Some(now_millis.saturating_add(INVENTORY_REQUEST_TIMEOUT_MILLIS));
        true
    }

    pub fn note_transport_pressure(&mut self, now_millis: u64) {
        let Some(pending) = self.pending.as_mut() else {
            return;
        };
        if pending.state != InventoryPendingState::AwaitingTransport {
            return;
        }
        let deadline = pending
            .transport_deadline_millis
            .get_or_insert_with(|| now_millis.saturating_add(INVENTORY_REQUEST_TIMEOUT_MILLIS));
        if now_millis >= *deadline {
            self.rollback_pending();
        }
    }

    /// Fails closed once transport admission no longer proves whether the
    /// server observed the request. Retransmitting an admitted mutation could
    /// apply it twice; retry is limited to pre-admission queue pressure.
    pub fn poll_timeout(&mut self, now_millis: u64) -> bool {
        let Some(pending) = self.pending.as_mut() else {
            return false;
        };
        if pending.state != InventoryPendingState::AwaitingResponse
            || pending
                .deadline_millis
                .is_none_or(|deadline| now_millis < deadline)
        {
            return false;
        }
        self.require_authoritative_recovery();
        false
    }

    pub fn transport_closed(&mut self) {
        match self.pending_state() {
            Some(InventoryPendingState::AwaitingTransport) => self.rollback_pending(),
            Some(InventoryPendingState::AwaitingResponse) => {
                self.require_authoritative_recovery();
            }
            None => {}
        }
    }

    pub fn apply(&mut self, event: &InventoryEvent) {
        match event {
            InventoryEvent::Authority(authority) => {
                self.authority = Some(*authority);
                if *authority != InventoryAuthority::Server {
                    self.pending = None;
                    self.cursor = None;
                    self.player_resync_required = false;
                    self.cursor_resync_required = false;
                }
            }
            InventoryEvent::Content(content)
                if content.container.window_id == Some(0)
                    && content.container.slot_type != Some(59) =>
            {
                let complete = content.slots.len() == PLAYER_INVENTORY_SLOT_COUNT;
                let revision = self.take_authority_revision();
                for index in 0..content.slots.len().min(PLAYER_INVENTORY_SLOT_COUNT) {
                    self.slots[index] = content
                        .slots
                        .get(index)
                        .filter(|stack| !stack.is_empty())
                        .cloned();
                    self.known[index] = true;
                    self.slot_revisions[index] = revision;
                }
                if complete {
                    let admitted =
                        self.pending_state() == Some(InventoryPendingState::AwaitingResponse);
                    self.pending = None;
                    self.player_resync_required = false;
                    if admitted {
                        self.cursor_resync_required = true;
                    }
                }
            }
            InventoryEvent::Content(content)
                if content.container.slot_type == Some(59) && content.slots.len() == 1 =>
            {
                self.cursor = content
                    .slots
                    .first()
                    .filter(|stack| !stack.is_empty())
                    .cloned();
                self.cursor_revision = self.take_authority_revision();
                self.cursor_resync_required = false;
            }
            InventoryEvent::Slot(update)
                if update.identity.container.window_id == Some(0)
                    && update.identity.container.slot_type != Some(59)
                    && usize::from(update.identity.slot) < PLAYER_INVENTORY_SLOT_COUNT =>
            {
                let index = usize::from(update.identity.slot);
                self.slots[index] = (!update.stack.is_empty()).then(|| update.stack.clone());
                self.known[index] = true;
                let revision = self.take_authority_revision();
                self.slot_revisions[index] = revision;
            }
            InventoryEvent::Slot(update)
                if update.identity.container.slot_type == Some(59) && update.identity.slot == 0 =>
            {
                self.cursor = (!update.stack.is_empty()).then(|| update.stack.clone());
                self.cursor_revision = self.take_authority_revision();
                self.cursor_resync_required = false;
            }
            InventoryEvent::Response(event) => self.apply_response(event),
            _ => {}
        }
    }

    fn apply_response(&mut self, event: &ItemStackResponseEvent) {
        let Some(request_id) = self.pending_request_id() else {
            return;
        };
        let Some(response) = event
            .responses
            .iter()
            .find(|response| response.request_id == request_id)
        else {
            return;
        };
        if response.status != StackResponseStatus::Accepted {
            self.rollback_pending();
            return;
        }
        let prediction = &self
            .pending
            .as_ref()
            .expect("the matching pending request was observed")
            .prediction;
        if self.cell_revision(prediction.source) != prediction.source_revision
            || self.cell_revision(prediction.destination) != prediction.destination_revision
        {
            self.require_authoritative_recovery();
            return;
        }
        let prediction = self
            .pending
            .take()
            .expect("the matching pending request was observed")
            .prediction;
        self.set_cell(prediction.source, prediction.source_stack);
        self.set_cell(prediction.destination, prediction.destination_stack);
        self.bump_cell_revision(prediction.source);
        self.bump_cell_revision(prediction.destination);
        for container in response.containers.iter() {
            for correction in container.slots.iter() {
                let cell = match container.container.slot_type {
                    Some(12) if usize::from(correction.slot) < PLAYER_INVENTORY_SLOT_COUNT => {
                        Cell::Inventory(correction.slot)
                    }
                    Some(59) if correction.slot == 0 => Cell::Cursor,
                    _ => continue,
                };
                if correction.count == 0 {
                    self.set_cell(cell, None);
                } else if let Some(stack) = self.cell_mut(cell) {
                    stack.count = u16::from(correction.count);
                    stack.stack_network_id = correction.item_stack_id;
                } else {
                    self.player_resync_required = true;
                    self.cursor_resync_required = true;
                }
                self.bump_cell_revision(cell);
            }
        }
    }

    fn rollback_pending(&mut self) {
        self.pending = None;
    }

    fn require_authoritative_recovery(&mut self) {
        self.pending = None;
        self.player_resync_required = true;
        self.cursor_resync_required = true;
    }

    fn predicted_cell(&self, cell: Cell) -> Option<Option<&NetworkItemStack>> {
        let prediction = &self.pending.as_ref()?.prediction;
        if prediction.source == cell {
            Some(prediction.source_stack.as_ref())
        } else if prediction.destination == cell {
            Some(prediction.destination_stack.as_ref())
        } else {
            None
        }
    }

    fn cell(&self, cell: Cell) -> Option<&NetworkItemStack> {
        match cell {
            Cell::Inventory(slot) => self.slots.get(usize::from(slot))?.as_ref(),
            Cell::Cursor => self.cursor.as_ref(),
        }
    }

    fn cell_mut(&mut self, cell: Cell) -> Option<&mut NetworkItemStack> {
        match cell {
            Cell::Inventory(slot) => self.slots.get_mut(usize::from(slot))?.as_mut(),
            Cell::Cursor => self.cursor.as_mut(),
        }
    }

    fn set_cell(&mut self, cell: Cell, stack: Option<NetworkItemStack>) {
        match cell {
            Cell::Inventory(slot) => self.slots[usize::from(slot)] = stack,
            Cell::Cursor => self.cursor = stack,
        }
    }

    fn cell_revision(&self, cell: Cell) -> u64 {
        match cell {
            Cell::Inventory(slot) => self
                .slot_revisions
                .get(usize::from(slot))
                .copied()
                .unwrap_or(0),
            Cell::Cursor => self.cursor_revision,
        }
    }

    fn bump_cell_revision(&mut self, cell: Cell) {
        let revision = self.take_authority_revision();
        match cell {
            Cell::Inventory(slot) => {
                if let Some(current) = self.slot_revisions.get_mut(usize::from(slot)) {
                    *current = revision;
                }
            }
            Cell::Cursor => self.cursor_revision = revision,
        }
    }

    fn take_authority_revision(&mut self) -> u64 {
        let revision = self.next_authority_revision;
        self.next_authority_revision = self.next_authority_revision.wrapping_add(1).max(1);
        revision
    }
}

fn request_slot(cell: Cell, stack_network_id: i32) -> StackRequestSlot {
    match cell {
        Cell::Inventory(slot) => StackRequestSlot {
            container: StackRequestContainer::PlayerInventory,
            slot,
            stack_network_id,
        },
        Cell::Cursor => StackRequestSlot {
            container: StackRequestContainer::Cursor,
            slot: 0,
            stack_network_id,
        },
    }
}
