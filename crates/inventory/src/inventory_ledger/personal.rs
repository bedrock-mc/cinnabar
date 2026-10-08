use super::{InventoryPendingState, PendingCloseOwner, PlayerInventoryLedger};

#[derive(Debug, Clone, Copy)]
pub(super) enum PersonalWindow {
    Opening {
        generation: u64,
        target_runtime_id: u64,
        admitted: bool,
        desired_open: bool,
        deadline_millis: Option<u64>,
    },
    Open {
        generation: u64,
        window_id: i32,
        window_type: i8,
    },
    Closing {
        generation: u64,
        window_id: i32,
        window_type: i8,
        deadline_millis: Option<u64>,
    },
}

impl PersonalWindow {
    pub(super) const fn generation(&self) -> u64 {
        match self {
            Self::Opening { generation, .. }
            | Self::Open { generation, .. }
            | Self::Closing { generation, .. } => *generation,
        }
    }

    const fn deadline_millis(&self) -> Option<u64> {
        match self {
            Self::Opening {
                deadline_millis, ..
            }
            | Self::Closing {
                deadline_millis, ..
            } => *deadline_millis,
            Self::Open { .. } => None,
        }
    }
}

impl PlayerInventoryLedger {
    #[must_use]
    pub const fn personal_inventory_desired_open(&self) -> bool {
        self.held_open.is_some()
            || matches!(
                self.personal,
                Some(
                    PersonalWindow::Opening {
                        desired_open: true,
                        ..
                    } | PersonalWindow::Open { .. }
                )
            )
    }

    pub(super) fn personal_generation_for_gesture(&self) -> Option<u64> {
        match self.personal {
            Some(
                PersonalWindow::Opening {
                    generation,
                    admitted: true,
                    desired_open: true,
                    ..
                }
                | PersonalWindow::Open { generation, .. },
            ) => Some(generation),
            _ => None,
        }
    }

    pub fn request_personal_open(&mut self, target_runtime_id: u64) -> bool {
        if self.authority.is_none()
            || target_runtime_id == 0
            || self.storage.is_some()
            || self.personal_lifecycle_failed
        {
            return false;
        }
        if self.personal_inventory_desired_open() {
            return true;
        }
        // The open follows the close on the wire instead of being dropped.
        if !self.pending_closes.is_empty()
            || matches!(self.personal, Some(PersonalWindow::Closing { .. }))
        {
            self.held_open = Some(target_runtime_id);
            return true;
        }
        if self.personal.is_some() {
            return false;
        }
        let generation = self.next_open_generation;
        self.next_open_generation = self.next_open_generation.wrapping_add(1).max(1);
        self.personal = Some(PersonalWindow::Opening {
            generation,
            target_runtime_id,
            admitted: false,
            desired_open: true,
            deadline_millis: None,
        });
        true
    }

    pub fn request_personal_close(&mut self) {
        if self.held_open.take().is_some() {
            return;
        }
        let Some(personal) = self.personal else {
            return;
        };
        if matches!(
            personal,
            PersonalWindow::Closing { .. }
                | PersonalWindow::Opening {
                    desired_open: false,
                    ..
                }
        ) {
            return;
        }
        let returning = if matches!(
            personal,
            PersonalWindow::Open { .. }
                | PersonalWindow::Opening {
                    admitted: true,
                    desired_open: true,
                    ..
                }
        ) {
            // Vanilla always closes; inputs it cannot return wait for the server's restatement.
            self.return_crafting_on_close().unwrap_or_else(|error| {
                self.note_close_return_failure(error);
                true
            })
        } else {
            false
        };
        let generation = match personal {
            PersonalWindow::Opening {
                generation,
                admitted: false,
                ..
            } => {
                self.personal = None;
                self.cancel_unsent_personal_prediction(generation);
                return;
            }
            PersonalWindow::Opening {
                generation,
                target_runtime_id,
                admitted: true,
                ..
            } => {
                self.personal = Some(PersonalWindow::Opening {
                    generation,
                    target_runtime_id,
                    admitted: true,
                    desired_open: false,
                    deadline_millis: personal.deadline_millis(),
                });
                generation
            }
            PersonalWindow::Open {
                generation,
                window_id,
                window_type,
            } => {
                self.queue_close(
                    window_id,
                    window_type,
                    PendingCloseOwner::Personal(generation),
                );
                self.personal = Some(PersonalWindow::Closing {
                    generation,
                    window_id,
                    window_type,
                    deadline_millis: None,
                });
                generation
            }
            PersonalWindow::Closing { generation, .. } => generation,
        };
        if !returning {
            self.cancel_unsent_personal_prediction(generation);
        }
    }

    /// Starts a held open once no close is unsent or awaiting its acknowledgement.
    pub(super) fn resume_held_open(&mut self) {
        if let Some(target_runtime_id) = self.held_open
            && self.pending_closes.is_empty()
            && !matches!(self.personal, Some(PersonalWindow::Closing { .. }))
        {
            self.held_open = None;
            self.request_personal_open(target_runtime_id);
        }
    }

    fn cancel_unsent_personal_prediction(&mut self, generation: u64) {
        self.abandon_requests(|pending| {
            pending.personal_generation == Some(generation)
                && pending.state == InventoryPendingState::AwaitingTransport
        });
    }
}
