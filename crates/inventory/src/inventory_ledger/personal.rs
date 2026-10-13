use super::{InventoryPendingState, PendingCloseOwner, PlayerInventoryLedger};

#[derive(Debug, Clone, Copy)]
pub(super) enum PersonalWindow {
    Opening {
        generation: u64,
        target_runtime_id: u64,
        admitted: bool,
        desired_open: bool,
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
        /// Only an unsent close may be cancelled when storage reuses its ID.
        admitted: bool,
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
}

impl PlayerInventoryLedger {
    /// Finds the active personal screen or a timed-out screen awaiting cleanup.
    pub(super) fn personal_lifecycle(&self) -> Option<&PersonalWindow> {
        self.personal.as_ref().or(self.expired_personal.as_ref())
    }

    /// Updates control admission without giving an expired lifecycle ownership of cells.
    pub(super) fn personal_lifecycle_mut(&mut self) -> Option<&mut PersonalWindow> {
        self.personal.as_mut().or(self.expired_personal.as_mut())
    }

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
        // Finish the current or timed-out personal cleanup before retrying.
        if !self.pending_closes.is_empty()
            || self.expired_personal.is_some()
            || matches!(
                self.personal,
                Some(
                    PersonalWindow::Closing { .. }
                        | PersonalWindow::Opening {
                            admitted: true,
                            desired_open: false,
                            ..
                        }
                )
            )
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
                    admitted: false,
                });
                generation
            }
            PersonalWindow::Closing { generation, .. } => generation,
        };
        if !returning {
            self.cancel_unsent_personal_prediction(generation);
        }
    }

    /// Starts a held open once the previous lifecycle and its queued closes have finished.
    pub(super) fn resume_held_open(&mut self) {
        if let Some(target_runtime_id) = self.held_open
            && self.pending_closes.is_empty()
            && self.personal.is_none()
            && self.expired_personal.is_none()
            && self.storage.is_none()
        {
            self.held_open = None;
            self.request_personal_open(target_runtime_id);
        }
    }

    /// Tests the personal window's existing admitted deadline without mutating it.
    pub(super) fn personal_timeout_due(&self, now_millis: u64) -> bool {
        self.personal.is_some_and(|personal| {
            self.sent_controls
                .generation_timeout_due(personal.generation(), now_millis)
        })
    }

    /// Expires the screen and its requests while retaining the unanswered lifecycle for cleanup.
    pub(super) fn poll_personal_timeout(&mut self, now_millis: u64) -> bool {
        if !self.personal_timeout_due(now_millis) {
            return false;
        }
        let mut personal = self
            .personal
            .take()
            .expect("an expired personal window was observed");
        let generation = personal.generation();
        if let PersonalWindow::Opening { desired_open, .. } = &mut personal {
            *desired_open = false;
        }
        self.expired_personal = Some(personal);
        self.pending_closes
            .retain(|close| close.owner.personal_generation() != Some(generation));
        self.abandon_requests(|pending| pending.personal_generation == Some(generation));
        self.held_open = None;
        self.clear_window_inputs(false);
        true
    }

    /// Releases an unacknowledged personal screen while retaining any sent reply correlation.
    pub(super) fn supersede_pending_personal_open(&mut self) {
        let Some(PersonalWindow::Opening { generation, .. }) = self.personal else {
            return;
        };
        self.sent_controls.supersede(generation);
        self.personal = None;
        self.abandon_requests(|pending| pending.personal_generation == Some(generation));
    }

    /// Cancels unsent cleanup when storage reuses its ID, because close packets carry only an ID.
    pub(super) fn supersede_expired_personal_close(&mut self, storage_window_id: i32) {
        if let Some(PersonalWindow::Closing {
            window_id,
            window_type,
            admitted: false,
            ..
        }) = self.expired_personal
            && window_id == storage_window_id
        {
            self.remove_pending_close(window_id, window_type);
            self.expired_personal = None;
        }
    }

    fn cancel_unsent_personal_prediction(&mut self, generation: u64) {
        self.abandon_requests(|pending| {
            pending.personal_generation == Some(generation)
                && pending.state == InventoryPendingState::AwaitingTransport
        });
    }
}
