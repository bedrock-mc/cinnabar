//! Pending transport controls and the current server-confirmed windows.

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
    deadline_millis: u64,
}

#[derive(Debug, Clone, Default)]
pub(super) struct WindowControl {
    pub desired_open: bool,
    /// The current window; admitting its close transfers ownership to `sent`.
    pub confirmed: Option<ControlWindow>,
    pub sent: Option<SentControl>,
}

impl WindowControl {
    /// Returns the current server window, including a closing one until its answer arrives.
    pub fn server_window(&self) -> Option<ControlWindow> {
        self.confirmed.or_else(|| match self.sent?.request {
            ControlRequest::Close(window) => Some(window),
            ControlRequest::Open { .. } => None,
        })
    }
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

    /// Transfers a closing window to its transport slot, leaving room for a replacement.
    pub fn sent(&mut self, request: ControlRequest, now_millis: u64) {
        let state = self.state_mut(request.kind());
        assert!(state.sent.is_none());
        if let ControlRequest::Close(window) = request {
            assert_eq!(state.confirmed.take(), Some(window));
        }
        state.sent = Some(SentControl {
            request,
            deadline_millis: now_millis.saturating_add(INVENTORY_REQUEST_TIMEOUT_MILLIS),
        });
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
