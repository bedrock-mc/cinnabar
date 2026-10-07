//! Native sparse predictions: absolute cell values, request-id ownership, and
//! historic snapshots for responses that arrive after a later local gesture.

use std::sync::Arc;

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
    /// Corrections retained while the response handler settles this request.
    pub(super) accepted: Option<Arc<[StackResponseContainer]>>,
    pub(super) session_generation: u64,
    pub(super) storage_generation: Option<u64>,
    pub(super) personal_generation: Option<u64>,
    pub(super) storage_identity: Option<ContainerIdentity>,
    /// A split must end with distinct positive server ids on surviving halves.
    pub(super) requires_distinct_stack_ids: bool,
    /// A merge whose capacity came from the session item registry.
    pub(super) registry_bound_merge: bool,
    pub(super) predicted: Vec<CellPrediction>,
    /// A mine-block request riding PlayerAuthInput rather than its own packet.
    pub(super) mining: Option<MiningPrediction>,
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
}

/// The durability a mine-block request predicts for one hotbar slot.
#[derive(Debug, Clone, Copy, Eq, PartialEq)]
pub(super) struct MiningPrediction {
    pub(super) slot: u8,
    pub(super) damage: i32,
}

/// Bounds outstanding mining predictions; the oldest is forgotten first.
const MAX_OUTSTANDING_MINING_REQUESTS: usize = 16;

impl PendingRequest {
    pub(super) fn touched(&self) -> impl Iterator<Item = Cell> + '_ {
        self.groups.iter().flat_map(DeltaGroup::touched)
    }

    pub(super) fn touches(&self, cell: Cell) -> bool {
        self.touched().any(|touched| touched == cell)
    }
}

impl PlayerInventoryLedger {
    /// Backing truth covered by the latest active absolute sparse cells.
    pub(super) fn view(&self) -> &Cells {
        self.view.as_ref().unwrap_or(&self.confirmed)
    }

    pub(super) fn refold(&mut self) {
        if self.queue.is_empty() {
            self.view = None;
            return;
        }
        let mut view = self.confirmed.clone();
        for prediction in self.queue.iter().flat_map(|request| &request.predicted) {
            if prediction.active {
                view.set(prediction.cell, prediction.held.clone());
            }
        }
        self.view = Some(view);
    }

    pub(super) fn ensure_queue_capacity(&self) -> Result<(), InventoryGestureError> {
        if self.queue.len() >= MAX_PENDING_REQUESTS {
            Err(InventoryGestureError::Busy)
        } else {
            Ok(())
        }
    }

    /// Queues one request built against the current view.
    pub(super) fn enqueue(&mut self, mut request: PendingRequest) {
        self.bind_request_dependencies(&mut request.actions, request.request_id);
        tracing::debug!(target: "bedrock_client::inventory_requests",
            request_id = request.request_id, actions = ?request.actions,
            "inventory request predicted");
        let before = self.view().clone();
        let mut predicted = before.clone();
        let applied = request
            .groups
            .iter()
            .all(|group| group.apply(&mut predicted));
        debug_assert!(applied, "gestures are built against the current view");
        let mut touched: Vec<Cell> = request.touched().collect();
        let mut unique = Vec::new();
        for cell in touched.drain(..) {
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
                if let Some(held) = &mut held {
                    // Stamp each changed prediction with the request that owns it.
                    held.stack.stack_network_id = request.request_id;
                }
                CellPrediction {
                    cell,
                    before: before.get(cell).cloned(),
                    held,
                    active: true,
                }
            })
            .collect();
        self.queue.push_back(request);
        self.refold();
    }

    /// Negative odd request ids name pending sparse cells. The cell address
    /// distinguishes even two halves stamped by the same split request.
    pub(super) fn awaiting_identity(&self, held: &Held) -> bool {
        let id = held.stack.stack_network_id;
        id <= 0
            && !(id < -1
                && id & 1 != 0
                && self.queue.iter().any(|request| request.request_id == id))
    }

    pub(super) fn request_is_current(&self, request: &PendingRequest) -> bool {
        request.session_generation == self.session_generation
            && request.personal_generation.is_none_or(|generation| {
                self.personal
                    .as_ref()
                    .map(super::personal::PersonalWindow::generation)
                    == Some(generation)
            })
            && request.storage_generation.is_none_or(|generation| {
                self.storage.as_ref().map(|storage| storage.generation) == Some(generation)
            })
            && request.storage_identity.is_none_or(|identity| {
                self.storage.as_ref().and_then(|storage| storage.identity) == Some(identity)
            })
    }

    pub(super) fn remove_unanswered_mining(&mut self, request_id: i32) {
        let before = self.queue.len();
        self.queue.retain(|request| {
            request.request_id != request_id
                || request.mining.is_none()
                || request.accepted.is_some()
        });
        if self.queue.len() != before {
            self.settle_accepted_heads();
        }
    }

    /// Each answer settles its own backing cells immediately; a later active
    /// sparse prediction remains visible over an earlier historic answer.
    pub(super) fn settle_accepted_heads(&mut self) {
        while let Some(index) = self
            .queue
            .iter()
            .position(|request| request.accepted.is_some())
        {
            let request = self.queue.remove(index).expect("index observed");
            self.settle(&request);
        }
    }

    fn settle(&mut self, request: &PendingRequest) {
        let containers = request
            .accepted
            .clone()
            .expect("only accepted heads settle");
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
                // A mining correction never touches a cell a later gesture owns.
                let owned =
                    request.mining.is_some() && self.queue.iter().any(|later| later.touches(cell));
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
        if (correction.count == 0) != (correction.item_stack_id <= 0) {
            tracing::warn!(target: "bedrock_client::inventory_requests",
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
        if let Some(prediction) = prediction
            && !prediction.active
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
        if correction.count == 0 {
            self.confirmed.set(cell, None);
            return;
        }
        if let Some(prediction) = prediction {
            // Vanilla's slot prediction and the historic path
            // use the requested slot's item, not the destination's.
            let held = prediction
                .held
                .as_ref()
                .or(prediction.before.as_ref())
                .cloned();
            self.confirmed.set(cell, held);
        }
        match self.confirmed.get_mut(cell) {
            Some(held) => {
                held.stack.count = u16::from(correction.count);
                held.stack.stack_network_id = correction.item_stack_id;
                merge_response_overlay(&mut held.overlay, correction);
            }
            // Screen cells restate through slot updates that follow the response.
            None if matches!(cell, Cell::Craft(_) | Cell::CreatedOutput) => {}
            None => self.mark_cell_recovery(cell),
        }
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
                if id <= 0 || ids.contains(&id) {
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

    /// Drops matching requests. Unsent ones roll back together with every later
    /// unsent request built on them; admitted ones become ambiguous and mark
    /// their cells for authoritative recovery.
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
                } else {
                    self.recover_request(&request);
                }
                continue;
            }
            index += 1;
        }
        self.settle_accepted_heads();
        self.refold();
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
        self.queue.retain(|request| {
            request.mining.is_none() || request.accepted.is_some() || !overdue(request)
        });
        let mut changed = self.queue.len() != before;
        for request in &mut self.queue {
            // A response already being settled is not an unanswered timeout.
            if request.state == InventoryPendingState::AwaitingResponse
                && request.mining.is_none()
                && request.accepted.is_none()
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
            self.settle_accepted_heads();
            self.refold();
        }
    }

    /// Records a complete refresh of `surface`, retiring timed-out requests
    /// once every surface they touched has been refreshed.
    pub(super) fn surface_refreshed(&mut self, surface: CellSurface) {
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
            self.settle_accepted_heads();
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
        if self.queue.len() >= MAX_PENDING_REQUESTS {
            return None;
        }
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
            // Evict the oldest unanswered request; accepted ones still carry corrections.
            let evicted = mining
                .iter()
                .copied()
                .find(|&index| self.queue[index].accepted.is_none())
                .unwrap_or(mining[0]);
            self.queue.remove(evicted);
            self.settle_accepted_heads();
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
                accepted: None,
                session_generation: self.session_generation,
                storage_generation: None,
                personal_generation: None,
                storage_identity: None,
                requires_distinct_stack_ids: false,
                registry_bound_merge: false,
                predicted: Vec::new(),
                mining: Some(MiningPrediction { slot, damage }),
            },
        );
        Some(request_id)
    }
}

fn storage_identity_mismatch(
    expected: Option<ContainerIdentity>,
    identity: ContainerIdentity,
) -> bool {
    expected.is_none_or(|expected| {
        identity.window_id.is_some()
            || expected.slot_type != identity.slot_type
            || expected.dynamic_id != identity.dynamic_id
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
