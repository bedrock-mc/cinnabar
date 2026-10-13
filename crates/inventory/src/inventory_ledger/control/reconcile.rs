use super::{ControlKind, ControlRequest, ControlWindow};
use crate::inventory_ledger::{
    InventoryGestureError, PersonalWindow, PlayerInventoryLedger, container_close_packet,
    open_inventory_packet,
};

impl PlayerInventoryLedger {
    /// Chooses a state transition without admitting a second request of either kind.
    pub(in crate::inventory_ledger) fn pending_control(&self) -> Option<ControlRequest> {
        for kind in [ControlKind::Storage, ControlKind::Personal] {
            let state = self.controls.state(kind);
            if state.sent.is_some() {
                continue;
            }
            if let Some(window) = state.confirmed
                && (!state.desired_open || state.reset_required)
                && self.close_ready()
                && !self.controls.close_outstanding(window.window_id)
            {
                // A personal ID may be reused by an active storage window; close storage first.
                if kind == ControlKind::Personal
                    && self
                        .controls
                        .storage
                        .confirmed
                        .is_some_and(|storage| storage.window_id == window.window_id)
                {
                    continue;
                }
                return Some(ControlRequest::Close(window));
            }
        }
        if self.controls.personal.sent.is_none()
            && self.controls.personal.confirmed.is_none()
            && self.controls.personal.desired_open
            && self.controls.storage.confirmed.is_none()
            && let Some(PersonalWindow::Opening {
                generation,
                admitted: false,
            }) = self.personal
        {
            return Some(ControlRequest::Open {
                generation,
                target_runtime_id: self.controls.personal_target,
            });
        }
        None
    }

    /// Encodes the next difference between confirmed state and the latest user intent.
    pub(in crate::inventory_ledger) fn pending_control_packet(
        &self,
    ) -> Result<Option<protocol::Packet>, InventoryGestureError> {
        self.pending_control()
            .map(|request| match request {
                ControlRequest::Open {
                    target_runtime_id, ..
                } => open_inventory_packet(target_runtime_id),
                ControlRequest::Close(window) => container_close_packet(window.window_id),
            })
            .transpose()
            .map_err(|_| InventoryGestureError::InvalidRequest)
    }

    /// Occupies the selected kind's admission slot only after transport accepts the packet.
    pub(in crate::inventory_ledger) fn mark_control_enqueued(&mut self, now_millis: u64) -> bool {
        let Some(request) = self.pending_control() else {
            return false;
        };
        self.controls.sent(request, now_millis);
        if matches!(request, ControlRequest::Open { .. })
            && let Some(PersonalWindow::Opening { admitted, .. }) = self.personal.as_mut()
        {
            *admitted = true;
        }
        true
    }

    /// Restores the screen implied by confirmed state once its outstanding transition resolves.
    pub(in crate::inventory_ledger) fn reconcile_personal(&mut self) {
        let state = &self.controls.personal;
        if state.sent.is_some()
            || state.reset_required
            || self.storage.is_some()
            || self.controls.personal_covered_by_storage_close()
        {
            return;
        }
        match (state.desired_open, state.confirmed) {
            (true, Some(window)) => {
                self.personal = Some(PersonalWindow::Open {
                    generation: window.generation,
                });
            }
            (true, None) if self.controls.storage.confirmed.is_none() => {
                if !matches!(self.personal, Some(PersonalWindow::Opening { .. })) {
                    let generation = self.next_open_generation;
                    self.next_open_generation = generation.wrapping_add(1).max(1);
                    self.personal = Some(PersonalWindow::Opening {
                        generation,
                        admitted: false,
                    });
                }
            }
            (false, Some(window)) => {
                if self.personal.is_some() {
                    self.personal = Some(PersonalWindow::Closing {
                        generation: window.generation,
                    });
                }
            }
            (false, None) => self.personal = None,
            _ => {}
        }
    }

    /// Requests closure of a received window through its kind's latest confirmed state.
    pub(in crate::inventory_ledger) fn reject_window(&mut self, window_id: i32, window_type: i8) {
        if !super::super::helpers::valid_raw_window_id(window_id) {
            return;
        }
        let generation = self.next_open_generation;
        self.next_open_generation = generation.wrapping_add(1).max(1);
        let window = ControlWindow {
            generation,
            window_id,
            window_type,
        };
        let state = self.controls.state_mut(window.kind());
        state.confirmed = Some(window);
        state.desired_open = false;
    }

    /// Resolves deadlines without retaining a barrier that can permanently prevent another open.
    pub(in crate::inventory_ledger) fn poll_control_timeout(&mut self, now_millis: u64) -> bool {
        let mut personal_expired = false;
        for kind in [ControlKind::Personal, ControlKind::Storage] {
            let Some(sent) = self.controls.expire(kind, now_millis) else {
                continue;
            };
            personal_expired |= kind == ControlKind::Personal && self.personal.is_some();
            match sent.request {
                ControlRequest::Open { generation, .. } => {
                    self.controls.personal.confirmed = Some(ControlWindow {
                        generation,
                        window_id: protocol::PLAYER_INVENTORY_WINDOW_ID,
                        window_type: super::super::PERSONAL_INVENTORY_WINDOW_TYPE,
                    });
                    self.controls.personal.reset_required = true;
                    let owned_screen = self
                        .personal
                        .is_some_and(|personal| personal.generation() == generation);
                    if owned_screen {
                        self.personal = None;
                    }
                    self.abandon_requests(|pending| {
                        pending.personal_generation == Some(generation)
                    });
                    if owned_screen {
                        self.clear_window_inputs(false);
                    }
                }
                ControlRequest::Close(_) => self.complete_sent_close(sent, false),
            }
        }
        self.reconcile_personal();
        personal_expired
    }
}
