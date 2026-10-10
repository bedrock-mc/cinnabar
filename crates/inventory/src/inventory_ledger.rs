//! Player-inventory gestures for the negotiated inventory authority.
//!
//! Server requests retain stamped sparse cells until their answers arrive.
//! Client-authoritative transactions retain old/new cells until transport
//! admission; later server pushes correct their committed values.

use std::collections::{BTreeMap, VecDeque};

mod admission;
mod auto_craft;
mod bundles;
mod cells;
mod crafting;
mod crafting_close;
#[cfg(test)]
mod crafting_tests;
mod distribute;
#[cfg(test)]
mod fixed_window_tests;
#[cfg(test)]
mod generic_storage_tests;
mod gesture;
#[cfg(test)]
mod gesture_tests;
mod helpers;
mod item_roles;
mod legacy;
#[cfg(test)]
mod legacy_tests;
#[cfg(test)]
mod lifecycle_tests;
#[cfg(test)]
mod merge_tests;
mod moves;
#[cfg(test)]
mod moves_tests;
mod overlay;
#[cfg(test)]
mod overlay_tests;
mod personal;
mod queue;
mod quick_move;
mod registry;
#[cfg(test)]
mod request_tests;
mod response;
mod revisions;
mod screen_actions;
#[cfg(test)]
mod screens_tests;
#[cfg(test)]
mod server_menu_tests;
mod settling;
mod windows;

use cells::{Cell, CellSurface, Cells};
pub use crafting::{CraftGridCell, CraftSink, CraftingGrid, CreativeDestination};
pub use distribute::{DistributeMode, DragDistribution, MAX_DISTRIBUTION_CELLS};
pub use gesture::{CellGesture, InventoryTarget};
pub use moves::DropSource;
use personal::PersonalWindow;
pub use queue::MAX_PENDING_REQUESTS;
use queue::PendingRequest;
pub use response::StackResponseOverlay;
pub use screen_actions::ScreenCraft;
use settling::SettlingWindow;

use helpers::valid_raw_window_id;

#[cfg(test)]
use protocol::NO_CONTAINER_WINDOW_TYPE;
use protocol::{
    ContainerIdentity, InventoryAuthority, ItemRegistryEntry, NetworkItemStack, Packet,
    container_close_packet, open_inventory_packet,
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
/// The crafting-table window; its 3x3 grid lives in UI slots 32..=40.
pub const WORKBENCH_WINDOW_TYPE: i8 = 1;
pub const PERSONAL_INVENTORY_WINDOW_TYPE: i8 = -1;
pub const SMALL_STORAGE_SLOT_COUNT: usize = 27;
pub const LARGE_STORAGE_SLOT_COUNT: usize = 54;
/// Maximum opt-in storage-content identity diagnostics emitted per ledger
/// session. This bounds even adversarial streams while retaining enough early
/// events to diagnose Open/Content ordering and identity mismatches.
const MAX_STORAGE_CONTENT_TRACES: u8 = 8;

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

/// Open generic-storage window metadata; its cells live in [`Cells`].
#[derive(Debug, Clone)]
struct StorageWindow {
    window_id: i32,
    window_type: i8,
    kind: protocol::WindowKind,
    /// Block position the server opened the window on.
    position: [i32; 3],
    /// Unique id of the actor the window belongs to (a mount), `-1` for blocks.
    actor_unique_id: i64,
    /// Server-pushed window properties (furnace progress, brew time, ...).
    data: BTreeMap<i32, i32>,
    generation: u64,
    identity: Option<ContainerIdentity>,
    resync_required: bool,
    /// Set by a local close while admitted predictions still await their
    /// responses, retaining the generation they reconcile against.
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

#[derive(Debug, Clone, Copy, Eq, PartialEq, Error)]
pub enum InventoryGestureError {
    #[error("inventory authority has not been negotiated")]
    AuthorityUnavailable,
    #[error("this action is not implemented for client-authoritative inventory")]
    LegacyActionUnavailable,
    #[error("personal inventory open has not been admitted")]
    PersonalInventoryUnavailable,
    #[error("player inventory slot {0} is outside 0..36")]
    InvalidSlot(u8),
    #[error("generic storage slot {0} is outside the authoritative window")]
    InvalidStorageSlot(u8),
    #[error("player inventory slot {0} is not known yet")]
    UnknownSlot(u8),
    #[error("the inventory request queue is full")]
    Busy,
    #[error("a touched stack has no settled server identity yet")]
    AwaitingIdentity,
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
    /// Backing truth, including client-authoritative writes admitted to transport.
    confirmed: Cells,
    /// Backing truth covered by active absolute sparse cells; `None` while idle.
    view: Option<Cells>,
    known: [bool; PLAYER_INVENTORY_SLOT_COUNT],
    armor_known: [bool; cells::ARMOR_CELLS],
    offhand_known: bool,
    slot_revisions: [u64; PLAYER_INVENTORY_SLOT_COUNT],
    item_registry: Option<std::sync::Arc<BTreeMap<i32, ItemRegistryEntry>>>,
    creative: Option<protocol::CreativeContentEvent>,
    /// Enchanting-table options for the current input item.
    enchant_options: Option<std::sync::Arc<[protocol::EnchantOption]>>,
    /// Bundle contents by dynamic container id.
    bundles: BTreeMap<u32, Vec<NetworkItemStack>>,
    queue: VecDeque<PendingRequest>,
    pending_world_drops: usize,
    next_request_id: i32,
    session_generation: u64,
    next_open_generation: u64,
    personal: Option<PersonalWindow>,
    personal_lifecycle_failed: bool,
    /// An open press made while a close is unsent or unacknowledged; opens once it settles.
    held_open: Option<u64>,
    /// Acknowledged closes whose requests still await answers; they stay current.
    settling: VecDeque<SettlingWindow>,
    storage: Option<StorageWindow>,
    pub(crate) furnace_selection: Option<crate::furnace_recipes::Selection>,
    pending_closes: VecDeque<PendingClose>,
    /// Admitted requests abandoned unanswered and the surfaces each put into
    /// recovery, oldest first: a late rejection lifts that recovery.
    abandoned: VecDeque<queue::AbandonedRequest>,
    player_resync_required: bool,
    cursor_resync_required: bool,
    armor_resync_required: bool,
    offhand_resync_required: bool,
    crafting_resync_required: bool,
    storage_content_traces_remaining: u8,
    /// Well-formed authoritative inventory traffic whose container identity
    /// did not resolve onto a retained canonical ledger cell. Typed counted
    /// leniency: these events mutate nothing and never end the session.
    skipped_unknown_containers: u64,
}

impl Default for PlayerInventoryLedger {
    fn default() -> Self {
        Self {
            authority: None,
            confirmed: Cells::default(),
            view: None,
            known: [false; PLAYER_INVENTORY_SLOT_COUNT],
            armor_known: [false; cells::ARMOR_CELLS],
            offhand_known: false,
            slot_revisions: [0; PLAYER_INVENTORY_SLOT_COUNT],
            item_registry: None,
            creative: None,
            enchant_options: None,
            bundles: BTreeMap::new(),
            queue: VecDeque::new(),
            pending_world_drops: 0,
            next_request_id: -3,
            session_generation: 0,
            next_open_generation: 1,
            personal: None,
            personal_lifecycle_failed: false,
            held_open: None,
            settling: VecDeque::new(),
            storage: None,
            furnace_selection: None,
            pending_closes: VecDeque::new(),
            abandoned: VecDeque::new(),
            player_resync_required: false,
            cursor_resync_required: false,
            armor_resync_required: false,
            offhand_resync_required: false,
            crafting_resync_required: false,
            storage_content_traces_remaining: MAX_STORAGE_CONTENT_TRACES,
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
        if !*self.known.get(usize::from(slot))? {
            return Some(PlayerInventorySlot::Unknown);
        }
        Some(match self.view_stack(Cell::Inventory(slot)) {
            Some(stack) => PlayerInventorySlot::Present(stack),
            None => PlayerInventorySlot::Empty,
        })
    }

    #[must_use]
    pub fn displayed_stack(&self, slot: u8) -> Option<&NetworkItemStack> {
        self.view_stack(Cell::Inventory(slot))
    }

    #[must_use]
    pub fn cursor_stack(&self) -> Option<&NetworkItemStack> {
        self.view_stack(Cell::Cursor)
    }

    #[must_use]
    pub fn storage_stack(&self, slot: u8) -> Option<&NetworkItemStack> {
        self.view_stack(Cell::Storage(slot))
    }

    /// The presented stack in any gesture target, including armor, offhand
    /// and crafting cells.
    #[must_use]
    pub fn target_stack(&self, target: InventoryTarget) -> Option<&NetworkItemStack> {
        self.view_stack(target.cell())
    }

    /// Worn armor or offhand presentation, preserving unobserved versus empty cells.
    /// Other surfaces and out-of-range armor addresses return `None`.
    pub fn gear_slot_state(&self, target: InventoryTarget) -> Option<PlayerInventorySlot<'_>> {
        let known = match target {
            InventoryTarget::Armor(slot) => *self.armor_known.get(usize::from(slot))?,
            InventoryTarget::Offhand => self.offhand_known,
            _ => return None,
        };
        Some(match self.target_stack(target) {
            Some(stack) => PlayerInventorySlot::Present(stack),
            None if known => PlayerInventorySlot::Empty,
            None => PlayerInventorySlot::Unknown,
        })
    }

    #[must_use]
    pub fn created_output_stack(&self) -> Option<&NetworkItemStack> {
        self.view_stack(Cell::CreatedOutput)
    }

    fn view_stack(&self, cell: Cell) -> Option<&NetworkItemStack> {
        self.view().get(cell).map(|held| &held.stack)
    }

    #[must_use]
    pub fn storage_identity(&self) -> Option<ContainerIdentity> {
        self.storage.as_ref()?.identity
    }

    /// The screen kind of the open container window, if one is open.
    #[must_use]
    pub fn window_kind(&self) -> Option<protocol::WindowKind> {
        Some(self.storage.as_ref()?.kind)
    }

    /// Where the open window's container sits in the world.
    #[must_use]
    pub fn window_position(&self) -> Option<[i32; 3]> {
        Some(self.storage.as_ref()?.position)
    }

    /// The actor the open window belongs to, such as a mount's inventory.
    #[must_use]
    pub fn window_actor(&self) -> Option<i64> {
        Some(self.storage.as_ref()?.actor_unique_id).filter(|id| *id != -1)
    }

    /// A server-pushed property of the open window (`ContainerSetData`), if sent.
    #[must_use]
    pub fn window_data(&self, property: i32) -> Option<i32> {
        self.storage.as_ref()?.data.get(&property).copied()
    }

    /// The enchanting options the server last offered, if any.
    #[must_use]
    pub fn enchant_options(&self) -> Option<&[protocol::EnchantOption]> {
        self.enchant_options.as_deref()
    }

    /// How requests name the open window's own cells; needs the window's content.
    pub(super) fn window_address(&self) -> Option<helpers::WindowAddress> {
        let storage = self.storage.as_ref()?;
        Some(helpers::WindowAddress {
            identity: storage.identity?,
            kind: storage.kind,
        })
    }

    #[must_use]
    pub fn storage_generation(&self) -> Option<u64> {
        Some(self.storage.as_ref()?.generation)
    }

    #[must_use]
    pub fn storage_slot_count(&self) -> Option<usize> {
        self.storage
            .as_ref()?
            .identity
            .map(|_| self.confirmed.storage_len())
    }

    /// The open storage window's container type code, for picking its screen.
    #[must_use]
    pub fn storage_window_type(&self) -> Option<i8> {
        self.storage.as_ref().map(|storage| storage.window_type)
    }

    /// The state of the oldest unresolved request.
    #[must_use]
    pub fn pending_state(&self) -> Option<InventoryPendingState> {
        self.gestures().next().map(|pending| pending.state)
    }

    /// Queued gesture requests; mining requests ride player input instead.
    fn gestures(&self) -> impl Iterator<Item = &PendingRequest> {
        self.queue.iter().filter(|pending| pending.mining.is_none())
    }

    /// The oldest unresolved request id.
    #[must_use]
    pub fn pending_request_id(&self) -> Option<i32> {
        self.gestures().next().map(|pending| pending.request_id)
    }

    #[must_use]
    pub fn pending_request_count(&self) -> usize {
        self.gestures().count()
    }

    /// Queues a mine-block prediction for hotbar `slot` and returns the id its
    /// PlayerAuthInput carries; `None` sends the break without a request.
    pub fn begin_mining_request(&mut self, slot: u8, predicted_damage: i32) -> Option<i32> {
        self.enqueue_mining(slot, predicted_damage)
    }

    /// Forgets a mining request whose input never left, so it cannot hold back later answers.
    pub fn cancel_mining_request(&mut self, request_id: i32) {
        self.remove_unanswered_mining(request_id);
    }

    /// The newest outstanding mining prediction for `slot`, else its last
    /// accepted damage.
    #[must_use]
    pub fn predicted_slot_damage(&self, slot: u8) -> Option<i32> {
        self.queue
            .iter()
            .rev()
            .filter_map(|pending| pending.mining)
            .find(|mining| mining.slot == slot)
            .map(|mining| mining.damage)
            .or_else(|| self.slot_overlay(slot)?.durability_correction)
    }

    #[must_use]
    pub fn slot_pending(&self, slot: u8) -> bool {
        self.cell_pending(Cell::Inventory(slot))
    }

    /// Whether an unanswered gesture, not a mining request, predicts hotbar `slot`.
    #[must_use]
    pub fn slot_gesture_pending(&self, slot: u8) -> bool {
        self.gestures()
            .any(|pending| pending.touches(Cell::Inventory(slot)))
    }

    #[must_use]
    pub fn storage_slot_pending(&self, slot: u8) -> bool {
        self.cell_pending(Cell::Storage(slot))
    }

    fn cell_pending(&self, cell: Cell) -> bool {
        self.queue.iter().any(|pending| pending.touches(cell))
    }

    #[must_use]
    pub fn resync_required(&self) -> bool {
        [
            CellSurface::Player,
            CellSurface::Cursor,
            CellSurface::Storage,
            CellSurface::Armor,
            CellSurface::Offhand,
            CellSurface::Crafting,
        ]
        .into_iter()
        .any(|surface| self.surface_flagged(surface))
    }

    /// Whether gestures touching `surface` must wait for authority.
    fn surface_flagged(&self, surface: CellSurface) -> bool {
        self.surface_awaiting_refresh(surface) || self.surface_recovering(surface)
    }

    fn surface_recovering(&self, surface: CellSurface) -> bool {
        match surface {
            CellSurface::Player => self.player_resync_required,
            CellSurface::Cursor => self.cursor_resync_required,
            CellSurface::Storage => self
                .storage
                .as_ref()
                .is_some_and(|storage| storage.resync_required),
            CellSurface::Armor => self.armor_resync_required,
            CellSurface::Offhand => self.offhand_resync_required,
            CellSurface::Crafting => self.crafting_resync_required,
        }
    }

    /// How many well-formed authoritative events resolved onto no retained
    /// canonical cell and were skipped as typed counted leniency.
    #[must_use]
    pub const fn skipped_unknown_containers(&self) -> u64 {
        self.skipped_unknown_containers
    }

    #[cfg(test)]
    fn first_unsent(&self) -> Option<&PendingRequest> {
        self.queue
            .iter()
            .find(|pending| pending.state == InventoryPendingState::AwaitingTransport)
    }

    /// Places ready requests in one packet; window controls keep their existing queue priority.
    pub fn pending_batch(&self) -> Result<Option<(Packet, usize)>, InventoryGestureError> {
        if let Some(control) = self.pending_control_packet()? {
            return Ok(Some((control, 1)));
        }
        if self.authority == Some(InventoryAuthority::Client) {
            return self
                .legacy_pending_packet()
                .map(|packet| packet.map(|packet| (packet, 1)));
        }
        let requests: Vec<_> = self
            .queue
            .iter()
            .filter(|pending| pending.state == InventoryPendingState::AwaitingTransport)
            .map(|pending| {
                (
                    pending.request_id,
                    pending.actions.as_slice(),
                    pending.filter_strings.as_slice(),
                )
            })
            .collect();
        if requests.is_empty() {
            return Ok(None);
        }
        let count = requests.len();
        protocol::item_stack_request_batch(requests)
            .map(|packet| packet.map(|packet| (packet, count)))
            .map_err(|_| InventoryGestureError::InvalidRequest)
    }

    /// Returns the next window lifecycle packet before inventory mutations.
    fn pending_control_packet(&self) -> Result<Option<Packet>, InventoryGestureError> {
        if let Some(close) = self.pending_closes.front().copied()
            && self.close_ready()
        {
            return container_close_packet(close.window_id)
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
        Ok(None)
    }

    pub fn mark_transport_enqueued(&mut self, now_millis: u64) -> bool {
        if self.close_ready()
            && let Some(close) = self.pending_closes.pop_front()
        {
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
            self.resume_held_open();
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
        if self.authority == Some(InventoryAuthority::Client) {
            return self.commit_legacy_transport();
        }
        let Some(pending) = self
            .queue
            .iter_mut()
            .find(|pending| pending.state == InventoryPendingState::AwaitingTransport)
        else {
            return false;
        };
        pending.state = InventoryPendingState::AwaitingResponse;
        tracing::debug!(target: "bedrock_client::inventory_requests",
            request_id = pending.request_id, "inventory request admitted to transport");
        pending.transport_deadline_millis = None;
        pending.deadline_millis = Some(now_millis.saturating_add(INVENTORY_REQUEST_TIMEOUT_MILLIS));
        true
    }

    /// Unsent requests never reached the server, so sustained queue pressure
    /// rolls every one of them back.
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
        let Some(pending) = self
            .queue
            .iter_mut()
            .find(|pending| pending.state == InventoryPendingState::AwaitingTransport)
        else {
            return;
        };
        let deadline = *pending
            .transport_deadline_millis
            .get_or_insert_with(|| now_millis.saturating_add(INVENTORY_REQUEST_TIMEOUT_MILLIS));
        if now_millis >= deadline {
            self.abandon_requests(|pending| {
                pending.state == InventoryPendingState::AwaitingTransport
            });
            self.finish_closing();
        }
    }

    /// Admitted requests are never retransmitted or rolled back on a missing
    /// response: they keep their prediction and require a full refresh.
    /// Returns `true` only when the personal Open/Close lifecycle expired.
    pub fn poll_timeout(&mut self, now_millis: u64) -> bool {
        let personal_expired = self.poll_personal_timeout(now_millis);
        self.expire_overdue_requests(now_millis);
        self.finish_settled_closes();
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
        self.abandon_requests(|pending| pending.personal_generation == Some(generation));
        self.personal = None;
        self.personal_lifecycle_failed = true;
        self.held_open = None;
        self.drop_confirmed_cursor();
        self.refold();
        true
    }

    pub fn transport_closed(&mut self) {
        self.pending_closes.clear();
        self.personal = None;
        self.held_open = None;
        self.settling.clear();
        self.abandon_requests(|_| true);
        self.finish_closing();
    }

    /// Clears a held cursor that no window vouches for any more.
    fn drop_confirmed_cursor(&mut self) {
        if self.confirmed.take(Cell::Cursor).is_some() {
            self.disown_abandoned_recovery(CellSurface::Player);
            self.disown_abandoned_recovery(CellSurface::Cursor);
            self.player_resync_required = true;
            self.cursor_resync_required = true;
        }
    }

    fn storage_request_bound(&self, generation: u64) -> bool {
        self.queue
            .iter()
            .any(|pending| pending.storage_generation == Some(generation))
    }

    pub fn request_storage_close(&mut self) {
        let Some(storage) = self.storage.as_ref() else {
            return;
        };
        if storage.closing {
            // A prior local close is already waiting out its admitted
            // predictions; further close gestures stay blocked.
            return;
        }
        let (window_id, window_type, generation) =
            (storage.window_id, storage.window_type, storage.generation);
        let returning = if window_type == WORKBENCH_WINDOW_TYPE
            || self.authority == Some(InventoryAuthority::Client)
        {
            // Vanilla always closes; inputs it cannot return wait for the server's restatement.
            self.return_crafting_on_close().unwrap_or_else(|error| {
                self.note_close_return_failure(error);
                true
            })
        } else {
            false
        };
        self.queue_close(window_id, window_type, PendingCloseOwner::Storage);
        if !returning {
            self.abandon_requests(|pending| {
                pending.storage_generation == Some(generation)
                    && pending.state == InventoryPendingState::AwaitingTransport
            });
        }
        if self.storage_request_bound(generation) {
            // Retain the window so outstanding responses still reconcile
            // against its exact generation and identity.
            self.storage
                .as_mut()
                .expect("storage observed above")
                .closing = true;
        } else {
            self.close_storage();
        }
    }

    /// Settles a locally closing window once no request is bound to it any
    /// more, exactly like an immediate close.
    fn finish_closing(&mut self) {
        let Some(storage) = self.storage.as_ref() else {
            return;
        };
        if storage.closing
            && !self.storage_request_bound(storage.generation)
            && (self.close_ready()
                || !self
                    .pending_closes
                    .iter()
                    .any(|close| close.owner == PendingCloseOwner::Storage))
        {
            self.discard_storage();
        }
    }

    fn close_storage(&mut self) {
        if let Some(generation) = self.storage_generation() {
            self.abandon_requests(|pending| pending.storage_generation == Some(generation));
        }
        self.discard_storage();
    }

    fn discard_storage(&mut self) {
        self.discard_storage_window();
        self.clear_storage_inputs();
    }

    /// Our acknowledged close keeps the window's unanswered requests correlated.
    fn acknowledge_storage_close(&mut self, server_initiated: bool) {
        let Some(storage) = self.storage.as_ref() else {
            return;
        };
        let window = SettlingWindow::Storage {
            generation: storage.generation,
            identity: storage.identity,
        };
        if !server_initiated && storage.closing && self.has_unanswered(window) {
            self.discard_storage_window();
            self.retain_settling(window);
            self.refold();
        } else {
            self.close_storage();
        }
    }

    fn discard_storage_window(&mut self) {
        self.storage = None;
        self.clear_furnace_recipe();
        self.enchant_options = None;
        self.confirmed.clear_storage();
    }

    fn clear_storage_inputs(&mut self) {
        self.clear_crafting();
        if self.confirmed.get(Cell::Cursor).is_some() {
            self.disown_abandoned_recovery(CellSurface::Player);
            self.disown_abandoned_recovery(CellSurface::Cursor);
            self.player_resync_required = true;
            self.cursor_resync_required = true;
        }
        self.refold();
    }

    fn finish_personal_close(&mut self, retain_confirmed_cursor: bool) {
        let generation = match self.personal {
            Some(
                PersonalWindow::Open { generation, .. }
                | PersonalWindow::Closing { generation, .. },
            ) => generation,
            _ => return,
        };
        let window = SettlingWindow::Personal(generation);
        if retain_confirmed_cursor && self.has_unanswered(window) {
            self.personal = None;
            self.retain_settling(window);
            self.refold();
            return;
        }
        // The cursor is session-owned rather than window-owned. Only a
        // settled cursor may survive the acknowledgement of our own close.
        let retain_confirmed_cursor =
            retain_confirmed_cursor && self.queue.is_empty() && !self.cursor_resync_required;
        self.abandon_requests(|pending| pending.personal_generation == Some(generation));
        self.personal = None;
        self.clear_window_inputs(retain_confirmed_cursor);
    }

    fn clear_window_inputs(&mut self, retain_confirmed_cursor: bool) {
        self.clear_crafting();
        if !retain_confirmed_cursor {
            self.drop_confirmed_cursor();
        }
        self.refold();
    }

    /// Inputs are actual inventory, not a disposable preview. Never erase an
    /// unreturned ingredient just because the screen stopped owning its UI.
    fn clear_crafting(&mut self) {
        if self
            .confirmed
            .occupied()
            .any(|(cell, _)| matches!(cell, Cell::Craft(_)))
        {
            self.disown_abandoned_recovery(CellSurface::Crafting);
            self.crafting_resync_required = true;
            return;
        }
        self.confirmed.clear_ui();
        self.crafting_resync_required = false;
        self.surface_refreshed(CellSurface::Crafting);
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

    /// Requires a refresh independently of any abandoned request's eventual answer.
    fn mark_cell_recovery(&mut self, cell: Cell) {
        self.disown_abandoned_recovery(cell.surface());
        self.require_cell_recovery(cell);
    }

    /// Marks a cell unverified and clears its response overlay.
    fn require_cell_recovery(&mut self, cell: Cell) {
        if let Some(held) = self.confirmed.get_mut(cell) {
            held.overlay = None;
        }
        match cell.surface() {
            CellSurface::Player => self.player_resync_required = true,
            CellSurface::Cursor => self.cursor_resync_required = true,
            CellSurface::Storage => {
                if let Some(storage) = self.storage.as_mut() {
                    storage.resync_required = true;
                }
            }
            CellSurface::Armor => self.armor_resync_required = true,
            CellSurface::Offhand => self.offhand_resync_required = true,
            CellSurface::Crafting => self.crafting_resync_required = true,
        }
    }
}
