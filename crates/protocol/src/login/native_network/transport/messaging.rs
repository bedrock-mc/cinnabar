//! Account/selected-route admission before the real established messaging lifecycle.

use super::Transport;
use crate::login::native_network::authentication::{MessagingAdmission, ensure_current, expired};
use nethernet_tokio::ConnectionConfig;
use std::{io, time::Instant};

/// Checks the exact joined-session route before using the protected socket.
pub(super) async fn connect(
    endpoint: &crate::NativeConnectionTarget,
    admission: Option<MessagingAdmission>,
) -> io::Result<Transport> {
    let admission = admission.ok_or_else(|| {
        io::Error::new(
            io::ErrorKind::PermissionDenied,
            "native messaging requires selected-account admission",
        )
    })?;
    let target = admission.target.connection_target();
    if endpoint.address != target.address || endpoint.transport != target.transport {
        return Err(io::Error::new(
            io::ErrorKind::InvalidInput,
            "native messaging target mismatch",
        ));
    }
    ensure_current(admission.expires, Instant::now())?;
    let expires = admission.expires;
    let connection = admission
        .prepared
        .connect(
            ConnectionConfig {
                max_incoming_message_bytes: bedrock_net::BatchLimits::default().max_encoded_bytes,
                identity: Some(admission.identity),
                server_trust: Some(admission.trust),
                allow_anonymous_answers: admission.target.allow_anonymous_answers(),
                attempts: 1,
                ..ConnectionConfig::default()
            },
            admission.expires,
        )
        .await
        .map_err(|error| {
            if Instant::now() >= expires {
                expired()
            } else {
                io::Error::other(error)
            }
        })?;
    Ok(Transport::from_messaging(
        connection,
        admission.target.nonce().to_owned(),
    ))
}
