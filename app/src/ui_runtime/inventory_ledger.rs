//! Server-authoritative player-inventory gestures.
//!
//! This first tranche deliberately owns one request at a time. It predicts only
//! the two touched cells and never queues a second gesture behind an in-flight
//! request.

mod helpers;
mod response;

pub use response::StackResponseOverlay;

use helpers::{
    cell_surface, request_slot, storage_slot_identity_matches, valid_raw_window_id,
    valid_storage_window_id,
};

use protocol::{
    ContainerIdentity, InventoryAuthority, InventoryEvent, NetworkItemStack, Packet,
    StackRequestAction, StackRequestContainer, StackRequestSlot, container_close_packet,
    item_stack_request_packet,
};
use thiserror::Error;

pub const PLAYER_INVENTORY_SLOT_COUNT: usize = 36;
pub const INVENTORY_REQUEST_TIMEOUT_MILLIS: u64 = 1_500;
pub const GENERIC_STORAGE_SLOT_TYPE: u8 = 7;
pub const GENERIC_STORAGE_WINDOW_TYPE: i8 = 0;
pub const SMALL_STORAGE_SLOT_COUNT: usize = 27;
pub const LARGE_STORAGE_SLOT_COUNT: usize = 54;

#[derive(Debug, Clone, Copy, Eq, PartialEq)]
pub enum InventoryPendingState {
    AwaitingTransport,
    AwaitingResponse,
}

/// The current known ledger state for one player-inventory slot.
#[derive(Debug, Clone, Copy, Eq, PartialEq)]
pub enum PlayerInventorySlot<'a> {
    /// No server inventory update has established this slot yet.
    Unknown,
    /// The current authoritative or predicted slot contains no item.
    Empty,
    /// The exact authoritative or predicted stack currently present in this slot.
    Present(&'a NetworkItemStack),
}
#[derive(Debug, Clone, Copy, Eq, PartialEq)]
enum Cell {
    Inventory(u8),
    Storage(u8),
    Cursor,
}

#[derive(Debug, Clone)]
struct StorageWindow {
    window_id: i32,
    generation: u64,
    identity: Option<ContainerIdentity>,
    slots: Vec<Option<NetworkItemStack>>,
    revisions: Vec<u64>,
    overlays: Vec<Option<StackResponseOverlay>>,
    resync_required: bool,
    /// Set by a local close while one admitted prediction still awaits its
    /// response, retaining the generation and journal it reconciles against.
    closing: bool,
}

#[derive(Debug, Clone, Copy)]
struct PendingClose {
    window_id: i32,
    window_type: i8,
}

#[derive(Debug, Clone, Copy, Eq, PartialEq)]
enum CellSurface {
    Player,
    Storage,
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
    /// The response overlay travelling with each predicted half so a moved
    /// stack keeps its retained identity until the server restates it.
    source_overlay: Option<StackResponseOverlay>,
    destination_overlay: Option<StackResponseOverlay>,
}

#[derive(Debug, Clone)]
struct PendingRequest {
    request_id: i32,
    action: StackRequestAction,
    prediction: Prediction,
    state: InventoryPendingState,
    transport_deadline_millis: Option<u64>,
    deadline_millis: Option<u64>,
    session_generation: u64,
    storage_generation: Option<u64>,
    storage_identity: Option<ContainerIdentity>,
}

#[derive(Debug, Clone, Copy, Eq, PartialEq, Error)]
pub enum InventoryGestureError {
    #[error("server-authoritative inventory is not active")]
    AuthorityUnavailable,
    #[error("player inventory slot {0} is outside 0..36")]
    InvalidSlot(u8),
    #[error("generic storage slot {0} is outside the authoritative window")]
    InvalidStorageSlot(u8),
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
    slot_overlays: [Option<StackResponseOverlay>; PLAYER_INVENTORY_SLOT_COUNT],
    cursor: Option<NetworkItemStack>,
    cursor_overlay: Option<StackResponseOverlay>,
    cursor_revision: u64,
    next_authority_revision: u64,
    pending: Option<PendingRequest>,
    next_request_id: i32,
    session_generation: u64,
    next_open_generation: u64,
    storage: Option<StorageWindow>,
    pending_close: Option<PendingClose>,
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
            slot_overlays: std::array::from_fn(|_| None),
            cursor: None,
            cursor_overlay: None,
            cursor_revision: 0,
            next_authority_revision: 1,
            pending: None,
            next_request_id: -3,
            session_generation: 0,
            next_open_generation: 1,
            storage: None,
            pending_close: None,
            player_resync_required: false,
            cursor_resync_required: false,
        }
    }
}

impl PlayerInventoryLedger {
    pub fn begin_session(&mut self, session_generation: u64) {
        *self = Self {
            session_generation,
            ..Self::default()
        };
    }

    /// Returns whether one current player-inventory slot is unknown, empty, or present.
    /// `None` means the requested slot is outside the player inventory.
    #[must_use]
    pub fn slot_state(&self, slot: u8) -> Option<PlayerInventorySlot<'_>> {
        let index = usize::from(slot);
        if !*self.known.get(index)? {
            return Some(PlayerInventorySlot::Unknown);
        }
        let cell = Cell::Inventory(slot);
        Some(
            match self.predicted_cell(cell).unwrap_or_else(|| self.cell(cell)) {
                Some(stack) => PlayerInventorySlot::Present(stack),
                None => PlayerInventorySlot::Empty,
            },
        )
    }

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
    pub fn storage_identity(&self) -> Option<ContainerIdentity> {
        self.storage.as_ref()?.identity
    }

    #[must_use]
    pub fn storage_generation(&self) -> Option<u64> {
        Some(self.storage.as_ref()?.generation)
    }

    #[must_use]
    pub fn storage_slot_count(&self) -> Option<usize> {
        let storage = self.storage.as_ref()?;
        storage.identity.map(|_| storage.slots.len())
    }

    #[must_use]
    pub fn storage_stack(&self, slot: u8) -> Option<&NetworkItemStack> {
        let cell = Cell::Storage(slot);
        self.predicted_cell(cell)
            .unwrap_or_else(|| self.cell(cell))
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
    pub fn storage_slot_pending(&self, slot: u8) -> bool {
        let cell = Cell::Storage(slot);
        self.pending.as_ref().is_some_and(|pending| {
            pending.prediction.source == cell || pending.prediction.destination == cell
        })
    }

    #[must_use]
    pub fn resync_required(&self) -> bool {
        self.player_resync_required
            || self.cursor_resync_required
            || self
                .storage
                .as_ref()
                .is_some_and(|storage| storage.resync_required)
    }

    pub fn begin_click(&mut self, slot: u8) -> Result<i32, InventoryGestureError> {
        self.begin_cell_click(Cell::Inventory(slot))
    }

    pub fn begin_storage_click(&mut self, slot: u8) -> Result<i32, InventoryGestureError> {
        self.begin_cell_click(Cell::Storage(slot))
    }

    fn begin_cell_click(&mut self, target: Cell) -> Result<i32, InventoryGestureError> {
        if self.authority != Some(InventoryAuthority::Server) {
            return Err(InventoryGestureError::AuthorityUnavailable);
        }
        if self.resync_required() {
            return Err(InventoryGestureError::ResyncRequired);
        }
        if self.pending.is_some() {
            return Err(InventoryGestureError::Busy);
        }
        let (target_stack, target_revision) = match target {
            Cell::Inventory(slot) => {
                let index = usize::from(slot);
                if index >= PLAYER_INVENTORY_SLOT_COUNT {
                    return Err(InventoryGestureError::InvalidSlot(slot));
                }
                if !self.known[index] {
                    return Err(InventoryGestureError::UnknownSlot(slot));
                }
                (self.slots[index].clone(), self.slot_revisions[index])
            }
            Cell::Storage(slot) => {
                let storage = self
                    .storage
                    .as_ref()
                    .ok_or(InventoryGestureError::InvalidStorageSlot(slot))?;
                if storage.identity.is_none() || storage.resync_required {
                    return Err(InventoryGestureError::ResyncRequired);
                }
                let index = usize::from(slot);
                if index >= storage.slots.len() {
                    return Err(InventoryGestureError::InvalidStorageSlot(slot));
                }
                (storage.slots[index].clone(), storage.revisions[index])
            }
            Cell::Cursor => unreachable!("cursor is not a click target"),
        };
        let inventory = target_stack
            .as_ref()
            .filter(|stack| !stack.is_empty())
            .cloned();
        let cursor = self
            .cursor
            .as_ref()
            .filter(|stack| !stack.is_empty())
            .cloned();
        let inventory_cell = target;
        let inventory_revision = target_revision;
        let cursor_revision = self.cell_revision(Cell::Cursor);
        let storage_identity = self.storage_identity();
        let target_overlay = self.cell_overlay(inventory_cell).cloned();
        let cursor_overlay = self.cell_overlay(Cell::Cursor).cloned();
        let (action, prediction) = match (inventory, cursor) {
            (Some(stack), None) => {
                let amount = u8::try_from(stack.count)
                    .ok()
                    .filter(|amount| *amount != 0)
                    .ok_or(InventoryGestureError::InvalidRequest)?;
                (
                    StackRequestAction::Take {
                        amount,
                        source: request_slot(
                            inventory_cell,
                            stack.stack_network_id,
                            storage_identity,
                        )?,
                        destination: request_slot(Cell::Cursor, -1, storage_identity)?,
                    },
                    Prediction {
                        source: inventory_cell,
                        source_stack: None,
                        source_revision: inventory_revision,
                        destination: Cell::Cursor,
                        destination_stack: Some(stack),
                        destination_revision: cursor_revision,
                        source_overlay: None,
                        destination_overlay: target_overlay,
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
                        source: request_slot(
                            Cell::Cursor,
                            stack.stack_network_id,
                            storage_identity,
                        )?,
                        destination: request_slot(inventory_cell, -1, storage_identity)?,
                    },
                    Prediction {
                        source: Cell::Cursor,
                        source_stack: None,
                        source_revision: cursor_revision,
                        destination: inventory_cell,
                        destination_stack: Some(stack),
                        destination_revision: inventory_revision,
                        source_overlay: None,
                        destination_overlay: cursor_overlay,
                    },
                )
            }
            (Some(inventory), Some(cursor)) => (
                StackRequestAction::Swap {
                    source: request_slot(Cell::Cursor, cursor.stack_network_id, storage_identity)?,
                    destination: request_slot(
                        inventory_cell,
                        inventory.stack_network_id,
                        storage_identity,
                    )?,
                },
                Prediction {
                    source: Cell::Cursor,
                    source_stack: Some(inventory),
                    source_revision: cursor_revision,
                    destination: inventory_cell,
                    destination_stack: Some(cursor),
                    destination_revision: inventory_revision,
                    source_overlay: target_overlay,
                    destination_overlay: cursor_overlay,
                },
            ),
            (None, None) => return Err(InventoryGestureError::EmptyGesture),
        };
        let request_id = self.next_request_id;
        self.next_request_id = self
            .next_request_id
            .checked_sub(2)
            .ok_or(InventoryGestureError::InvalidRequest)?;
        self.pending = Some(PendingRequest {
            request_id,
            action,
            prediction,
            state: InventoryPendingState::AwaitingTransport,
            transport_deadline_millis: None,
            deadline_millis: None,
            session_generation: self.session_generation,
            storage_generation: self.storage.as_ref().map(|storage| storage.generation),
            storage_identity: self.storage.as_ref().and_then(|storage| storage.identity),
        });
        Ok(request_id)
    }

    pub fn pending_packet(&self) -> Result<Option<Packet>, InventoryGestureError> {
        if let Some(close) = self.pending_close {
            return container_close_packet(close.window_id, close.window_type)
                .map(Some)
                .map_err(|_| InventoryGestureError::InvalidRequest);
        }
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
        if self.pending_close.take().is_some() {
            return true;
        }
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
        if self.pending_close.is_some() {
            return;
        }
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
        self.pending_close = None;
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
                    self.cursor_overlay = None;
                    self.player_resync_required = false;
                    self.cursor_resync_required = false;
                    self.storage = None;
                    self.pending_close = None;
                }
            }
            InventoryEvent::Open(open) => self.apply_open(*open),
            InventoryEvent::Close(close) => {
                if self.pending_close.is_some_and(|pending| {
                    close.container.window_id == Some(pending.window_id)
                        && close.window_type == pending.window_type
                }) {
                    self.pending_close = None;
                }
                if self.storage.as_ref().is_some_and(|storage| {
                    close.container.window_id == Some(storage.window_id)
                        && close.window_type == GENERIC_STORAGE_WINDOW_TYPE
                }) {
                    self.close_storage(false);
                }
            }
            InventoryEvent::Content(content)
                if content.container.slot_type == Some(GENERIC_STORAGE_SLOT_TYPE) =>
            {
                self.apply_storage_content(content.container, &content.slots);
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
                    self.slot_overlays[index] = None;
                    self.known[index] = true;
                    self.slot_revisions[index] = revision;
                }
                if complete {
                    self.player_resync_required = false;
                    self.cancel_pending_for_authority(CellSurface::Player, None);
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
                self.cursor_overlay = None;
                self.cursor_revision = self.take_authority_revision();
                self.cursor_resync_required = false;
                self.cancel_pending_for_authority(CellSurface::Cursor, None);
            }
            InventoryEvent::Slot(update)
                if update.identity.container.window_id == Some(0)
                    && update.identity.container.slot_type != Some(59)
                    && usize::from(update.identity.slot) < PLAYER_INVENTORY_SLOT_COUNT =>
            {
                let index = usize::from(update.identity.slot);
                self.slots[index] = (!update.stack.is_empty()).then(|| update.stack.clone());
                self.slot_overlays[index] = None;
                self.known[index] = true;
                let revision = self.take_authority_revision();
                self.slot_revisions[index] = revision;
            }
            InventoryEvent::Slot(update)
                if update.identity.container.slot_type == Some(59) && update.identity.slot == 0 =>
            {
                self.cursor = (!update.stack.is_empty()).then(|| update.stack.clone());
                self.cursor_overlay = None;
                self.cursor_revision = self.take_authority_revision();
                self.cursor_resync_required = false;
            }
            InventoryEvent::Slot(update)
                if self.storage_slot_identity_matches(update.identity.container) =>
            {
                self.apply_storage_slot(
                    update.identity.container,
                    update.identity.slot,
                    &update.stack,
                );
            }
            InventoryEvent::Response(event) => self.apply_response(event),
            _ => {}
        }
    }

    fn rollback_pending(&mut self) {
        self.pending = None;
        self.finish_closing();
    }

    fn require_authoritative_recovery(&mut self) {
        if let Some(pending) = self.pending.take() {
            self.mark_cell_recovery(pending.prediction.source);
            self.mark_cell_recovery(pending.prediction.destination);
        }
        self.finish_closing();
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

    /// The travelling overlay of the predicted half occupying `cell`, or
    /// `None` when no in-flight gesture touches that cell.
    fn predicted_cell_overlay(&self, cell: Cell) -> Option<Option<&StackResponseOverlay>> {
        let prediction = &self.pending.as_ref()?.prediction;
        if prediction.source == cell {
            Some(prediction.source_overlay.as_ref())
        } else if prediction.destination == cell {
            Some(prediction.destination_overlay.as_ref())
        } else {
            None
        }
    }

    fn cell(&self, cell: Cell) -> Option<&NetworkItemStack> {
        match cell {
            Cell::Inventory(slot) => self.slots.get(usize::from(slot))?.as_ref(),
            Cell::Storage(slot) => self
                .storage
                .as_ref()?
                .slots
                .get(usize::from(slot))?
                .as_ref(),
            Cell::Cursor => self.cursor.as_ref(),
        }
    }

    fn cell_mut(&mut self, cell: Cell) -> Option<&mut NetworkItemStack> {
        match cell {
            Cell::Inventory(slot) => self.slots.get_mut(usize::from(slot))?.as_mut(),
            Cell::Storage(slot) => self
                .storage
                .as_mut()?
                .slots
                .get_mut(usize::from(slot))?
                .as_mut(),
            Cell::Cursor => self.cursor.as_mut(),
        }
    }

    fn set_cell(&mut self, cell: Cell, stack: Option<NetworkItemStack>) {
        self.clear_cell_overlay(cell);
        match cell {
            Cell::Inventory(slot) => self.slots[usize::from(slot)] = stack,
            Cell::Storage(slot) => {
                self.storage.as_mut().expect("validated storage").slots[usize::from(slot)] = stack
            }
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
            Cell::Storage(slot) => self
                .storage
                .as_ref()
                .and_then(|storage| storage.revisions.get(usize::from(slot)))
                .copied()
                .unwrap_or(0),
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
            Cell::Storage(slot) => {
                if let Some(current) = self
                    .storage
                    .as_mut()
                    .and_then(|storage| storage.revisions.get_mut(usize::from(slot)))
                {
                    *current = revision;
                }
            }
        }
    }

    fn take_authority_revision(&mut self) -> u64 {
        let revision = self.next_authority_revision;
        self.next_authority_revision = self.next_authority_revision.wrapping_add(1).max(1);
        revision
    }

    fn apply_open(&mut self, open: protocol::ContainerOpenEvent) {
        if self
            .pending
            .as_ref()
            .is_some_and(|pending| pending.storage_generation.is_some())
        {
            if self.pending_state() == Some(InventoryPendingState::AwaitingResponse) {
                self.require_authoritative_recovery();
            } else {
                self.rollback_pending();
            }
        }
        let Some(window_id) = open.container.window_id else {
            return;
        };
        if open.window_type != GENERIC_STORAGE_WINDOW_TYPE || !valid_storage_window_id(window_id) {
            self.queue_close(window_id, open.window_type);
            self.storage = None;
            return;
        }
        if self.pending_close.is_some_and(|close| {
            close.window_id == window_id && close.window_type == open.window_type
        }) {
            self.pending_close = None;
        }
        let generation = self.next_open_generation;
        self.next_open_generation = self.next_open_generation.wrapping_add(1).max(1);
        self.storage = Some(StorageWindow {
            window_id,
            generation,
            identity: None,
            slots: Vec::new(),
            revisions: Vec::new(),
            overlays: Vec::new(),
            resync_required: false,
            closing: false,
        });
    }

    fn apply_storage_content(&mut self, identity: ContainerIdentity, slots: &[NetworkItemStack]) {
        let valid_len = matches!(
            slots.len(),
            SMALL_STORAGE_SLOT_COUNT | LARGE_STORAGE_SLOT_COUNT
        );
        let Some(storage) = self.storage.as_ref() else {
            return;
        };
        let window_id = storage.window_id;
        let generation = storage.generation;
        if identity.window_id != Some(window_id) {
            return;
        }
        if storage.identity.is_some_and(|current| current != identity) {
            return;
        }
        if !valid_len {
            self.queue_close(window_id, GENERIC_STORAGE_WINDOW_TYPE);
            self.close_storage(false);
            return;
        }
        let revision = self.take_authority_revision();
        let storage = self.storage.as_mut().expect("storage remains active");
        storage.identity = Some(identity);
        storage.slots = slots
            .iter()
            .map(|stack| (!stack.is_empty()).then(|| stack.clone()))
            .collect();
        storage.revisions = vec![revision; slots.len()];
        storage.overlays = vec![None; slots.len()];
        storage.resync_required = false;
        self.cancel_pending_for_authority(CellSurface::Storage, Some(generation));
    }

    fn apply_storage_slot(
        &mut self,
        identity: ContainerIdentity,
        slot: u16,
        stack: &NetworkItemStack,
    ) {
        let Some(storage) = self.storage.as_ref() else {
            return;
        };
        if !storage_slot_identity_matches(storage, identity)
            || usize::from(slot) >= storage.slots.len()
        {
            return;
        }
        let revision = self.take_authority_revision();
        let storage = self.storage.as_mut().expect("storage remains active");
        storage.slots[usize::from(slot)] = (!stack.is_empty()).then(|| stack.clone());
        storage.revisions[usize::from(slot)] = revision;
        storage.overlays[usize::from(slot)] = None;
    }

    fn pending_identity_mismatch(&self, identity: ContainerIdentity) -> bool {
        self.pending
            .as_ref()
            .and_then(|pending| pending.storage_identity)
            .is_none_or(|expected| {
                identity.window_id.is_some()
                    || expected.slot_type != identity.slot_type
                    || expected.dynamic_id != identity.dynamic_id
            })
    }

    pub fn request_storage_close(&mut self) {
        let Some(storage) = self.storage.as_ref() else {
            return;
        };
        if storage.closing {
            // A prior local close is already waiting out its in-flight
            // prediction; further close gestures stay blocked until
            // authority settles the retained window.
            return;
        }
        let (window_id, generation) = (storage.window_id, storage.generation);
        self.queue_close(window_id, GENERIC_STORAGE_WINDOW_TYPE);
        let awaiting_response = self.pending.as_ref().is_some_and(|pending| {
            pending.state == InventoryPendingState::AwaitingResponse
                && pending.storage_generation == Some(generation)
        });
        if awaiting_response {
            // Retain the window so the outstanding response still
            // reconciles against its exact generation and identity.
            self.storage
                .as_mut()
                .expect("storage observed above")
                .closing = true;
        } else {
            self.close_storage(true);
        }
    }

    /// Settles a locally requested close whose retained prediction is gone.
    ///
    /// The closing window survives exactly until that prediction resolves,
    /// an authoritative close lands, or an existing timeout or recovery path
    /// consumes it, so it can never outlive the ledger's timeout authority.
    /// Settlement drops the generation and journal exactly like an immediate
    /// close, including held-cursor restatement.
    fn finish_closing(&mut self) {
        if !self.storage.as_ref().is_some_and(|storage| storage.closing) || self.pending.is_some() {
            return;
        }
        self.storage = None;
        if self.cursor.as_ref().is_some_and(|stack| !stack.is_empty()) {
            self.player_resync_required = true;
            self.cursor_resync_required = true;
        }
    }

    fn close_storage(&mut self, local: bool) {
        let storage_generation = self.storage.as_ref().map(|storage| storage.generation);
        if self
            .pending
            .as_ref()
            .is_some_and(|pending| pending.storage_generation == storage_generation)
        {
            if local && self.pending_state() == Some(InventoryPendingState::AwaitingTransport) {
                self.rollback_pending();
            } else {
                self.cancel_pending_for_authority(CellSurface::Storage, storage_generation);
            }
        }
        self.storage = None;
        if self.cursor.as_ref().is_some_and(|stack| !stack.is_empty()) {
            self.player_resync_required = true;
            self.cursor_resync_required = true;
        }
    }

    fn queue_close(&mut self, window_id: i32, window_type: i8) {
        if valid_raw_window_id(window_id) {
            self.pending_close = Some(PendingClose {
                window_id,
                window_type,
            });
        }
    }

    fn storage_slot_identity_matches(&self, identity: ContainerIdentity) -> bool {
        self.storage
            .as_ref()
            .is_some_and(|storage| storage_slot_identity_matches(storage, identity))
    }

    fn cancel_pending_for_authority(
        &mut self,
        confirmed: CellSurface,
        storage_generation: Option<u64>,
    ) {
        if storage_generation.is_some()
            && self
                .pending
                .as_ref()
                .and_then(|pending| pending.storage_generation)
                != storage_generation
        {
            return;
        }
        let Some(pending) = self.pending.take() else {
            self.finish_closing();
            return;
        };
        if pending.state != InventoryPendingState::AwaitingResponse {
            self.finish_closing();
            return;
        }
        for cell in [pending.prediction.source, pending.prediction.destination] {
            if cell_surface(cell) != confirmed {
                self.mark_cell_recovery(cell);
            }
        }
        self.finish_closing();
    }

    fn mark_cell_recovery(&mut self, cell: Cell) {
        self.clear_cell_overlay(cell);
        match cell_surface(cell) {
            CellSurface::Player => self.player_resync_required = true,
            CellSurface::Cursor => self.cursor_resync_required = true,
            CellSurface::Storage => {
                if let Some(storage) = self.storage.as_mut() {
                    storage.resync_required = true;
                }
            }
        }
    }
}
