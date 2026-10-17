//! One admitted control per window kind, with unanswered opens retained in send order.

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
    pub target: Option<OpenTarget>,
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
pub(super) struct OpenTarget {
    pub runtime_id: u64,
    pub actor_unique_id: Option<i64>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) struct OpenRequest {
    pub generation: u64,
    pub target: OpenTarget,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) enum ControlRequest {
    Open(OpenRequest),
    Close(ControlWindow),
}

impl ControlRequest {
    /// Identifies the open or close generation owned by this admitted control.
    pub fn generation(self) -> u64 {
        match self {
            Self::Open(request) => request.generation,
            Self::Close(window) => window.generation,
        }
    }

    /// Returns the window kind whose admission slot this request occupies.
    pub fn kind(self) -> ControlKind {
        match self {
            Self::Open(_) => ControlKind::Personal,
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
    /// A rejected reply needs a close without becoming the active window.
    pub rejected: Option<ControlWindow>,
    /// An unanswered open needs a close before it is safe to try opening again.
    pub reset_required: bool,
}

#[derive(Debug, Clone, Default)]
pub(super) struct WindowControls {
    pub personal: WindowControl,
    pub storage: WindowControl,
    pub personal_target: Option<OpenTarget>,
    /// Timeouts release admission slots, but replies still consume their original requests.
    pub unanswered_opens: std::collections::VecDeque<OpenRequest>,
}

impl WindowControls {
    /// Returns the admitted target while its open or confirmed window still owns the slot.
    pub fn admitted_personal_target(&self) -> Option<OpenTarget> {
        if let Some(SentControl {
            request: ControlRequest::Open(OpenRequest { target, .. }),
            ..
        }) = self.personal.sent
        {
            Some(target)
        } else {
            self.personal.confirmed.and_then(|window| window.target)
        }
    }

    /// Matches the oldest unanswered request for the identity carried by this reply.
    pub fn answer_open(&mut self, open: protocol::ContainerOpenEvent) -> Option<OpenRequest> {
        let index = self.unanswered_opens.iter().position(|request| {
            if open.window_type == PERSONAL_INVENTORY_WINDOW_TYPE {
                request.target.actor_unique_id.is_none()
            } else {
                request.target.actor_unique_id == Some(open.runtime_entity_id)
            }
        })?;
        self.unanswered_opens.remove(index)
    }

    /// Confirms a window and drops unsent rejection closes that would hit its reused ID.
    pub fn confirm(&mut self, window: ControlWindow) {
        for state in [&mut self.personal, &mut self.storage] {
            if state
                .rejected
                .is_some_and(|rejected| rejected.window_id == window.window_id)
            {
                state.rejected = None;
            }
        }
        self.state_mut(window.kind()).confirmed = Some(window);
    }

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
                if window.kind() == ControlKind::Storage && self.storage.rejected != Some(window) {
                    self.personal
                        .confirmed
                        .filter(|personal| personal.window_id == window.window_id)
                        .map(|personal| personal.generation)
                        .or_else(|| match self.personal.sent?.request {
                            ControlRequest::Open(request) => Some(request.generation),
                            ControlRequest::Close(_) => None,
                        })
                } else {
                    None
                }
            }
            ControlRequest::Open(request) => {
                self.unanswered_opens.push_back(request);
                None
            }
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
