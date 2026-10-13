//! Replies update confirmed state before reconciling the latest desired state.

use protocol::{ContainerCloseEvent, ContainerOpenEvent};

use super::{ControlKind, ControlRequest, ControlWindow, SentControl};
use crate::inventory_ledger::PlayerInventoryLedger;

impl PlayerInventoryLedger {
    /// Accepts the sole outstanding open, including one whose screen was superseded.
    pub(in crate::inventory_ledger) fn apply_personal_open(&mut self, open: ContainerOpenEvent) {
        let Some(window_id) = open.container.window_id else {
            self.note_unrouted_container();
            return;
        };
        let generation = if let Some(super::SentControl {
            request: ControlRequest::Open { generation, .. },
            ..
        }) = self.controls.personal.sent
        {
            self.controls.personal.sent = None;
            generation
        } else if let Some(window) = self.controls.personal.confirmed {
            window.generation
        } else {
            self.note_unrouted_container();
            // With no outstanding open or confirmed window, this may predate a completed reset.
            self.controls.personal.reset_required = true;
            let generation = self.next_open_generation;
            self.next_open_generation = generation.wrapping_add(1).max(1);
            generation
        };
        if !super::super::helpers::valid_raw_window_id(window_id) {
            self.controls.personal.desired_open = false;
            self.personal = None;
            self.note_unrouted_container();
            return;
        }
        self.controls.personal.confirmed = Some(ControlWindow {
            generation,
            window_id,
            window_type: open.window_type,
        });
        // A late open during timeout recovery supplies the actual ID for the required close.
        self.reconcile_personal();
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
            let Some(window) = self.controls.state(kind).confirmed.filter(|window| {
                close.container.window_id == Some(window.window_id)
                    && close.window_type == window.window_type
            }) else {
                continue;
            };
            let state = self.controls.state_mut(kind);
            state.desired_open = false;
            state.reset_required = false;
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

    /// Completes the admitted close and its captured personal generation with a matching ID.
    pub(in crate::inventory_ledger) fn complete_sent_close(
        &mut self,
        sent: SentControl,
        acknowledged: bool,
    ) {
        let ControlRequest::Close(window) = sent.request else {
            unreachable!()
        };
        self.complete_control_close(window, acknowledged);
        if let Some(personal) = self
            .controls
            .personal
            .confirmed
            .filter(|personal| sent.covers_personal(*personal))
        {
            self.complete_control_close(personal, acknowledged);
        }
    }

    /// Resolves only the confirmed generation targeted by this completed close.
    pub(in crate::inventory_ledger) fn complete_control_close(
        &mut self,
        window: ControlWindow,
        acknowledged: bool,
    ) {
        let kind = window.kind();
        let state = self.controls.state_mut(kind);
        if state.confirmed == Some(window) {
            state.confirmed = None;
            state.reset_required = false;
        }
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
