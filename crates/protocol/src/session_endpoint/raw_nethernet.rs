//! Explicit service signaling syntax; parsing supplies no account or joined-world authority.

use super::{NativeConnectionTarget, NativeTransport};
use bedrock_net::messaging::Recipient;
use bridge::contract::{
    NETHERNET_SIGNALING_JSONRPC, NETHERNET_SIGNALING_WEBSOCKET, NETHERNET_TARGET_PREFIX,
};
use std::io;
use uuid::Uuid;

/// One explicitly tagged service destination, retaining its actual signaling dialect.
/// The selected account must still acquire and verify its own credential and protected socket.
pub struct NativeRawTarget {
    recipient: Recipient,
}

impl NativeRawTarget {
    /// Recognizes namespaces and untagged service IDs before ordinary DNS/port normalization.
    /// A recognized value may be malformed; only `parse` admits an explicit tagged route.
    pub fn recognizes(address: &str) -> bool {
        suffix(address).is_some() || raw_id(address.trim())
    }

    /// Parses only the generated prefix and an explicit jsonrpc/websocket selector.
    /// JSON-RPC requires a nonnil UUID; legacy preserves the exact decimal/UUID spelling.
    ///
    /// # Errors
    /// Rejects malformed or untagged service IDs without guessing signaling or using DNS.
    pub fn parse(address: &str) -> io::Result<Option<Self>> {
        let Some(suffix) = suffix(address) else {
            return if raw_id(address.trim()) {
                Err(invalid())
            } else {
                Ok(None)
            };
        };
        let (signaling, id) = suffix.split_once('/').ok_or_else(invalid)?;
        let recipient = if signaling.eq_ignore_ascii_case(NETHERNET_SIGNALING_JSONRPC) {
            Recipient::Messaging(
                Uuid::parse_str(id)
                    .ok()
                    .filter(|peer| !peer.is_nil())
                    .ok_or_else(invalid)?,
            )
        } else if signaling.eq_ignore_ascii_case(NETHERNET_SIGNALING_WEBSOCKET) && raw_id(id) {
            Recipient::Legacy(id.to_owned())
        } else {
            return Err(invalid());
        };
        Ok(Some(Self { recipient }))
    }

    /// Borrows only the explicitly parsed signaling route, never a cached social endpoint.
    pub fn recipient(&self) -> &Recipient {
        &self.recipient
    }

    /// Projects the actual endpoint after its selected account has prepared the same route.
    pub fn connection_target(&self) -> NativeConnectionTarget {
        match &self.recipient {
            Recipient::Messaging(peer) => {
                NativeConnectionTarget::new(peer.to_string(), NativeTransport::NetherNetMessaging)
            }
            Recipient::Legacy(peer) => {
                NativeConnectionTarget::new(peer.clone(), NativeTransport::NetherNetLegacy)
            }
        }
    }

    /// Checks the exact prepared socket dialect and recipient without transferring account authority.
    pub(crate) fn matches_recipient(&self, recipient: &Recipient) -> bool {
        match (&self.recipient, recipient) {
            (Recipient::Messaging(expected), Recipient::Messaging(actual)) => expected == actual,
            (Recipient::Legacy(expected), Recipient::Legacy(actual)) => expected == actual,
            _ => false,
        }
    }
}

/// Borrows the case-insensitive generated namespace after the Go owner's outer trim.
fn suffix(address: &str) -> Option<&str> {
    let address = address.trim();
    address
        .get(..NETHERNET_TARGET_PREFIX.len())
        .filter(|prefix| prefix.eq_ignore_ascii_case(NETHERNET_TARGET_PREFIX))?;
    address.get(NETHERNET_TARGET_PREFIX.len()..)
}

/// Go raw targets accept uint64 decimal spelling or UUID; friend-world nonce policy is separate.
fn raw_id(id: &str) -> bool {
    (!id.is_empty() && id.bytes().all(|byte| byte.is_ascii_digit()) && id.parse::<u64>().is_ok())
        || Uuid::parse_str(id).is_ok()
}

/// Omits the user-supplied target from controlled failure copy.
fn invalid() -> io::Error {
    io::Error::new(
        io::ErrorKind::InvalidInput,
        "NetherNet service target requires an explicit signaling tag and valid recipient",
    )
}

#[cfg(test)]
mod tests;
