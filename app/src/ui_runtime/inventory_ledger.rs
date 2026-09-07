//! Server-authoritative player-inventory gestures.
//!
//! This first tranche deliberately owns one request at a time. It predicts only
//! the two touched cells and never queues a second gesture behind an in-flight
//! request.

use std::collections::{BTreeMap, VecDeque};

mod admission;
mod gesture;
#[cfg(test)]
mod gesture_tests;
mod helpers;
#[cfg(test)]
mod lifecycle_tests;
#[cfg(test)]
mod merge_tests;
mod personal;
mod registry;
mod response;

use personal::PersonalWindow;
pub use response::StackResponseOverlay;

use helpers::{cell_surface, valid_raw_window_id};

use protocol::{
    ContainerIdentity, InventoryAuthority, ItemRegistryEntry, NetworkItemStack, Packet,
    StackRequestAction, StackRequestContainer, StackRequestSlot, container_close_packet,
    item_stack_request_packet, open_inventory_packet,
};
use thiserror::Error;

pub const PLAYER_INVENTORY_SLOT_COUNT: usize = 36;
pub const INVENTORY_REQUEST_TIMEOUT_MILLIS: u64 = 1_500;
/// Remote window churn is retained only far enough to close the newest
/// observed surface, while the current personal close can never be evicted.
const MAX_PENDING_CLOSES: usize = 8;
/// The decoded generic-storage container name
/// (`protocol::CONTAINER_NAME_LEVEL_ENTITY`).
pub const GENERIC_STORAGE_SLOT_TYPE: u8 = protocol::CONTAINER_NAME_LEVEL_ENTITY;
pub const GENERIC_STORAGE_WINDOW_TYPE: i8 = 0;
pub const PERSONAL_INVENTORY_WINDOW_TYPE: i8 = -1;
/// A close acknowledgement sent after the addressed window no longer exists.
const NO_CONTAINER_WINDOW_TYPE: i8 = -9;
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
    owner: PendingCloseOwner,
}

#[derive(Debug, Clone, Copy, Eq, PartialEq)]
enum PendingCloseOwner {
    Cleanup,
    Storage,
    Personal(u64),
}

impl PendingCloseOwner {
    const fn priority(self) -> u8 {
        match self {
            Self::Cleanup => 0,
            Self::Storage => 1,
            Self::Personal(_) => 2,
        }
    }

    const fn personal_generation(self) -> Option<u64> {
        match self {
            Self::Personal(generation) => Some(generation),
            Self::Cleanup | Self::Storage => None,
        }
    }
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
    /// A partial transfer temporarily presents two halves with the source's
    /// retained identity. An accepted response must separate those
    /// identities before either half becomes reusable authority.
    requires_distinct_stack_ids: bool,
    /// This prediction merged two occupied stacks using the session registry.
    registry_bound_merge: bool,
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
    personal_generation: Option<u64>,
    storage_identity: Option<ContainerIdentity>,
}

#[derive(Debug, Clone, Copy, Eq, PartialEq, Error)]
pub enum InventoryGestureError {
    #[error("server-authoritative inventory is not active")]
    AuthorityUnavailable,
    #[error("personal inventory open has not been admitted")]
    PersonalInventoryUnavailable,
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
    item_registry: Option<BTreeMap<i32, ItemRegistryEntry>>,
    next_authority_revision: u64,
    pending: Option<PendingRequest>,
    next_request_id: i32,
    session_generation: u64,
    next_open_generation: u64,
    personal: Option<PersonalWindow>,
    personal_lifecycle_failed: bool,
    storage: Option<StorageWindow>,
    pending_closes: VecDeque<PendingClose>,
    player_resync_required: bool,
    cursor_resync_required: bool,
    /// Well-formed authoritative inventory traffic whose container identity
    /// did not resolve onto a retained canonical ledger cell: unknown
    /// container codes, unreviewed surfaces (armor, offhand), or indices
    /// outside every mapped surface. Typed counted leniency — these events
    /// mutate nothing and never end the session.
    skipped_unknown_containers: u64,
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
            item_registry: None,
            next_authority_revision: 1,
            pending: None,
            next_request_id: -3,
            session_generation: 0,
            next_open_generation: 1,
            personal: None,
            personal_lifecycle_failed: false,
            storage: None,
            pending_closes: VecDeque::new(),
            player_resync_required: false,
            cursor_resync_required: false,
            skipped_unknown_containers: 0,
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

    /// How many well-formed authoritative events resolved onto no retained
    /// canonical cell and were skipped as typed counted leniency. See
    /// [`admission`].
    #[must_use]
    pub const fn skipped_unknown_containers(&self) -> u64 {
        self.skipped_unknown_containers
    }

    pub fn pending_packet(&self) -> Result<Option<Packet>, InventoryGestureError> {
        if let Some(close) = self.pending_closes.front().copied() {
            return container_close_packet(close.window_id, close.window_type)
                .map(Some)
                .map_err(|_| InventoryGestureError::InvalidRequest);
        }
        if let Some(PersonalWindow::Opening {
            target_runtime_id,
            admitted: false,
            ..
        }) = self.personal
        {
            return open_inventory_packet(target_runtime_id)
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
        if let Some(close) = self.pending_closes.pop_front() {
            if let Some(generation) = close.owner.personal_generation()
                && let Some(PersonalWindow::Closing {
                    generation: current,
                    deadline_millis,
                    ..
                }) = self.personal.as_mut()
                && *current == generation
            {
                *deadline_millis =
                    Some(now_millis.saturating_add(INVENTORY_REQUEST_TIMEOUT_MILLIS));
            }
            return true;
        }
        if let Some(PersonalWindow::Opening {
            admitted,
            deadline_millis,
            ..
        }) = self.personal.as_mut()
            && !*admitted
        {
            *admitted = true;
            *deadline_millis = Some(now_millis.saturating_add(INVENTORY_REQUEST_TIMEOUT_MILLIS));
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
        if !self.pending_closes.is_empty()
            || matches!(
                self.personal,
                Some(PersonalWindow::Opening {
                    admitted: false,
                    ..
                })
            )
        {
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
    /// Returns `true` only when the personal Open/Close lifecycle expired.
    pub fn poll_timeout(&mut self, now_millis: u64) -> bool {
        let personal_expired = self.poll_personal_timeout(now_millis);
        let Some(pending) = self.pending.as_mut() else {
            return personal_expired;
        };
        if pending.state != InventoryPendingState::AwaitingResponse
            || pending
                .deadline_millis
                .is_none_or(|deadline| now_millis < deadline)
        {
            return personal_expired;
        }
        self.require_authoritative_recovery();
        personal_expired
    }

    fn poll_personal_timeout(&mut self, now_millis: u64) -> bool {
        let Some(personal) = self.personal else {
            return false;
        };
        let expired = match personal {
            PersonalWindow::Opening {
                admitted: true,
                deadline_millis: Some(deadline),
                ..
            }
            | PersonalWindow::Closing {
                deadline_millis: Some(deadline),
                ..
            } => now_millis >= deadline,
            _ => false,
        };
        if !expired {
            return false;
        }
        let generation = personal.generation();
        self.pending_closes
            .retain(|close| close.owner.personal_generation() != Some(generation));
        if self
            .pending
            .as_ref()
            .is_some_and(|pending| pending.personal_generation == Some(generation))
        {
            match self.pending_state() {
                Some(InventoryPendingState::AwaitingTransport) => self.rollback_pending(),
                Some(InventoryPendingState::AwaitingResponse) => {
                    self.require_authoritative_recovery();
                }
                None => {}
            }
        }
        self.personal = None;
        self.personal_lifecycle_failed = true;
        if self.cursor.as_ref().is_some_and(|stack| !stack.is_empty()) {
            self.cursor = None;
            self.cursor_overlay = None;
            self.bump_cell_revision(Cell::Cursor);
            self.player_resync_required = true;
            self.cursor_resync_required = true;
        }
        true
    }

    pub fn transport_closed(&mut self) {
        self.pending_closes.clear();
        self.personal = None;
        match self.pending_state() {
            Some(InventoryPendingState::AwaitingTransport) => self.rollback_pending(),
            Some(InventoryPendingState::AwaitingResponse) => {
                self.require_authoritative_recovery();
            }
            None => {}
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
        self.queue_close(
            window_id,
            GENERIC_STORAGE_WINDOW_TYPE,
            PendingCloseOwner::Storage,
        );
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

    fn finish_personal_close(&mut self, retain_confirmed_cursor: bool) {
        let generation = match self.personal {
            Some(
                PersonalWindow::Open { generation, .. }
                | PersonalWindow::Closing { generation, .. },
            ) => generation,
            _ => return,
        };
        // The cursor is session-owned rather than window-owned. Only a
        // settled cursor may survive the acknowledgement of our own close;
        // every pending or recovery-marked state keeps the fail-closed path.
        let retain_confirmed_cursor =
            retain_confirmed_cursor && self.pending.is_none() && !self.cursor_resync_required;
        if self
            .pending
            .as_ref()
            .is_some_and(|pending| pending.personal_generation == Some(generation))
        {
            match self.pending_state() {
                Some(InventoryPendingState::AwaitingTransport) => self.rollback_pending(),
                Some(InventoryPendingState::AwaitingResponse) => {
                    self.require_authoritative_recovery();
                }
                None => {}
            }
        }
        self.personal = None;
        if !retain_confirmed_cursor && self.cursor.as_ref().is_some_and(|stack| !stack.is_empty()) {
            self.cursor = None;
            self.cursor_overlay = None;
            self.bump_cell_revision(Cell::Cursor);
            self.player_resync_required = true;
            self.cursor_resync_required = true;
        }
    }

    fn queue_close(&mut self, window_id: i32, window_type: i8, owner: PendingCloseOwner) {
        if !valid_raw_window_id(window_id) {
            return;
        }
        if let Some(existing) = self
            .pending_closes
            .iter_mut()
            .find(|close| close.window_id == window_id && close.window_type == window_type)
        {
            if owner.priority() > existing.owner.priority() {
                existing.owner = owner;
            }
            return;
        }
        if self.pending_closes.len() >= MAX_PENDING_CLOSES {
            let current_personal = self.personal.as_ref().map(PersonalWindow::generation);
            // Old cleanup and storage closes describe superseded server
            // windows. Evict the oldest such control, but preserve the close
            // that owns the still-current personal generation.
            let Some(eviction) = self.pending_closes.iter().position(|close| {
                current_personal
                    .is_none_or(|generation| close.owner.personal_generation() != Some(generation))
            }) else {
                return;
            };
            self.pending_closes.remove(eviction);
        }
        self.pending_closes.push_back(PendingClose {
            window_id,
            window_type,
            owner,
        });
    }

    fn remove_pending_close(&mut self, window_id: i32, window_type: i8) {
        self.pending_closes
            .retain(|close| close.window_id != window_id || close.window_type != window_type);
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
