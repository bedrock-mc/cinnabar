//! A borrowed callback with typed sends and the canonical world operations.

use super::__private::RawCallback;
use super::*;

/// World access for one callback. Raw transport imports live only in `bindings`.
pub struct Callback<'a>(pub(super) &'a RawCallback);

impl<'a> Callback<'a> {
    /// Borrows the host resource for an ABI adapter; it cannot outlive that invocation.
    #[doc(hidden)]
    pub fn from_raw(raw: &'a RawCallback) -> Self {
        Self(raw)
    }

    /// Validates a reply and stages it using the message's declared delivery mode.
    pub fn send<M: crate::message::ToClient>(
        &self,
        player: &str,
        message: &M,
    ) -> Result<(), GuestError> {
        let payload = nodes(message.encode().map_err(GuestError::Rejected)?);
        let send = if M::CONTROL {
            RawCallback::send_control
        } else {
            RawCallback::send_client
        };
        send(self.0, player, M::CHANNEL.id, M::CHANNEL.schema, &payload)
            .map_err(|error| GuestError::Failed(format!("send {}: {error:?}", M::CHANNEL.id)))
    }
}

include!(concat!(env!("OUT_DIR"), "/callback.rs"));
