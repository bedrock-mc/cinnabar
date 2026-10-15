//! Native sparse predictions: absolute cell values, request-id ownership, and
//! historic snapshots for responses that arrive after a later local gesture.

use protocol::{
    CanonicalCell, ContainerIdentity, StackRequestAction, StackResponseContainer,
    StackResponseSlot, project_container_cell,
};

use super::cells::{Cell, CellSurface, Cells, Held};
use super::overlay::DeltaGroup;
use super::response::merge_response_overlay;
use super::{InventoryGestureError, InventoryPendingState, PlayerInventoryLedger};

/// Retained predictions awaiting answers. New gestures are refused rather than evicting work
/// the server may still apply.
pub const MAX_PENDING_REQUESTS: usize = 256;

#[derive(Debug, Clone)]
pub(super) struct PendingRequest {
    pub(super) request_id: i32,
    pub(super) actions: Vec<StackRequestAction>,
    /// Anvil text a craft-optional action's filter index points into.
    pub(super) filter_strings: Vec<String>,
    pub(super) groups: Vec<DeltaGroup>,
    pub(super) state: InventoryPendingState,
    pub(super) transport_deadline_millis: Option<u64>,
    pub(super) deadline_millis: Option<u64>,
    /// No response arrived in time: the prediction stays until a response or a
    /// complete refresh of every surface it touched settles it.
    pub(super) timed_out: bool,
    /// Surfaces a timed-out request still needs a complete refresh of.
    pub(super) awaiting_refresh: Vec<CellSurface>,
    pub(super) session_generation: u64,
    pub(super) storage_generation: Option<u64>,
    pub(super) personal_generation: Option<u64>,
    pub(super) storage_identity: Option<ContainerIdentity>,
    /// Its storage window closed while it settles; its storage cells belong to no visible window.
    pub(super) storage_detached: bool,
    /// A split must end with distinct settled stack ids on surviving halves.
    pub(super) requires_distinct_stack_ids: bool,
    /// A merge whose capacity came from the session item registry.
    pub(super) registry_bound_merge: bool,
    /// The input routing a quick move chose; retired when its facts route elsewhere.
    pub(super) quick_move_route: Option<super::quick_move::QuickMoveRoute>,
    pub(super) predicted: Vec<CellPrediction>,
    /// A mine-block request riding PlayerAuthInput rather than its own packet.
    pub(super) mining: Option<MiningPrediction>,
    /// A drop key press with no window open, which leaves as a normal transaction.
    pub(super) world_drop: Option<WorldDrop>,
}

/// A reserved transaction identity and the admitted gameplay consumption after its drop.
#[derive(Debug, Clone, Copy)]
pub(super) struct WorldDrop {
    pub(super) legacy_request_id: i32,
    pub(super) consumed_after: u16,
}

/// The native sparse cell and its historic request snapshot. A later local
/// write replaces ownership, but the earlier snapshot still reconciles its
/// response into backing truth. Empty predictions also own a request id.
#[derive(Debug, Clone)]
pub(super) struct CellPrediction {
    pub(super) cell: Cell,
    pub(super) before: Option<Held>,
    pub(super) held: Option<Held>,
    pub(super) active: bool,
    /// No later authoritative write has invalidated this rollback source.
    pub(super) current: bool,
    /// A later rollback explicitly restored this empty result into backing.
    pub(super) owns_empty_backing: bool,
}

/// The durability a mine-block request predicts for one hotbar slot.
#[derive(Debug, Clone, Copy, Eq, PartialEq)]
pub(super) struct MiningPrediction {
    pub(super) slot: u8,
    pub(super) damage: i32,
    /// The item identity that mining may still correct after later local moves.
    pub(super) source_stack_id: Option<i32>,
}

/// Bounds outstanding mining predictions; the oldest is forgotten first.
const MAX_OUTSTANDING_MINING_REQUESTS: usize = 16;

impl PendingRequest {
    /// An admitted normal transaction is retained history, not unanswered gesture work.
    pub(super) fn locally_committed(&self) -> bool {
        self.world_drop.is_some() && self.state == InventoryPendingState::AwaitingResponse
    }

    pub(super) fn touched(&self) -> impl Iterator<Item = Cell> + '_ {
        self.groups
            .iter()
            .flat_map(DeltaGroup::touched)
            .filter(|cell| !(self.storage_detached && matches!(cell, Cell::Storage(_))))
    }

    pub(super) fn touches(&self, cell: Cell) -> bool {
        self.touched().any(|touched| touched == cell)
    }
}

/// Bounds abandoned requests remembered for a late rejection.
const MAX_ABANDONED_REQUESTS: usize = 16;

/// An admitted request dropped unanswered.
#[derive(Debug, Clone)]
pub(super) struct AbandonedRequest {
    request_id: i32,
    /// Touched surfaces not yet authoritatively refreshed.
    touched: Vec<CellSurface>,
    /// The touched surfaces whose recovery this request answers for.
    recovering: Vec<CellSurface>,
}

impl PlayerInventoryLedger {
    /// Backing truth covered by the latest active absolute sparse cells.
    pub(super) fn view(&self) -> &Cells {
        self.view.as_ref().unwrap_or(&self.confirmed)
    }

    /// Rebuilds the predicted view over confirmed cells; with no active prediction the view
    /// is confirmed truth itself, so nothing is cloned.
    pub(super) fn refold(&mut self) {
        let mut predictions = self
            .queue
            .iter()
            .flat_map(|request| &request.predicted)
            .filter(|prediction| prediction.active)
            .peekable();
        self.view = predictions.peek().is_some().then(|| {
            let mut view = self.confirmed.clone();
            for prediction in predictions {
                view.set(prediction.cell, prediction.held.clone());
            }
            view
        });
        self.observe_furnace_cells();
    }

    pub(super) fn ensure_queue_capacity(&self) -> Result<(), InventoryGestureError> {
        if self.pending_requests().count() >= MAX_PENDING_REQUESTS {
            Err(InventoryGestureError::Busy)
        } else {
            Ok(())
        }
    }

    /// Queues one request built against the current view.
    pub(super) fn enqueue(&mut self, mut request: PendingRequest) {
        let server_authoritative = self.authority == Some(protocol::InventoryAuthority::Server);
        if server_authoritative {
            self.bind_request_dependencies(&mut request.actions, request.request_id);
        }
        tracing::debug!(target: "cinnabar::inventory_requests",
            request_id = request.request_id, actions = ?request.actions,
            "inventory request predicted");
        let before = self.view().clone();
        let mut predicted = before.clone();
        let applied = request
            .groups
            .iter()
            .all(|group| group.apply(&mut predicted));
        debug_assert!(applied, "gestures are built against the current view");
        let mut unique = Vec::new();
        for cell in request.touched() {
            if !unique.contains(&cell) {
                unique.push(cell);
            }
        }
        for previous in self
            .queue
            .iter_mut()
            .flat_map(|request| &mut request.predicted)
        {
            if unique.contains(&previous.cell) {
                previous.active = false;
            }
        }
        request.predicted = unique
            .into_iter()
            .map(|cell| {
                let mut held = predicted.get(cell).cloned();
                if server_authoritative && let Some(held) = &mut held {
                    // Stamp each changed prediction with the request that owns it.
                    held.source_stack_id.get_or_insert(request.request_id);
                    held.stack.stack_network_id = request.request_id;
                }
                CellPrediction {
                    cell,
                    before: before.get(cell).cloned(),
                    held,
                    active: true,
                    current: true,
                    owns_empty_backing: false,
                }
            })
            .collect();
        if self.queue.len() >= MAX_PENDING_REQUESTS
            && let Some(index) = self
                .queue
                .iter()
                .position(PendingRequest::locally_committed)
        {
            self.queue.remove(index);
        }
        self.queue.push_back(request);
        self.refold();
    }

    /// Negative odd request ids name pending sparse cells. The cell address
    /// distinguishes even two halves stamped by the same split request. A
    /// negative even id names what a sent drop transaction left behind.
    pub(super) fn awaiting_identity(&self, held: &Held) -> bool {
        self.stack_awaiting_identity(&held.stack)
    }

    /// Whether a stack lacks a server id, a settled legacy id, or a queued sparse owner.
    pub(super) fn stack_awaiting_identity(&self, stack: &protocol::NetworkItemStack) -> bool {
        if self.authority == Some(protocol::InventoryAuthority::Client) {
            return false;
        }
        let id = stack.stack_network_id;
        id <= 0
            && !protocol::is_legacy_request_id(id)
            && !(id < -1
                && id & 1 != 0
                && self.queue.iter().any(|request| request.request_id == id))
    }

    pub(super) fn request_is_current(&self, request: &PendingRequest) -> bool {
        request.session_generation == self.session_generation
            && request.personal_generation.is_none_or(|generation| {
                self.personal_settling(generation)
                    || self
                        .personal
                        .as_ref()
                        .map(super::personal::PersonalWindow::generation)
                        == Some(generation)
            })
            && self.storage_request_is_current(request)
    }

    /// A storage request answers its own open window, or the closed one it settles against.
    fn storage_request_is_current(&self, request: &PendingRequest) -> bool {
        let current = self.storage.as_ref();
        let identity = match request.storage_generation {
            Some(generation) if current.map(|storage| storage.generation) != Some(generation) => {
                match self.storage_settling(generation) {
                    Some(identity) => identity,
                    None => return false,
                }
            }
            _ => current.and_then(|storage| storage.identity),
        };
        request
            .storage_identity
            .is_none_or(|expected| identity == Some(expected))
    }

    pub(super) fn remove_unanswered_mining(&mut self, request_id: i32) {
        self.queue
            .retain(|request| request.request_id != request_id || request.mining.is_none());
    }

    /// Rolls back only backing owned by this request and rebases its later snapshots.
    pub(super) fn reject_request(&mut self, request: &PendingRequest) {
        for prediction in request
            .predicted
            .iter()
            .filter(|prediction| prediction.current)
        {
            self.publish_request_cell(
                request,
                prediction.cell,
                prediction.held.as_ref(),
                prediction.before.clone(),
                true,
            );
        }
    }

    /// Invalidates rollback sources older than this write, retiring completed local history.
    pub(super) fn invalidate_request_sources(&mut self, cell: Cell, newer: i32) {
        for prediction in self
            .queue
            .iter_mut()
            .filter(|request| request.request_id > newer)
            .flat_map(|request| &mut request.predicted)
            .filter(|prediction| prediction.cell == cell)
        {
            prediction.current = false;
        }
        self.queue.retain(|request| {
            request.request_id <= newer || !request.locally_committed() || !request.touches(cell)
        });
    }

    /// Publishes a settled cell without overwriting a newer admitted transaction.
    fn publish_request_cell(
        &mut self,
        request: &PendingRequest,
        cell: Cell,
        from: Option<&Held>,
        to: Option<Held>,
        rejected: bool,
    ) {
        let newer = self.queue.iter().any(|later| {
            later.request_id < request.request_id
                && later.locally_committed()
                && later.touches(cell)
        });
        if !newer && (!rejected || self.request_owns_backing(request, cell, from)) {
            if !rejected {
                self.invalidate_request_sources(cell, request.request_id);
            }
            self.confirmed.set(cell, to.clone());
            if rejected
                && to.is_none()
                && let Some(previous) = self.queue.iter_mut().rev().find(|previous| {
                    previous.request_id > request.request_id && previous.touches(cell)
                })
                && let Some(prediction) = previous.predicted.iter_mut().find(|p| p.cell == cell)
                && prediction.current
                && prediction.held.is_none()
            {
                prediction.owns_empty_backing = true;
            }
        }
        self.rebase_predictions(request.request_id, cell, from, to);
    }

    /// Empty backing needs an admitted transaction or an explicit rollback restoring its owner.
    fn request_owns_backing(
        &self,
        request: &PendingRequest,
        cell: Cell,
        predicted: Option<&Held>,
    ) -> bool {
        (predicted.is_some()
            || request.locally_committed()
            || request
                .predicted
                .iter()
                .any(|prediction| prediction.cell == cell && prediction.owns_empty_backing))
            && same_name(self.confirmed.get(cell), predicted)
    }

    /// Rebases all later rollback sources and replays only requests not yet transported.
    pub(super) fn rebase_predictions(
        &mut self,
        predecessor: i32,
        cell: Cell,
        from: Option<&Held>,
        to: Option<Held>,
    ) {
        let mut replacements = vec![(cell, from.cloned(), to)];
        let mut queue = std::mem::take(&mut self.queue);
        let mut invalid = None;
        'requests: for request in queue
            .iter_mut()
            .filter(|request| request.request_id < predecessor)
        {
            let mut changed = false;
            for prediction in request
                .predicted
                .iter_mut()
                .filter(|prediction| prediction.current)
            {
                if let Some((_, _, replacement)) =
                    replacements.iter().rev().find(|(cell, old, _)| {
                        *cell == prediction.cell
                            && same_name(prediction.before.as_ref(), old.as_ref())
                    })
                {
                    let same_item = match (prediction.before.as_ref(), replacement.as_ref()) {
                        (Some(before), Some(after)) => Self::same_item(&before.stack, &after.stack),
                        (None, None) => true,
                        _ => false,
                    };
                    if request.state == InventoryPendingState::AwaitingTransport && !same_item {
                        invalid = Some(request.request_id);
                        break 'requests;
                    }
                    prediction.before = replacement.clone();
                    changed = true;
                }
            }
            if changed && request.state == InventoryPendingState::AwaitingTransport {
                let mut cells = self.confirmed.clone();
                for prediction in &request.predicted {
                    cells.set(prediction.cell, prediction.before.clone());
                }
                for slot in request
                    .actions
                    .iter_mut()
                    .flat_map(super::helpers::action_slots_mut)
                {
                    if slot.stack_network_id == request.request_id {
                        continue;
                    }
                    if let Some(cell) = self.request_cell(*slot) {
                        slot.stack_network_id = cells
                            .get(cell)
                            .map_or(0, |held| held.stack.stack_network_id);
                    }
                }
                if !request.groups.iter_mut().all(|group| {
                    group.rebind(&cells);
                    group.apply(&mut cells)
                }) {
                    invalid = Some(request.request_id);
                    break;
                }
                for prediction in &mut request.predicted {
                    let old = prediction.held.take();
                    prediction.held = cells.get(prediction.cell).cloned();
                    if self.authority == Some(protocol::InventoryAuthority::Server)
                        && let Some(held) = &mut prediction.held
                    {
                        held.stack.stack_network_id = request.request_id;
                    }
                    replacements.push((prediction.cell, old, prediction.held.clone()));
                }
            }
        }
        self.queue = queue;
        if let Some(id) = invalid {
            self.abandon_requests(|request| request.request_id == id);
        }
    }

    /// Applies accepted corrections from the request's retained cell snapshots.
    pub(super) fn settle(
        &mut self,
        request: &PendingRequest,
        containers: &[StackResponseContainer],
    ) {
        if !self.request_is_current(request) {
            // The window that bound this request is gone; its close path
            // already owns recovery.
            return;
        }
        if containers.iter().any(|container| {
            matches!(
                project_container_cell(&container.container, 0),
                Some(CanonicalCell::GenericStorage { .. })
            ) && storage_identity_mismatch(request.storage_identity, container.container)
        }) {
            self.recover_request(request);
            return;
        }
        for container in containers.iter() {
            for correction in container.slots.iter() {
                let Some(cell) =
                    self.retained_response_cell(&container.container, u16::from(correction.slot))
                else {
                    self.note_unrouted_container();
                    continue;
                };
                let Some(requested) = self.retained_response_cell(
                    &container.container,
                    u16::from(correction.hotbar_slot),
                ) else {
                    self.note_unrouted_container();
                    continue;
                };
                if request.storage_detached
                    && (matches!(cell, Cell::Storage(_)) || matches!(requested, Cell::Storage(_)))
                {
                    continue;
                }
                // A mining correction never touches a cell a later gesture owns.
                let owned = request.mining.is_some_and(|mining| {
                    self.confirmed
                        .get(cell)
                        .is_none_or(|held| held.source_stack_id != mining.source_stack_id)
                }) || (request.mining.is_some()
                    && self
                        .queue
                        .iter()
                        .any(|later| later.request_id < request.request_id && later.touches(cell)));
                if !owned {
                    self.apply_correction(request, requested, cell, correction);
                }
            }
        }
        if request.requires_distinct_stack_ids && !self.split_is_distinct(request) {
            self.recover_request(request);
        }
    }

    /// Applies one accepted count/stack-id/overlay correction to server truth.
    fn apply_correction(
        &mut self,
        request: &PendingRequest,
        requested: Cell,
        cell: Cell,
        correction: &StackResponseSlot,
    ) {
        if (correction.count == 0 && correction.item_stack_id > 0)
            || (correction.count > 0
                && correction.item_stack_id <= 0
                && !protocol::is_legacy_request_id(correction.item_stack_id))
        {
            tracing::warn!(target: "cinnabar::inventory_requests",
                request_id = request.request_id, slot = correction.slot,
                count = correction.count, stack_id = correction.item_stack_id,
                "skipping invalid response count/stack-id pair");
            self.note_unrouted_container();
            return;
        }
        let prediction = request
            .predicted
            .iter()
            .find(|prediction| prediction.cell == requested);
        if request.locally_committed() && prediction.is_some_and(|prediction| !prediction.current) {
            return;
        }
        if let Some(prediction) = prediction
            && !prediction.active
            && !(prediction.current
                && self.request_owns_backing(request, cell, prediction.held.as_ref()))
            && !self
                .queue
                .iter()
                .flat_map(|pending| &pending.predicted)
                .any(|prediction| prediction.cell == requested && prediction.active)
        {
            // Vanilla treats this as a missing prediction, not a historic one,
            // after a newer owner was removed.
            self.note_unrouted_container();
            return;
        }
        self.note_authoritative_write(cell);
        let count = u16::from(correction.count)
            .saturating_sub(request.world_drop.map_or(0, |drop| drop.consumed_after));
        let mut held = prediction
            .and_then(|prediction| prediction.held.as_ref().or(prediction.before.as_ref()))
            .or_else(|| self.confirmed.get(cell))
            .cloned();
        if count == 0 {
            held = None;
        } else if let Some(held) = &mut held {
            held.stack.count = count;
            held.stack.stack_network_id = request
                .world_drop
                .filter(|drop| drop.consumed_after != 0)
                .and_then(|_| prediction.and_then(|prediction| prediction.held.as_ref()))
                .map_or(correction.item_stack_id, |held| held.stack.stack_network_id);
            held.acknowledged_request_id = Some(request.request_id);
            merge_response_overlay(&mut held.overlay, correction);
        } else if !matches!(cell, Cell::Craft(_) | Cell::CreatedOutput) {
            self.mark_cell_recovery(cell);
        }
        self.publish_request_cell(
            request,
            cell,
            prediction.and_then(|prediction| prediction.held.as_ref()),
            held,
            false,
        );
    }

    fn split_is_distinct(&self, request: &PendingRequest) -> bool {
        let mut cells: Vec<Cell> = Vec::new();
        let mut ids: Vec<i32> = Vec::new();
        for cell in request.touched() {
            if cells.contains(&cell) {
                continue;
            }
            cells.push(cell);
            if let Some(held) = self.confirmed.get(cell) {
                let id = held.stack.stack_network_id;
                if (id <= 0 && !protocol::is_legacy_request_id(id)) || ids.contains(&id) {
                    return false;
                }
                ids.push(id);
            }
        }
        true
    }

    pub(super) fn recover_request(&mut self, request: &PendingRequest) {
        let touched: Vec<Cell> = request.touched().collect();
        for cell in touched {
            self.mark_cell_recovery(cell);
        }
    }

    /// Recovers an admitted request dropped unanswered, remembering which
    /// surfaces that put into recovery for [`Self::settle_abandoned_rejection`].
    fn recover_abandoned(&mut self, request: &PendingRequest) {
        let mut touched: Vec<CellSurface> = Vec::new();
        for surface in request.touched().map(|cell| cell.surface()) {
            if !touched.contains(&surface) {
                touched.push(surface);
            }
        }
        let mut recovering: Vec<CellSurface> = touched
            .iter()
            .copied()
            .filter(|surface| !self.surface_recovering(*surface))
            .collect();
        for cell in request.touched() {
            self.require_cell_recovery(cell);
        }
        recovering.retain(|surface| self.surface_recovering(*surface));
        if self.abandoned.len() >= MAX_ABANDONED_REQUESTS {
            self.abandoned.pop_front();
        }
        self.abandoned.push_back(AbandonedRequest {
            request_id: request.request_id,
            touched,
            recovering,
        });
    }

    /// Releases only the ambiguity owned by the rejected request, preserving
    /// independent recovery and unanswered requests on the same surface.
    pub(super) fn settle_abandoned_rejection(&mut self, request_id: i32) {
        let Some(index) = self
            .abandoned
            .iter()
            .position(|abandoned| abandoned.request_id == request_id)
        else {
            return;
        };
        let settled = self.abandoned.remove(index).expect("index observed");
        for surface in settled.recovering {
            if let Some(other) = self
                .abandoned
                .iter_mut()
                .find(|abandoned| abandoned.touched.contains(&surface))
            {
                if !other.recovering.contains(&surface) {
                    other.recovering.push(surface);
                }
                continue;
            }
            match surface {
                CellSurface::Player => self.player_resync_required = false,
                CellSurface::Cursor => self.cursor_resync_required = false,
                CellSurface::Armor => self.armor_resync_required = false,
                CellSurface::Offhand => self.offhand_resync_required = false,
                CellSurface::Crafting => self.crafting_resync_required = false,
                // The window the request addressed is gone or replaced.
                CellSurface::Storage => {}
            }
        }
    }

    /// Independent invalidation cannot be settled by an abandoned request's rejection.
    pub(super) fn disown_abandoned_recovery(&mut self, surface: CellSurface) {
        for abandoned in &mut self.abandoned {
            abandoned.recovering.retain(|owned| *owned != surface);
        }
    }

    /// Drops matching requests. Unsent ones roll back together with every later
    /// unsent request built on them; admitted ones become ambiguous and mark
    /// their cells for authoritative recovery. Client authority restores the
    /// newest surviving prediction on each cell before refolding.
    pub(super) fn abandon_requests(&mut self, matches: impl Fn(&PendingRequest) -> bool) {
        let mut rolling_back = false;
        let mut index = 0;
        while index < self.queue.len() {
            let request = &self.queue[index];
            let unsent = request.state == InventoryPendingState::AwaitingTransport;
            if (unsent && rolling_back) || matches(request) {
                let request = self.queue.remove(index).expect("index is in range");
                if unsent {
                    rolling_back = true;
                } else if !request.locally_committed() {
                    self.recover_abandoned(&request);
                }
                continue;
            }
            index += 1;
        }
        if self.authority == Some(protocol::InventoryAuthority::Client) {
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
        self.refold();
    }

    /// Detects timeout work while preserving shared presentation owners on quiet polls.
    pub(super) fn request_timeout_work_due(&self, now_millis: u64) -> bool {
        self.queue.iter().any(|request| {
            if request.mining.is_some() && request.deadline_millis.is_none() {
                return true;
            }
            let overdue = request
                .deadline_millis
                .is_some_and(|deadline| now_millis >= deadline);
            overdue
                && (request.mining.is_some()
                    || (request.state == InventoryPendingState::AwaitingResponse
                        && !request.timed_out))
        })
    }

    /// Marks every overdue admitted request timed out. Predictions stay: the
    /// server may still apply and answer them.
    pub(super) fn expire_overdue_requests(&mut self, now_millis: u64) {
        // A mining request's clock starts at the first poll after it rides an
        // input; an unanswered one is simply forgotten.
        for request in &mut self.queue {
            if request.mining.is_some() && request.deadline_millis.is_none() {
                request.deadline_millis =
                    Some(now_millis.saturating_add(super::INVENTORY_REQUEST_TIMEOUT_MILLIS));
            }
        }
        let overdue = |request: &PendingRequest| {
            request
                .deadline_millis
                .is_some_and(|deadline| now_millis >= deadline)
        };
        let before = self.queue.len();
        self.queue
            .retain(|request| request.mining.is_none() || !overdue(request));
        let mut changed = self.queue.len() != before;
        for request in &mut self.queue {
            // A response already being settled is not an unanswered timeout.
            if request.state == InventoryPendingState::AwaitingResponse
                && request.mining.is_none()
                && !request.timed_out
                && overdue(request)
            {
                request.timed_out = true;
                request.awaiting_refresh = request.touched().map(Cell::surface).collect();
                request.awaiting_refresh.dedup();
                changed = true;
            }
        }
        if changed {
            self.refold();
        }
    }

    /// Records a complete refresh of `surface`, retiring timed-out requests
    /// once every surface they touched has been refreshed.
    pub(super) fn surface_refreshed(&mut self, surface: CellSurface) {
        self.abandoned.retain_mut(|abandoned| {
            abandoned.touched.retain(|touched| *touched != surface);
            abandoned.recovering.retain(|owned| *owned != surface);
            !abandoned.touched.is_empty()
        });
        let mut retired = false;
        self.queue.retain_mut(|request| {
            request
                .awaiting_refresh
                .retain(|pending| *pending != surface);
            let done = request.timed_out && request.awaiting_refresh.is_empty();
            retired |= done;
            !done
        });
        if retired {
            self.refold();
        }
    }

    /// Whether a timed-out request still leaves `surface` unverified.
    pub(super) fn surface_awaiting_refresh(&self, surface: CellSurface) -> bool {
        self.queue
            .iter()
            .any(|request| request.awaiting_refresh.contains(&surface))
    }

    /// Queues a mine-block prediction at its wire position: after every
    /// admitted request, ahead of unsent ones. `None` when the queue is full,
    /// so the break goes out without a request.
    pub(super) fn enqueue_mining(&mut self, slot: u8, damage: i32) -> Option<i32> {
        self.ensure_queue_capacity().ok()?;
        let request_id = self.next_request_id;
        self.next_request_id = request_id.checked_sub(2)?;
        let mining: Vec<usize> = self
            .queue
            .iter()
            .enumerate()
            .filter(|(_, request)| request.mining.is_some())
            .map(|(index, _)| index)
            .collect();
        if mining.len() >= MAX_OUTSTANDING_MINING_REQUESTS {
            self.queue.remove(mining[0]);
        }
        let position = self
            .queue
            .iter()
            .position(|request| request.state == InventoryPendingState::AwaitingTransport)
            .unwrap_or(self.queue.len());
        self.queue.insert(
            position,
            PendingRequest {
                request_id,
                actions: Vec::new(),
                filter_strings: Vec::new(),
                groups: Vec::new(),
                state: InventoryPendingState::AwaitingResponse,
                transport_deadline_millis: None,
                deadline_millis: None,
                timed_out: false,
                awaiting_refresh: Vec::new(),
                session_generation: self.session_generation,
                storage_generation: None,
                personal_generation: None,
                storage_identity: None,
                storage_detached: false,
                requires_distinct_stack_ids: false,
                registry_bound_merge: false,
                quick_move_route: None,
                predicted: Vec::new(),
                mining: Some(MiningPrediction {
                    slot,
                    damage,
                    source_stack_id: self
                        .view()
                        .get(Cell::Inventory(slot))
                        .and_then(|held| held.source_stack_id),
                }),
                world_drop: None,
            },
        );
        Some(request_id)
    }
}

/// Compares retained stack names, including empty results that still own a cell.
fn same_name(left: Option<&Held>, right: Option<&Held>) -> bool {
    left.map(|held| held.stack.stack_network_id) == right.map(|held| held.stack.stack_network_id)
}

fn storage_identity_mismatch(
    expected: Option<ContainerIdentity>,
    identity: ContainerIdentity,
) -> bool {
    expected.is_none_or(|expected| {
        // Legacy windows without a dynamic identity may be echoed with a zero-filled ID.
        let legacy_zero = expected.dynamic_id.is_none()
            && expected
                .window_id
                .is_some_and(|id| id != protocol::DYNAMIC_STORAGE_WINDOW_ID)
            && identity.dynamic_id == Some(0);
        identity.window_id.is_some()
            || expected.slot_type != identity.slot_type
            || (expected.dynamic_id != identity.dynamic_id && !legacy_zero)
    })
}

#[cfg(test)]
impl PlayerInventoryLedger {
    pub(super) fn newest_action(&self) -> Option<StackRequestAction> {
        self.queue.back()?.actions.first().cloned()
    }

    pub(super) fn newest_request(&self) -> Option<&PendingRequest> {
        self.queue.back()
    }

    pub(super) fn set_confirmed_overlay(
        &mut self,
        cell: Cell,
        overlay: super::StackResponseOverlay,
    ) {
        self.confirmed.get_mut(cell).expect("occupied cell").overlay = Some(overlay);
        self.refold();
    }

    pub(super) fn confirmed_stack(&self, cell: Cell) -> Option<&protocol::NetworkItemStack> {
        self.confirmed.get(cell).map(|held| &held.stack)
    }
}
