//! Close acknowledgements consume one sent control, never a newer screen with the same ID.

use protocol::ContainerCloseEvent;

use super::{
    ControlKind, ControlRequest, PendingCloseOwner, PersonalWindow, PlayerInventoryLedger,
};

impl PlayerInventoryLedger {
    /// Routes client acknowledgements through the FIFO and server commands by their window identity.
    pub(in crate::inventory_ledger) fn apply_close(&mut self, close: ContainerCloseEvent) {
        if !close.server_initiated {
            self.answer_control_close();
            return;
        }

        let storage = self
            .storage
            .as_ref()
            .filter(|storage| {
                close.container.window_id == Some(storage.window_id)
                    && close.window_type == storage.window_type
            })
            .map(|storage| (storage.window_id, storage.generation));
        // A server command ends screen ownership, but the separately sent close still needs a reply.
        let personal = self
            .personal_lifecycle()
            .copied()
            .filter(|personal| match personal {
                PersonalWindow::Open {
                    window_id,
                    window_type,
                    ..
                }
                | PersonalWindow::Closing {
                    window_id,
                    window_type,
                    ..
                } => {
                    close.container.window_id == Some(*window_id)
                        && close.window_type == *window_type
                }
                PersonalWindow::Opening { .. } => false,
            });
        if let Some(personal) = personal {
            let generation = personal.generation();
            self.pending_closes
                .retain(|pending| pending.owner.personal_generation() != Some(generation));
            self.sent_controls.supersede(generation);
            if self
                .expired_personal
                .is_some_and(|expired| expired.generation() == generation)
            {
                self.expired_personal = None;
            } else {
                self.finish_personal_close(false);
            }
        }
        if let Some((window_id, generation)) = storage {
            self.sent_controls.supersede(generation);
            self.remove_pending_close(window_id, close.window_type);
            if self
                .storage
                .as_ref()
                .is_some_and(|storage| storage.generation == generation)
            {
                self.acknowledge_storage_close(true);
            }
            self.queue_close(window_id, close.window_type, PendingCloseOwner::Cleanup);
        }
    }

    /// Consumes one close reply using its retained owner, ignoring uncorrelated payload IDs and types.
    fn answer_control_close(&mut self) {
        let Some(sent) = self.sent_controls.answer(ControlKind::Close) else {
            return;
        };
        let ControlRequest::Close(close) = sent.request else {
            unreachable!("the close FIFO returns only closes");
        };
        match close.owner {
            PendingCloseOwner::Personal(generation) => {
                if self
                    .expired_personal
                    .is_some_and(|personal| personal.generation() == generation)
                {
                    self.expired_personal = None;
                } else if !sent.superseded
                    && matches!(self.personal, Some(PersonalWindow::Closing { generation: active, .. }) if active == generation)
                {
                    self.finish_personal_close(true);
                }
            }
            PendingCloseOwner::Storage(generation) => {
                if !sent.superseded
                    && self
                        .storage
                        .as_ref()
                        .is_some_and(|storage| storage.generation == generation && storage.closing)
                {
                    self.acknowledge_storage_close(false);
                }
            }
            PendingCloseOwner::Cleanup => {}
        }
    }
}
