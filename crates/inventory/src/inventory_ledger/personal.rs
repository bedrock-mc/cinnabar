use super::{InventoryPendingState, PlayerInventoryLedger};

#[derive(Debug, Clone, Copy)]
pub(super) enum PersonalWindow {
    Opening {
        generation: u64,
        admitted: bool,
    },
    Open {
        generation: u64,
    },
    Closing {
        generation: u64,
        /// The initial open has not received a server window yet.
        awaiting_open: bool,
    },
}

impl PersonalWindow {
    /// Identifies the screen generation that owns its predicted item gestures.
    pub(super) const fn generation(&self) -> u64 {
        match self {
            Self::Opening { generation, .. }
            | Self::Open { generation, .. }
            | Self::Closing { generation, .. } => *generation,
        }
    }
}

impl PlayerInventoryLedger {
    #[must_use]
    pub const fn personal_inventory_desired_open(&self) -> bool {
        self.controls.personal.desired_open
    }

    /// Whether the current storage-screen intent still wants its interface visible.
    #[must_use]
    pub const fn storage_inventory_desired_open(&self) -> bool {
        self.controls.storage.desired_open
    }

    /// Admits personal gestures only while the latest intent still wants this screen open.
    pub(super) fn personal_generation_for_gesture(&self) -> Option<u64> {
        if !self.controls.personal.desired_open {
            return None;
        }
        match self.personal {
            Some(
                PersonalWindow::Opening {
                    generation,
                    admitted: true,
                }
                | PersonalWindow::Open { generation, .. },
            ) => Some(generation),
            _ => None,
        }
    }

    /// Keeps a closing screen's authority until its queued item returns have settled.
    pub(super) fn personal_close_pending(&self) -> bool {
        matches!(self.personal, Some(PersonalWindow::Closing { generation, .. })
            if self.personal_generation_pending(generation))
    }

    /// Whether retained item requests still belong to this personal window generation.
    pub(super) fn personal_generation_pending(&self, generation: u64) -> bool {
        self.pending_requests()
            .any(|request| request.personal_generation == Some(generation))
    }

    /// Queues inventory input for the runtime actor selected when the user pressed it.
    pub fn request_personal_open(&mut self, target_runtime_id: u64) -> bool {
        if self.authority.is_none() || target_runtime_id == 0 || self.storage.is_some() {
            return false;
        }
        self.controls.personal_target = target_runtime_id;
        self.controls.personal.desired_open = true;
        if let Some(PersonalWindow::Closing {
            generation,
            awaiting_open: true,
        }) = self.personal
            && self.controls.personal.sent.is_some()
        {
            self.personal = Some(PersonalWindow::Opening {
                generation,
                admitted: true,
            });
        }
        self.reconcile_personal();
        true
    }

    pub fn request_personal_close(&mut self) {
        if !self.controls.personal.desired_open {
            return;
        }
        let returning = if self.personal_generation_for_gesture().is_some() {
            self.return_crafting_on_close().unwrap_or_else(|error| {
                self.note_close_return_failure(error);
                true
            })
        } else {
            self.personal
                .is_some_and(|personal| self.personal_generation_pending(personal.generation()))
        };
        self.controls.personal.desired_open = false;
        if let Some(personal) = self.personal {
            let generation = personal.generation();
            if !returning {
                self.abandon_requests(|pending| {
                    pending.personal_generation == Some(generation)
                        && pending.state == InventoryPendingState::AwaitingTransport
                });
            }
            if let PersonalWindow::Opening { admitted, .. } = personal {
                self.personal = (admitted || returning).then_some(PersonalWindow::Closing {
                    generation,
                    awaiting_open: true,
                });
            }
        }
        self.reconcile_personal();
    }

    /// Drops an opening screen's gestures without changing its retained intent or wire request.
    pub(super) fn discard_pending_personal_open(&mut self) {
        let Some(PersonalWindow::Opening { generation, .. }) = self.personal else {
            return;
        };
        self.personal = None;
        self.abandon_requests(|pending| pending.personal_generation == Some(generation));
    }
}
