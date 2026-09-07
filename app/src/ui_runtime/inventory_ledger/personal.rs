use protocol::InventoryAuthority;

use super::{InventoryPendingState, PlayerInventoryLedger};

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
        matches!(
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
        if self.authority != Some(InventoryAuthority::Server)
            || target_runtime_id == 0
            || self.storage.is_some()
            || self.pending_close.is_some()
            || self.personal_lifecycle_failed
        {
            return false;
        }
        if self.personal_inventory_desired_open() {
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
        let Some(personal) = self.personal else {
            return;
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
                self.queue_close(window_id, window_type, Some(generation));
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
        self.cancel_unsent_personal_prediction(generation);
    }

    fn cancel_unsent_personal_prediction(&mut self, generation: u64) {
        if self.pending.as_ref().is_some_and(|pending| {
            pending.personal_generation == Some(generation)
                && pending.state == InventoryPendingState::AwaitingTransport
        }) {
            self.rollback_pending();
        }
    }
}
