//! Replies update confirmed state before reconciling the latest desired state.

use protocol::{ContainerCloseEvent, ContainerOpenEvent};

use super::{ControlKind, ControlRequest, ControlWindow, SentControl};
use crate::inventory_ledger::{PersonalWindow, PlayerInventoryLedger};

impl PlayerInventoryLedger {
    /// Releases an admitted open when any supported server window arrives.
    pub(in crate::inventory_ledger) fn settle_open_request(&mut self) -> Option<u64> {
        if let Some(super::SentControl {
            request: ControlRequest::Open { generation, .. },
            ..
        }) = self.controls.personal.sent
        {
            self.controls.personal.sent = None;
            Some(generation)
        } else {
            None
        }
    }

    /// Opens the server-selected personal window regardless of earlier local requests.
    pub(in crate::inventory_ledger) fn apply_personal_open(&mut self, open: ContainerOpenEvent) {
        let Some(window_id) = open
            .container
            .window_id
            .filter(|id| super::super::helpers::valid_raw_window_id(*id))
        else {
            self.note_unrouted_container();
            return;
        };
        let requested_generation = self.settle_open_request();
        let generation = requested_generation
            .or(match self.personal {
                Some(
                    PersonalWindow::Closing {
                        generation,
                        awaiting_open: true,
                    }
                    | PersonalWindow::Opening { generation, .. },
                ) if self.personal_generation_pending(generation) => Some(generation),
                _ => None,
            })
            .or_else(|| {
                self.controls
                    .personal
                    .confirmed
                    .filter(|window| window.window_id == window_id)
                    .map(|window| window.generation)
            })
            .unwrap_or_else(|| {
                let generation = self.next_open_generation;
                self.next_open_generation = generation.wrapping_add(1).max(1);
                generation
            });
        self.retire_replaced_windows(Some(generation));
        self.controls.storage.confirmed = None;
        self.controls.storage.desired_open = false;
        self.controls.personal.confirmed = Some(ControlWindow {
            generation,
            window_id,
            window_type: open.window_type,
        });
        self.controls.personal.desired_open = true;
        self.personal = Some(crate::inventory_ledger::PersonalWindow::Open { generation });
    }

    /// Retires replaced screens without clearing inputs owned by their replacements.
    pub(in crate::inventory_ledger) fn retire_replaced_windows(
        &mut self,
        retained_personal_generation: Option<u64>,
    ) {
        if let Some(personal) = self.personal
            && Some(personal.generation()) != retained_personal_generation
        {
            let locally_closing = matches!(personal, PersonalWindow::Closing { .. });
            self.discard_pending_personal_open();
            self.finish_personal_close(locally_closing);
        }
        if let Some(storage) = self.storage.as_ref() {
            if storage.closing {
                self.acknowledge_storage_close(false);
            } else {
                let generation = storage.generation;
                self.abandon_requests(|request| request.storage_generation == Some(generation));
                self.discard_storage_window();
            }
        }
    }

    /// Keeps authoritative server commands separate from outstanding client acknowledgements.
    pub(in crate::inventory_ledger) fn apply_close(&mut self, close: ContainerCloseEvent) {
        if !close.server_initiated {
            if let Some(sent) = self.controls.answer_close(close) {
                self.complete_sent_close(sent, true);
            }
            return;
        }
        for kind in [ControlKind::Personal, ControlKind::Storage] {
            let Some(window) = self.controls.state(kind).server_window().filter(|window| {
                close.container.window_id == Some(window.window_id)
                    && close.window_type == window.window_type
            }) else {
                continue;
            };
            let state = self.controls.state_mut(kind);
            state.desired_open = false;
            // Storage requires confirmation unless our close is already on the wire.
            if kind == ControlKind::Personal
                || state
                    .sent
                    .is_some_and(|sent| sent.request == ControlRequest::Close(window))
            {
                state.confirmed = None;
            }
            match kind {
                ControlKind::Personal => {
                    if self
                        .personal
                        .is_some_and(|personal| personal.generation() == window.generation)
                    {
                        self.finish_personal_close(false);
                    }
                }
                ControlKind::Storage => {
                    if self
                        .storage
                        .as_ref()
                        .is_some_and(|storage| storage.generation == window.generation)
                    {
                        self.acknowledge_storage_close(true);
                    }
                }
            }
        }
    }

    /// Completes the admitted close without altering a replacement window.
    pub(in crate::inventory_ledger) fn complete_sent_close(
        &mut self,
        sent: SentControl,
        acknowledged: bool,
    ) {
        let ControlRequest::Close(window) = sent.request else {
            unreachable!()
        };
        let kind = window.kind();
        match kind {
            ControlKind::Personal => {
                if self
                    .personal
                    .is_some_and(|personal| personal.generation() == window.generation)
                {
                    self.finish_personal_close(acknowledged);
                }
            }
            ControlKind::Storage => {
                if self.storage.as_ref().is_some_and(|storage| {
                    storage.generation == window.generation && storage.closing
                }) {
                    self.acknowledge_storage_close(!acknowledged);
                }
            }
        }
    }
}
