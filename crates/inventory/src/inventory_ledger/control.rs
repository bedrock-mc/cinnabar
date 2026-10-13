//! Sent window controls retain their reply order independently of screen ownership.

use std::collections::VecDeque;

use super::{
    INVENTORY_REQUEST_TIMEOUT_MILLIS, PendingClose, PendingCloseOwner, PersonalWindow,
    PlayerInventoryLedger,
};

mod replies;
#[cfg(test)]
mod tests;

/// Stop admitting controls rather than evicting an unanswered correlation record.
const MAX_SENT_CONTROLS: usize = 128;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) enum ControlKind {
    PersonalOpen,
    Close,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) enum ControlRequest {
    PersonalOpen(u64),
    Close(PendingClose),
}

impl ControlRequest {
    /// Replies distinguish personal opens from closes, but not the owner of a close.
    pub(super) const fn kind(self) -> ControlKind {
        match self {
            Self::PersonalOpen(_) => ControlKind::PersonalOpen,
            Self::Close(_) => ControlKind::Close,
        }
    }

    /// Identifies the window that sent this control; unsolicited cleanup owns no screen.
    const fn generation(self) -> Option<u64> {
        match self {
            Self::PersonalOpen(generation) => Some(generation),
            Self::Close(close) => match close.owner {
                PendingCloseOwner::Personal(generation)
                | PendingCloseOwner::Storage(generation) => Some(generation),
                PendingCloseOwner::Cleanup => None,
            },
        }
    }
}

#[derive(Debug, Clone, Copy)]
pub(super) struct SentControl {
    pub request: ControlRequest,
    pub superseded: bool,
    /// None means this request timed out once; its late reply still occupies this FIFO slot.
    deadline_millis: Option<u64>,
}

#[derive(Debug, Clone, Default)]
pub(super) struct SentControls {
    entries: VecDeque<SentControl>,
}

impl SentControls {
    /// Keeps the FIFO bounded without forgetting any request already sent.
    pub fn has_capacity(&self) -> bool {
        self.entries.len() < MAX_SENT_CONTROLS
    }

    /// Records one successful transport admission, including its window generation.
    pub fn sent(&mut self, request: ControlRequest, now_millis: u64) {
        assert!(
            self.has_capacity(),
            "control admission requires FIFO capacity"
        );
        self.entries.push_back(SentControl {
            request,
            superseded: false,
            deadline_millis: Some(now_millis.saturating_add(INVENTORY_REQUEST_TIMEOUT_MILLIS)),
        });
    }

    /// Removes only the oldest request of this reply kind, including a timed-out tombstone.
    pub fn answer(&mut self, kind: ControlKind) -> Option<SentControl> {
        let index = self
            .entries
            .iter()
            .position(|sent| sent.request.kind() == kind)?;
        self.entries.remove(index)
    }

    /// Inspects the oldest close before treating an authoritative server close as its answer.
    pub fn first_close(&self) -> Option<PendingClose> {
        self.entries.iter().find_map(|sent| match sent.request {
            ControlRequest::Close(close) => Some(close),
            ControlRequest::PersonalOpen(_) => None,
        })
    }

    /// Releases a window's ownership without changing the order of its unanswered controls.
    pub fn supersede(&mut self, generation: u64) {
        for sent in &mut self.entries {
            if sent.request.generation() == Some(generation) {
                sent.superseded = true;
            }
        }
    }

    /// Retires all screen ownership when authority changes on the same transport.
    pub fn supersede_all(&mut self) {
        for sent in &mut self.entries {
            sent.superseded = true;
        }
    }

    /// Finds the active screen's deadline in the same FIFO that owns its reply correlation.
    pub fn generation_timeout_due(&self, generation: u64, now_millis: u64) -> bool {
        self.entries.iter().any(|sent| {
            sent.request.generation() == Some(generation)
                && sent
                    .deadline_millis
                    .is_some_and(|deadline| now_millis >= deadline)
        })
    }

    /// Reports whether polling can time out a sent request for the first time.
    pub fn timeout_due(&self, now_millis: u64) -> bool {
        self.entries.iter().any(|sent| {
            sent.deadline_millis
                .is_some_and(|deadline| now_millis >= deadline)
        })
    }

    /// Completes each overdue wait once, retaining its slot to drain a possible late reply.
    pub fn expire(&mut self, now_millis: u64) -> usize {
        let mut expired = 0;
        for sent in &mut self.entries {
            if sent
                .deadline_millis
                .is_some_and(|deadline| now_millis >= deadline)
            {
                sent.deadline_millis = None;
                expired += 1;
            }
        }
        expired
    }
}

impl PlayerInventoryLedger {
    /// Records a ready control only after transport accepts it, before releasing its UI owner.
    pub(super) fn mark_control_enqueued(&mut self, now_millis: u64) -> bool {
        if !self.sent_controls.has_capacity() {
            return false;
        }
        if self.close_ready()
            && let Some(close) = self.pending_closes.pop_front()
        {
            self.sent_controls
                .sent(ControlRequest::Close(close), now_millis);
            if let Some(generation) = close.owner.personal_generation()
                && let Some(PersonalWindow::Closing {
                    generation: current,
                    admitted,
                    ..
                }) = self.personal_lifecycle_mut()
                && *current == generation
            {
                *admitted = true;
            }
            self.resume_held_open();
            return true;
        }
        if let Some(PersonalWindow::Opening {
            generation,
            admitted,
            ..
        }) = self.personal.as_mut()
            && !*admitted
        {
            *admitted = true;
            self.sent_controls
                .sent(ControlRequest::PersonalOpen(*generation), now_millis);
            return true;
        }
        false
    }

    /// Cleans up an old personal reply only when no newer screen could own its wire ID.
    pub(super) fn queue_late_personal_cleanup(&mut self, window_id: i32, generation: Option<u64>) {
        let personal_owns_id = self.personal.is_some_and(|personal| match personal {
            // Its server-assigned ID is still unknown, so a cleanup could close the newer window.
            PersonalWindow::Opening { admitted, .. } => admitted,
            PersonalWindow::Open {
                window_id: active, ..
            }
            | PersonalWindow::Closing {
                window_id: active, ..
            } => active == window_id,
        });
        if personal_owns_id
            || self
                .storage
                .as_ref()
                .is_some_and(|storage| storage.window_id == window_id)
        {
            return;
        }
        self.queue_close(
            window_id,
            super::PERSONAL_INVENTORY_WINDOW_TYPE,
            generation.map_or(PendingCloseOwner::Cleanup, PendingCloseOwner::Personal),
        );
    }
}
