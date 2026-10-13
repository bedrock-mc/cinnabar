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
    /// The personal generation this storage close may cover, even before its ID arrives.
    pub covered_personal_generation: Option<u64>,
    deadline_millis: u64,
}

impl SentControl {
    /// Matches the captured personal generation once its reply supplies the wire ID.
    pub fn covers_personal(self, personal: ControlWindow) -> bool {
        self.covered_personal_generation == Some(personal.generation)
            && matches!(self.request, ControlRequest::Close(storage)
                if storage.kind() == ControlKind::Storage
                    && storage.window_id == personal.window_id)
    }
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
        let covered_personal_generation = match request {
            ControlRequest::Close(window) => {
                assert!(!self.close_outstanding(window.window_id));
                if window.kind() == ControlKind::Storage {
                    self.personal
                        .confirmed
                        .filter(|personal| personal.window_id == window.window_id)
                        .map(|personal| personal.generation)
                        .or_else(|| match self.personal.sent?.request {
                            ControlRequest::Open { generation, .. } => Some(generation),
                            ControlRequest::Close(_) => None,
                        })
                } else {
                    None
                }
            }
            ControlRequest::Open { .. } => None,
        };
        self.state_mut(request.kind()).sent = Some(SentControl {
            request,
            covered_personal_generation,
            deadline_millis: now_millis.saturating_add(INVENTORY_REQUEST_TIMEOUT_MILLIS),
        });
    }

    /// Keeps the captured personal generation unusable until its covering storage close resolves.
    pub fn personal_covered_by_storage_close(&self) -> bool {
        self.personal.confirmed.is_some_and(|personal| {
            self.storage
                .sent
                .is_some_and(|sent| sent.covers_personal(personal))
        })
    }

    /// Prevents two kinds from sending closes for the same wire ID concurrently.
    pub fn close_outstanding(&self, window_id: i32) -> bool {
        [&self.personal, &self.storage].into_iter().any(|state| {
            state.sent.is_some_and(|sent| {
                matches!(sent.request, ControlRequest::Close(window) if window.window_id == window_id)
            })
        })
    }

    /// Resolves the admitted generation, accepting an untyped acknowledgement only for its sole ID slot.
    pub fn answer_close(&mut self, close: protocol::ContainerCloseEvent) -> Option<SentControl> {
        let kind = [ControlKind::Personal, ControlKind::Storage]
            .into_iter()
            .find(|kind| {
                self.state(*kind).sent.is_some_and(|sent| {
                    matches!(sent.request, ControlRequest::Close(window)
                        if close.container.window_id == Some(window.window_id)
                            && (close.window_type == window.window_type
                                || close.window_type == protocol::NO_CONTAINER_WINDOW_TYPE))
                })
            })?;
        self.state_mut(kind).sent.take()
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
    pub fn expire(&mut self, kind: ControlKind, now_millis: u64) -> Option<SentControl> {
        let state = self.state_mut(kind);
        if state
            .sent
            .is_some_and(|sent| now_millis >= sent.deadline_millis)
        {
            state.sent.take()
        } else {
            None
        }
    }
}
