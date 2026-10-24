//! Original Login proof carried by a directly selected signaling transport.

use nethernet::identity::{SdpIdentity, TokenTrust};
use std::{sync::Arc, time::Instant};

/// Metadata selects a route; only this original proof and policy authenticate its SDP.
pub(in crate::login::native_network) struct TransportIdentity {
    pub(in crate::login::native_network) identity: Arc<dyn SdpIdentity>,
    pub(in crate::login::native_network) trust: TokenTrust,
    pub(in crate::login::native_network) expires: Option<Instant>,
}
