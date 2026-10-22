//! Player-inventory gestures for the negotiated inventory authority.
//!
//! Server requests retain stamped sparse cells until their answers arrive.
//! Client-authoritative transactions retain old/new cells until transport
//! admission; later server pushes correct their committed values.

use std::collections::{BTreeMap, VecDeque};

mod admission;
mod auto_craft;
#[cfg(test)]
pub(crate) use auto_craft::take_source_validations;
mod bundles;
mod cancellation;
mod cells;
mod consumption;
mod control;
mod crafting;
mod crafting_close;
#[cfg(test)]
mod crafting_tests;
mod distribute;
pub(crate) mod equipment;
#[cfg(test)]
mod fixed_window_tests;
mod fuel;
mod furnace_inputs;
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
#[cfg(test)]
mod offhand_quick_move_tests;
mod overlay;
#[cfg(test)]
mod overlay_tests;
mod packet_routes;
mod personal;
mod presentation;
mod queue;
mod quick_move;
#[cfg(test)]
mod quick_move_role_tests;
mod recipe_allocation;
mod recovery;
#[cfg(test)]
mod recovery_tests;
pub(crate) mod registry;
#[cfg(test)]
mod request_tests;
mod response;
mod revisions;
mod screen_actions;
mod trading;
pub use trading::{ResolvedTrade, TradeState};
#[cfg(test)]
mod screens_tests;
#[cfg(test)]
mod server_menu_tests;
mod validation;
mod windows;

use cells::{Cell, CellSurface, Cells};
pub use crafting::{CraftGridCell, CraftSink, CraftingGrid, CreativeDestination};
pub use distribute::{DistributeMode, DragDistribution, MAX_DISTRIBUTION_CELLS};
pub use gesture::{CellGesture, InventoryTarget};
pub use moves::DropSource;
use personal::PersonalWindow;
pub use presentation::InventoryPresentationSnapshot;
pub use queue::MAX_PENDING_REQUESTS;
use queue::PendingRequest;
pub use recovery::INVENTORY_RECOVERY_WINDOW_MILLIS;
pub use response::StackResponseOverlay;
pub use screen_actions::{ANVIL_MATERIAL_SLOT, ANVIL_TARGET_SLOT, ScreenCraft};

#[cfg(test)]
use protocol::NO_CONTAINER_WINDOW_TYPE;
use protocol::{
    ContainerIdentity, InventoryAuthority, ItemRegistryEntry, NetworkItemStack, Packet,
    container_close_packet, open_inventory_packet,
};
use thiserror::Error;

/// Player slots share the protocol request boundary.
pub const PLAYER_INVENTORY_SLOT_COUNT: usize = protocol::PLAYER_INVENTORY_SLOTS as usize;
pub(crate) use cells::{PERSONAL_INPUT_COUNT, PERSONAL_UPDATE_COUNT};
/// `None` means unmodified; `Some(None)` is an admitted empty cell.
pub(crate) type PersonalUpdates<'a> = [Option<Option<&'a NetworkItemStack>>; PERSONAL_UPDATE_COUNT];

pub const INVENTORY_REQUEST_TIMEOUT_MILLIS: u64 = 1_500;
/// The decoded generic-storage container name
/// (`protocol::CONTAINER_NAME_LEVEL_ENTITY`).
pub const GENERIC_STORAGE_SLOT_TYPE: u8 = protocol::CONTAINER_NAME_LEVEL_ENTITY;
pub const GENERIC_STORAGE_WINDOW_TYPE: i8 = protocol_ids::WindowType::Container.wire_value();
/// The crafting-table window; its 3x3 grid lives in UI slots 32..=40.
pub const WORKBENCH_WINDOW_TYPE: i8 = protocol_ids::WindowType::Workbench.wire_value();
pub const PERSONAL_INVENTORY_WINDOW_TYPE: i8 = protocol_ids::WindowType::Inventory.wire_value();
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
    trade: Option<TradeState>,
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
    /// The container name of a chest-like block that requests must use instead of the level entity.
    block_container: Option<u8>,
    /// The address in-flight requests were sent under before the block entity renamed it.
    superseded_identity: Option<ContainerIdentity>,
    resync_required: bool,
    /// Set by a local close while admitted predictions still await their
    /// responses, retaining the generation they reconcile against.
    closing: bool,
}

#[derive(Debug, Clone, Copy, Eq, PartialEq, Error)]
pub enum InventoryGestureError {
    #[error("inventory authority has not been negotiated")]
    AuthorityUnavailable,
    #[error("this action is not implemented for client-authoritative inventory")]
    LegacyActionUnavailable,
    #[error("personal inventory open has not been admitted")]
    PersonalInventoryUnavailable,
    #[error("player inventory slot {0} is outside the authoritative container")]
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
    #[error("the equipment slot does not accept this item")]
    EquipmentRefused,
    #[error("this item cannot leave its slot")]
    SlotLocked,
}

/// Effective actor permissions captured when an inventory command is built.
#[derive(Debug, Clone, Copy, Default, Eq, PartialEq)]
pub struct InventoryPermissions {
    pub creative: bool,
    pub instabuild: Option<bool>,
}

#[derive(Debug, Clone)]
pub struct PlayerInventoryLedger {
    authority: Option<InventoryAuthority>,
    permissions: InventoryPermissions,
    /// Backing truth, including client-authoritative writes admitted to transport.
    confirmed: Cells,
    /// Backing truth covered by active absolute sparse cells; `None` while idle.
    view: Option<Cells>,
    known: [bool; PLAYER_INVENTORY_SLOT_COUNT],
    /// Personal cells written by the latest event, consumed by the FIFO observer.
    personal_writes: u8,
    /// Whether the latest inventory event restated the backing offhand cell.
    offhand_written: bool,
    armor_known: [bool; cells::ARMOR_CELLS],
    offhand_known: bool,
    slot_revisions: [u64; PLAYER_INVENTORY_SLOT_COUNT],
    item_registry: Option<std::sync::Arc<BTreeMap<i32, ItemRegistryEntry>>>,
    item_facts: std::sync::Arc<crate::SessionItemFacts>,
    fuel_catalog: Option<std::sync::Arc<assets::RuntimeEntityAssets>>,
    fuel_facts: BTreeMap<i32, assets::item::fuel::ItemFuel>,
    furnace_inputs: furnace_inputs::FurnaceInputs,
    creative: Option<protocol::CreativeContentEvent>,
    /// Enchanting-table options for the current input item.
    enchant_options: Option<std::sync::Arc<[protocol::EnchantOption]>>,
    /// Bundle contents by dynamic container id.
    bundles: BTreeMap<u32, Vec<NetworkItemStack>>,
    queue: VecDeque<PendingRequest>,
    /// Admitted world-drop gestures waiting to deliver their immediate feedback.
    pending_world_drops: usize,
    /// Excess container opens awaiting a delivery acknowledgement, bounded like requests.
    pending_open_warnings: usize,
    next_request_id: i32,
    session_generation: u64,
    next_open_generation: u64,
    personal: Option<PersonalWindow>,
    /// Acknowledged closes whose requests still await answers; they stay current.
    storage: Option<StorageWindow>,
    pub(crate) furnace_selection: Option<crate::furnace_recipes::Selection>,
    /// The last recipe-book placement into the crafting grid.
    recipe_placement: Option<crafting::RecipePlacement>,
    /// Each window kind retains one desired state and at most one outstanding request.
    controls: control::WindowControls,
    /// Admitted requests abandoned unanswered and the surfaces each put into
    /// recovery, oldest first: a late rejection lifts that recovery.
    abandoned: VecDeque<queue::AbandonedRequest>,
    /// When the current unverified state first refused gestures.
    recovery_since_millis: Option<u64>,
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
            permissions: InventoryPermissions::default(),
            confirmed: Cells::default(),
            view: None,
            known: [false; PLAYER_INVENTORY_SLOT_COUNT],
            personal_writes: 0,
            offhand_written: false,
            armor_known: [false; cells::ARMOR_CELLS],
            offhand_known: false,
            slot_revisions: [0; PLAYER_INVENTORY_SLOT_COUNT],
            item_registry: None,
            item_facts: Default::default(),
            fuel_catalog: None,
            fuel_facts: BTreeMap::new(),
            furnace_inputs: Default::default(),
            creative: None,
            enchant_options: None,
            bundles: BTreeMap::new(),
            queue: VecDeque::new(),
            pending_world_drops: 0,
            pending_open_warnings: 0,
            next_request_id: -3,
            session_generation: 0,
            next_open_generation: 1,
            personal: None,
            storage: None,
            furnace_selection: None,
            recipe_placement: None,
            controls: control::WindowControls::default(),
            abandoned: VecDeque::new(),
            recovery_since_millis: None,
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
    /// Installs the accepted local actor's effective permissions before building a command.
    pub fn set_permissions(&mut self, permissions: InventoryPermissions) {
        self.permissions = permissions;
    }

    /// Returns only personal cells authoritatively written by the latest inventory event.
    pub(crate) fn personal_updates(&self) -> PersonalUpdates<'_> {
        std::array::from_fn(|index| {
            (self.personal_writes & (1 << index) != 0).then(|| {
                self.confirmed
                    .get(cells::personal_cell(index))
                    .map(|held| &held.stack)
            })
        })
    }

    /// Reports an admitted offhand write in the most recently applied inventory event.
    pub const fn offhand_authority_changed(&self) -> bool {
        self.offhand_written
    }

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

    /// Reads player backing independently of screen-owned sparse presentation.
    #[must_use]
    pub fn backing_slot_state(&self, slot: u8) -> Option<PlayerInventorySlot<'_>> {
        if !*self.known.get(usize::from(slot))? {
            return Some(PlayerInventorySlot::Unknown);
        }
        Some(match self.backing_stack(slot) {
            Some(stack) => PlayerInventorySlot::Present(stack),
            None => PlayerInventorySlot::Empty,
        })
    }

    /// Borrows an occupied player backing cell without applying screen predictions.
    #[must_use]
    pub fn backing_stack(&self, slot: u8) -> Option<&NetworkItemStack> {
        self.confirmed
            .get(Cell::Inventory(slot))
            .map(|held| &held.stack)
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
        self.gear_state(target, self.view())
    }

    /// Returns server-observed gear without retaining a temporary local prediction.
    pub fn authoritative_gear_slot_state(
        &self,
        target: InventoryTarget,
    ) -> Option<PlayerInventorySlot<'_>> {
        self.gear_state(target, &self.confirmed)
    }

    /// Projects a gear cell from the requested owner while preserving observation state.
    fn gear_state<'a>(
        &self,
        target: InventoryTarget,
        cells: &'a Cells,
    ) -> Option<PlayerInventorySlot<'a>> {
        let known = match target {
            InventoryTarget::Armor(slot) => *self.armor_known.get(usize::from(slot))?,
            InventoryTarget::Offhand => self.offhand_known,
            _ => return None,
        };
        Some(match cells.get(target.cell()) {
            Some(held) => PlayerInventorySlot::Present(&held.stack),
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

    /// Requests still awaiting transport or a required server response.
    fn pending_requests(&self) -> impl Iterator<Item = &PendingRequest> {
        self.queue
            .iter()
            .filter(|request| !request.locally_committed())
    }

    /// Queued gesture requests; mining requests ride player input instead.
    fn gestures(&self) -> impl Iterator<Item = &PendingRequest> {
        self.pending_requests()
            .filter(|pending| pending.mining.is_none())
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
            .filter_map(|pending| pending.mining.as_ref())
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

    /// Whether a local backing mutation still needs transport admission for this player slot.
    #[must_use]
    pub fn slot_backing_transport_pending(&self, slot: u8) -> bool {
        self.queue.iter().any(|request| {
            request.state == InventoryPendingState::AwaitingTransport
                && (self.authority == Some(InventoryAuthority::Client)
                    || request.world_drop.is_some())
                && request.touches(Cell::Inventory(slot))
        })
    }

    #[must_use]
    pub fn storage_slot_pending(&self, slot: u8) -> bool {
        self.cell_pending(Cell::Storage(slot))
    }

    fn cell_pending(&self, cell: Cell) -> bool {
        self.pending_requests().any(|pending| pending.touches(cell))
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
        if self.pending_open_warnings != 0 {
            return Ok(Some((
                protocol::repeated_container_open_warning_packet(),
                1,
            )));
        }
        if let Some(control) = self.pending_control_packet()? {
            return Ok(Some((control, 1)));
        }
        if self.authority == Some(InventoryAuthority::Client) {
            return self
                .legacy_pending_packet()
                .map(|packet| packet.map(|packet| (packet, 1)));
        }
        let mut ready = self
            .queue
            .iter()
            .filter(|pending| pending.state == InventoryPendingState::AwaitingTransport)
            .peekable();
        if let Some(drop) = ready.next_if(|pending| pending.world_drop.is_some()) {
            return self.world_drop_packet(drop).map(|packet| Some((packet, 1)));
        }
        // A later drop leaves in its own packet, after the requests queued before it.
        let requests: Vec<_> = ready
            .take_while(|pending| pending.world_drop.is_none())
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

    pub fn mark_transport_enqueued(&mut self, now_millis: u64) -> bool {
        if self.pending_open_warnings != 0 {
            self.pending_open_warnings -= 1;
            return true;
        }
        if self.mark_control_enqueued(now_millis) {
            return true;
        }
        if self.authority == Some(InventoryAuthority::Client) {
            return self.commit_legacy_transport();
        }
        let Some(index) = self
            .queue
            .iter()
            .position(|pending| pending.state == InventoryPendingState::AwaitingTransport)
        else {
            return false;
        };
        if self.queue[index].world_drop.is_some() {
            return self.commit_world_drop(index);
        }
        let pending = &mut self.queue[index];
        pending.state = InventoryPendingState::AwaitingResponse;
        tracing::debug!(target: "cinnabar::inventory_requests",
            request_id = pending.request_id, "inventory request admitted to transport");
        pending.transport_deadline_millis = None;
        pending.deadline_millis = Some(now_millis.saturating_add(INVENTORY_REQUEST_TIMEOUT_MILLIS));
        true
    }

    /// Unsent requests never reached the server, so sustained queue pressure
    /// rolls every one of them back.
    pub fn note_transport_pressure(&mut self, now_millis: u64) {
        if self.pending_open_warnings != 0
            || self.pending_control().is_some()
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
        let personal_expired = self.poll_control_timeout(now_millis);
        self.expire_overdue_requests(now_millis);
        self.poll_recovery(now_millis);
        personal_expired
    }

    /// Whether polling can change a deadline, request, or deferred close.
    pub(crate) fn timeout_work_due(&self, now_millis: u64) -> bool {
        self.controls.timeout_due(now_millis)
            || self.request_timeout_work_due(now_millis)
            || self.recovery_work_due(now_millis)
    }

    pub fn transport_closed(&mut self) {
        self.controls = control::WindowControls::default();
        self.personal = None;
        self.abandon_requests(|_| true);
        self.confirmed.clear_result();
        self.refold();
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
        self.pending_requests()
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
        let generation = storage.generation;
        let returning = self.return_crafting_on_close().unwrap_or_else(|error| {
            self.note_close_return_failure(error);
            true
        });
        self.controls.storage.desired_open = false;
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
            && (self.close_ready() || self.controls.storage.sent.is_some())
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
        let Some(kind) = self.window_kind() else {
            return;
        };
        self.discard_storage_window();
        self.clear_storage_inputs(kind);
    }

    /// A normal acknowledgement destroys this screen's request ownership, preserving backing.
    fn acknowledge_storage_close(&mut self, server_initiated: bool) {
        let Some(storage) = self.storage.as_ref() else {
            return;
        };
        if !server_initiated && storage.closing {
            let generation = storage.generation;
            self.retire_screen_requests(|request| request.storage_generation == Some(generation));
            self.discard_storage_window();
            self.refold();
        } else {
            self.close_storage();
        }
    }

    fn discard_storage_window(&mut self) {
        self.confirmed.clear_result();
        self.storage = None;
        self.clear_furnace_recipe();
        self.enchant_options = None;
        self.confirmed.clear_storage();
    }

    fn clear_storage_inputs(&mut self, kind: protocol::WindowKind) {
        self.clear_crafting(Some(kind));
        if self.confirmed.get(Cell::Cursor).is_some() {
            self.disown_abandoned_recovery(CellSurface::Player);
            self.disown_abandoned_recovery(CellSurface::Cursor);
            self.player_resync_required = true;
            self.cursor_resync_required = true;
        }
        self.refold();
    }

    fn finish_personal_close(&mut self, acknowledged: bool) {
        let generation = match self.personal {
            Some(
                PersonalWindow::Open { generation, .. }
                | PersonalWindow::Closing { generation, .. },
            ) => generation,
            _ => return,
        };
        self.confirmed.clear_result();
        if acknowledged {
            self.retire_screen_requests(|request| request.personal_generation == Some(generation));
            self.personal = None;
            self.refold();
            return;
        }
        self.abandon_requests(|pending| pending.personal_generation == Some(generation));
        self.personal = None;
        self.clear_window_inputs(false);
    }

    fn clear_window_inputs(&mut self, retain_confirmed_cursor: bool) {
        self.clear_crafting(None);
        if !retain_confirmed_cursor {
            self.drop_confirmed_cursor();
        }
        self.refold();
    }

    /// Inputs are actual inventory, not a disposable preview. Never erase an
    /// unreturned ingredient just because the screen stopped owning its UI.
    fn clear_crafting(&mut self, kind: Option<protocol::WindowKind>) {
        self.confirmed.clear_result();
        if crafting_close::screen_input_cells(kind).any(|cell| self.confirmed.get(cell).is_some()) {
            self.disown_abandoned_recovery(CellSurface::Crafting);
            self.crafting_resync_required = true;
            return;
        }
        if self
            .confirmed
            .occupied()
            .any(|(cell, _)| matches!(cell, Cell::Craft(_)))
        {
            // Another screen's inputs are retained independently of this close.
            return;
        }
        self.confirmed.clear_ui();
        self.crafting_resync_required = false;
        self.surface_refreshed(CellSurface::Crafting);
    }

    /// Requires a refresh independently of any abandoned request's eventual answer.
    fn mark_cell_recovery(&mut self, cell: Cell) {
        self.disown_abandoned_recovery(cell.surface());
        self.discard_requests(|request| request.locally_committed() && request.touches(cell));
        if let Some(held) = self.confirmed.get_mut(cell) {
            held.overlay = None;
        }
        self.require_cell_recovery(cell);
    }

    /// Marks a cell unverified while retaining its confirmed metadata.
    fn require_cell_recovery(&mut self, cell: Cell) {
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
