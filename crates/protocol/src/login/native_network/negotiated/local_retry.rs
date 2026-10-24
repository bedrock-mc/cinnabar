//! A single keyless retry confined to the launcher's selected loopback HTTP server.

use super::*;
use crate::session_wire::CoreMessage;
use bedrock_client_tokio::PacketIo;
use bedrock_protocol_1_26_51::enums::ConnectionDisconnectFailReason;

/// Recognizes only the original failed Login's actual typed keyless disconnect.
pub(super) fn is_keyless(connection: &Connection, error: &ProtocolError) -> bool {
    error.server_disconnect().is_some()
        && matches!(
            super::super::connection::current_terminal(connection),
            Ok(Some(CoreMessage::Disconnect(disconnect)))
                if crate::disconnect::disconnect_reason(disconnect.reason)
                    == ConnectionDisconnectFailReason::Hostwithoutkeys
        )
}

/// Retires the failed association before one offline Login under the original join cutoff.
/// Account credentials remain with the application for any later transfer or remote join.
pub(super) async fn retry(
    mut connection: Connection,
    target: &NativeConnectionTarget,
    login: &SessionLogin<'_>,
    mut identity: SessionIdentity,
    deadline: tokio::time::Instant,
) -> Result<NegotiatedConnection, ProtocolError> {
    target.local_http_address().map_err(failure)?;
    tokio::time::timeout_at(deadline, connection.finish_close())
        .await
        .map_err(|_| {
            failure(io::Error::new(
                io::ErrorKind::TimedOut,
                "local login retry cleanup timed out",
            ))
        })?
        .map_err(failure)?;
    drop(connection);
    if tokio::time::Instant::now() >= deadline {
        return Err(failure(io::Error::new(
            io::ErrorKind::TimedOut,
            "local login retry exceeded the join deadline",
        )));
    }
    tracing::warn!(
        reason = "server_verification_keys_unavailable",
        "retrying local server login with local identity"
    );
    let mut credentials = Credentials::new(None)?;
    identity.xuid.clear();
    credentials.identity = Some(identity);
    // Without account credentials this attempt cannot authorize another retry.
    Box::pin(super::attempt(target, login, credentials, deadline)).await
}
