//! Replies update confirmed state before reconciling the latest desired state.

use protocol::{ContainerCloseEvent, ContainerOpenEvent};

use super::{ControlKind, ControlRequest, ControlWindow, OpenReply, SentControl};
use crate::inventory_ledger::PlayerInventoryLedger;

impl PlayerInventoryLedger {
    /// Transfers a matching actor reply to storage while preserving the latest screen intent.
    pub(in crate::inventory_ledger) fn answer_actor_open(
        &mut self,
        open: ContainerOpenEvent,
    ) -> Option<bool> {
        let OpenReply { generation, target } = self.controls.answer_open(open)?;
        let desired_open = generation.is_some()
            && self.controls.personal.desired_open
            && self.controls.personal_target == Some(target);
        if self.controls.personal.confirmed.is_some_and(|window| {
            Some(window.generation) == generation
                || (self.controls.personal.reset_required && window.target == Some(target))
        }) {
            self.controls.personal.confirmed = None;
            self.controls.personal.reset_required = false;
        }
        if desired_open {
            self.controls.personal.desired_open = false;
        }
        if self
            .personal
            .is_some_and(|personal| Some(personal.generation()) == generation)
        {
            self.discard_pending_personal_open();
        }
        Some(desired_open)
    }

    /// Keeps personal replies bound to a compatible admitted target until its window closes.
    pub(in crate::inventory_ledger) fn apply_personal_open(&mut self, open: ContainerOpenEvent) {
        let Some(window_id) = open.container.window_id else {
            self.note_unrouted_container();
            return;
        };
        if !super::super::helpers::valid_raw_window_id(window_id) {
            self.controls.personal.desired_open = false;
            self.personal = None;
            self.note_unrouted_container();
            return;
        }
        let request = self.controls.answer_open(open);
        let window =
            if let Some(OpenReply {
                generation: Some(generation),
                target,
            }) = request
            {
                ControlWindow {
                    generation,
                    window_id,
                    window_type: open.window_type,
                    target: Some(target),
                }
            } else if let Some(window) = self.controls.personal.confirmed.filter(|window| {
                request.is_none_or(|request| window.target == Some(request.target))
            }) {
                ControlWindow {
                    window_id,
                    ..window
                }
            } else {
                self.note_unrouted_container();
                self.reject_window(window_id, open.window_type);
                return;
            };
        self.controls.confirm(window);
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
        if state.rejected == Some(window) {
            state.rejected = None;
        }
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
