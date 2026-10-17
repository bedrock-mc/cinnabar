use super::control::OpenTarget;
use super::{InventoryPendingState, PlayerInventoryLedger};

#[derive(Debug, Clone, Copy)]
pub(super) enum PersonalWindow {
    Opening { generation: u64, admitted: bool },
    Open { generation: u64 },
    Closing { generation: u64 },
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

    /// Admits personal gestures only while the latest intent still wants this screen open.
    pub(super) fn personal_generation_for_gesture(&self) -> Option<u64> {
        if !self.controls.personal.desired_open || self.controls.personal_covered_by_storage_close()
        {
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

    pub fn request_personal_open(&mut self, target_runtime_id: u64) -> bool {
        self.request_open(OpenTarget {
            runtime_id: target_runtime_id,
            actor_unique_id: None,
        })
    }

    /// Opens an actor inventory and correlates its reply by persistent actor identity.
    pub fn request_actor_open(&mut self, target_runtime_id: u64, actor_unique_id: i64) -> bool {
        self.request_open(OpenTarget {
            runtime_id: target_runtime_id,
            actor_unique_id: Some(actor_unique_id),
        })
    }

    /// Retains the requested target and latest intent until its transport reply arrives.
    fn request_open(&mut self, target: OpenTarget) -> bool {
        if self.authority.is_none() || target.runtime_id == 0 || self.storage.is_some() {
            return false;
        }
        self.controls.personal_target = Some(target);
        self.controls.personal.desired_open = true;
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
            false
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
            if matches!(
                personal,
                PersonalWindow::Opening {
                    admitted: false,
                    ..
                }
            ) {
                self.personal = None;
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
