//! One outstanding control and one desired state per window kind.

use super::{INVENTORY_REQUEST_TIMEOUT_MILLIS, PERSONAL_INVENTORY_WINDOW_TYPE};

mod reconcile;
mod replies;
#[cfg(test)]
mod tests;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) enum ControlKind {
    Personal,
    Storage,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) struct ControlWindow {
    pub generation: u64,
    pub window_id: i32,
    pub window_type: i8,
}

impl ControlWindow {
    /// Personal and storage windows have independent outstanding requests.
    pub fn kind(self) -> ControlKind {
        if self.window_type == PERSONAL_INVENTORY_WINDOW_TYPE {
            ControlKind::Personal
        } else {
            ControlKind::Storage
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) enum ControlRequest {
    Open {
        generation: u64,
        target_runtime_id: u64,
    },
    Close(ControlWindow),
}

impl ControlRequest {
    /// Returns the window kind whose admission slot this request occupies.
    pub fn kind(self) -> ControlKind {
        match self {
            Self::Open { .. } => ControlKind::Personal,
            Self::Close(window) => window.kind(),
        }
    }
}

#[derive(Debug, Clone, Copy)]
pub(super) struct SentControl {
    pub request: ControlRequest,
    sequence: u64,
    deadline_millis: u64,
}

#[derive(Debug, Clone, Default)]
pub(super) struct WindowControl {
    pub desired_open: bool,
    pub confirmed: Option<ControlWindow>,
    pub sent: Option<SentControl>,
    /// An unanswered open needs a close before it is safe to try opening again.
    pub reset_required: bool,
}

#[derive(Debug, Clone, Default)]
pub(super) struct WindowControls {
    pub personal: WindowControl,
    pub storage: WindowControl,
    pub personal_target: u64,
    sequence: u64,
}

impl WindowControls {
    /// Borrows the state that owns this kind's desired state and outstanding request.
    pub fn state(&self, kind: ControlKind) -> &WindowControl {
        match kind {
            ControlKind::Personal => &self.personal,
            ControlKind::Storage => &self.storage,
        }
    }

    /// Mutably borrows one admission slot without changing the other kind.
    pub fn state_mut(&mut self, kind: ControlKind) -> &mut WindowControl {
        match kind {
            ControlKind::Personal => &mut self.personal,
            ControlKind::Storage => &mut self.storage,
        }
    }

    /// Records transport admission only when this kind has no outstanding request.
    pub fn sent(&mut self, request: ControlRequest, now_millis: u64) {
        assert!(self.state(request.kind()).sent.is_none());
        self.sequence = self.sequence.wrapping_add(1);
        self.state_mut(request.kind()).sent = Some(SentControl {
            request,
            sequence: self.sequence,
            deadline_millis: now_millis.saturating_add(INVENTORY_REQUEST_TIMEOUT_MILLIS),
        });
    }

    /// Matches the wire ID, using send order only when both kinds share that ID.
    pub fn answer_close(&mut self, window_id: Option<i32>) -> Option<ControlWindow> {
        let kind = [ControlKind::Personal, ControlKind::Storage]
            .into_iter()
            .filter_map(|kind| {
                let sent = self.state(kind).sent?;
                matches!(sent.request, ControlRequest::Close(window) if Some(window.window_id) == window_id)
                    .then_some((kind, sent.sequence))
            })
            .min_by_key(|(_, sequence)| *sequence)?
            .0;
        let sent = self.state_mut(kind).sent.take()?;
        let ControlRequest::Close(window) = sent.request else {
            unreachable!()
        };
        Some(window)
    }

    /// Reports whether an outstanding control can resolve through its deadline.
    pub fn timeout_due(&self, now_millis: u64) -> bool {
        [&self.personal, &self.storage].into_iter().any(|state| {
            state
                .sent
                .is_some_and(|sent| now_millis >= sent.deadline_millis)
        })
    }

    /// Releases this kind's slot exactly once when its reply deadline expires.
    pub fn expire(&mut self, kind: ControlKind, now_millis: u64) -> Option<ControlRequest> {
        let state = self.state_mut(kind);
        if state
            .sent
            .is_some_and(|sent| now_millis >= sent.deadline_millis)
        {
            state.sent.take().map(|sent| sent.request)
        } else {
            None
        }
    }
}
