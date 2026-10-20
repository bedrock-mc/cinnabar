use super::{ControlKind, ControlRequest};
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
                && !state.desired_open
                && self.close_ready()
                && !self.controls.close_outstanding(window.window_id)
            {
                return Some(ControlRequest::Close(window));
            }
        }
        if self.controls.personal.sent.is_none()
            && self.controls.personal.confirmed.is_none()
            && self.controls.personal.desired_open
            && self.controls.storage.server_window().is_none()
            && let Some(
                PersonalWindow::Opening {
                    generation,
                    admitted: false,
                }
                | PersonalWindow::Recovering { generation },
            ) = self.personal
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
        if let ControlRequest::Open { generation, .. } = request {
            self.personal = Some(PersonalWindow::Opening {
                generation,
                admitted: true,
            });
        }
        true
    }

    /// Applies the current window's intent independently of older outstanding controls.
    pub(in crate::inventory_ledger) fn reconcile_personal(&mut self) {
        let state = &self.controls.personal;
        if self.storage.is_some() {
            return;
        }
        if !state.desired_open
            && state.confirmed.is_none()
            && state.sent.is_none()
            && self.personal_close_pending()
        {
            return;
        }
        match (state.desired_open, state.confirmed) {
            (true, Some(window)) => {
                self.personal = Some(PersonalWindow::Open {
                    generation: window.generation,
                });
            }
            (true, None)
                if state.sent.is_none() && self.controls.storage.server_window().is_none() =>
            {
                if !matches!(
                    self.personal,
                    Some(PersonalWindow::Opening { .. } | PersonalWindow::Recovering { .. })
                ) {
                    if let Some(PersonalWindow::Closing {
                        generation,
                        awaiting_open: true,
                    }) = self.personal
                    {
                        self.personal = Some(PersonalWindow::Recovering { generation });
                    } else {
                        let generation = self.next_open_generation;
                        self.next_open_generation = generation.wrapping_add(1).max(1);
                        self.confirmed.clear_result();
                        self.refold();
                        self.personal = Some(PersonalWindow::Opening {
                            generation,
                            admitted: false,
                        });
                    }
                }
            }
            (false, Some(window)) => {
                if self.personal.is_some() {
                    self.personal = Some(PersonalWindow::Closing {
                        generation: window.generation,
                        awaiting_open: false,
                    });
                }
            }
            (false, None) if state.sent.is_none() => {
                if matches!(self.personal, Some(PersonalWindow::Closing { .. })) {
                    self.finish_personal_close(true);
                } else {
                    self.personal = None;
                }
            }
            _ => {}
        }
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
                    let owned_screen = self
                        .personal
                        .is_some_and(|personal| personal.generation() == generation);
                    if owned_screen && self.personal_generation_pending(generation) {
                        if matches!(self.personal, Some(PersonalWindow::Opening { .. })) {
                            self.personal = Some(PersonalWindow::Recovering { generation });
                        }
                        continue;
                    }
                    if owned_screen {
                        self.personal = None;
                        self.controls.personal.desired_open = false;
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
