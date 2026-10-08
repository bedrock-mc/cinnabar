//! Closed windows whose admitted requests still await answers.
//!
//! The close leaves before its returns are answered, so an acknowledgement can
//! overtake them. Each closed generation stays correlated until its last request
//! settles, then runs the cleanup its close deferred.

use protocol::ContainerIdentity;

use super::queue::PendingRequest;
use super::{InventoryPendingState, PlayerInventoryLedger};

/// Bounds closed windows kept for late answers; the oldest is recovered first.
pub(super) const MAX_SETTLING_CLOSES: usize = 8;

#[derive(Debug, Clone, Copy, Eq, PartialEq)]
pub(super) enum SettlingWindow {
    Personal(u64),
    Storage {
        generation: u64,
        identity: Option<ContainerIdentity>,
    },
}

impl SettlingWindow {
    fn owns(self, request: &PendingRequest) -> bool {
        match self {
            Self::Personal(generation) => request.personal_generation == Some(generation),
            Self::Storage { generation, .. } => request.storage_generation == Some(generation),
        }
    }
}

impl PlayerInventoryLedger {
    /// Whether `window` has a request the server admitted and may still answer in time.
    pub(super) fn has_unanswered(&self, window: SettlingWindow) -> bool {
        self.queue.iter().any(|request| {
            window.owns(request)
                && request.state == InventoryPendingState::AwaitingResponse
                && !request.timed_out
        })
    }

    /// Keeps a closed window's admitted requests correlated until they settle.
    pub(super) fn retain_settling(&mut self, window: SettlingWindow) {
        self.abandon_requests(|request| {
            window.owns(request) && request.state == InventoryPendingState::AwaitingTransport
        });
        if self.settling.len() >= MAX_SETTLING_CLOSES
            && let Some(oldest) = self.settling.pop_front()
        {
            self.abandon_requests(|request| oldest.owns(request));
            self.clear_closed_inputs(oldest);
        }
        self.settling.push_back(window);
    }

    pub(super) fn personal_settling(&self, generation: u64) -> bool {
        self.settling
            .contains(&SettlingWindow::Personal(generation))
    }

    /// The identity a closed storage generation answered for, if it is still settling.
    pub(super) fn storage_settling(&self, generation: u64) -> Option<Option<ContainerIdentity>> {
        self.settling.iter().find_map(|window| match window {
            SettlingWindow::Storage {
                generation: settling,
                identity,
            } if *settling == generation => Some(*identity),
            _ => None,
        })
    }

    pub(super) fn settling_storage_generations(&self) -> Vec<u64> {
        self.settling
            .iter()
            .filter_map(|window| match window {
                SettlingWindow::Storage { generation, .. } => Some(*generation),
                SettlingWindow::Personal(_) => None,
            })
            .collect()
    }

    /// Runs deferred cleanup for every closed window whose last request has settled.
    pub(super) fn finish_settled_closes(&mut self) {
        while let Some(index) = self
            .settling
            .iter()
            .position(|window| !self.queue.iter().any(|request| window.owns(request)))
        {
            let window = self.settling.remove(index).expect("index observed");
            self.clear_closed_inputs(window);
        }
    }

    /// A reopened window owns the crafting inputs and cursor by then.
    fn clear_closed_inputs(&mut self, window: SettlingWindow) {
        if self.personal.is_some() || self.storage.is_some() {
            return;
        }
        match window {
            SettlingWindow::Personal(_) => {
                let retain_confirmed_cursor = !self.cursor_resync_required;
                self.clear_window_inputs(retain_confirmed_cursor);
            }
            SettlingWindow::Storage { .. } => self.clear_storage_inputs(),
        }
    }
}
