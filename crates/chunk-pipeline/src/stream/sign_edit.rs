//! Presentation access to authoritative sign-edit requests.

use client_world::SignEditRequest;
use client_world::ingestion::OpenSignEvent;

use super::WorldStream;

impl WorldStream {
    /// Retains a sign request at its ordered commit position.
    pub(super) fn consume_open_sign(&mut self, event: OpenSignEvent) {
        self.authority.consume_open_sign(event);
    }

    /// Takes the pending sign-edit request, if any.
    pub fn take_pending_sign_edit(&mut self) -> Option<SignEditRequest> {
        self.authority.take_pending_sign_edit()
    }
}
