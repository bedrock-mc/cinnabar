//! Selected-account protected signaling acquisition for an explicit joined session.

use super::*;
use bedrock_net::messaging::{PreparedMessaging, Recipient};
use bedrock_services::signaling::websocket::{MessagingSocket, Options, SocketError};
use prepare::check_preparation_deadline;

impl NativeAccount {
    /// Prepares signed NetherNet on the same selected account that mints its multiplayer token.
    ///
    /// The caller supplies a typed signaling route and join nonce obtained from an
    /// explicit admission flow. This does not join MPSD, resolve cards, host a world
    /// or admit a friend listing. Poll on the existing session runtime; dropping
    /// the future retires its socket and persists already visible OAuth rotation.
    pub async fn prepare_messaging(
        self: Arc<Self>,
        target: protocol::NativeMessagingTarget,
        claims: Value,
        random_id: protocol::ClientRandomId,
    ) -> io::Result<(
        protocol::NativeConnectionTarget,
        protocol::NativeAuthentication,
    )> {
        if !claims.is_object() {
            return Err(io::Error::new(
                io::ErrorKind::InvalidInput,
                "native client data must be a JSON object",
            ));
        }
        let deadline = tokio::time::Instant::now() + ACQUISITION_TIMEOUT;
        let result = tokio::time::timeout_at(deadline, async {
            check_preparation_deadline(deadline)?;
            let mut operation = Arc::clone(&self).acquire_verified().await?;
            check_preparation_deadline(deadline)?;
            operation.promote()?;
            self.prepare_messaging_borrowed(&mut operation, target, claims, random_id, deadline)
                .await
        })
        .await
        .map_err(|_| {
            io::Error::new(
                io::ErrorKind::TimedOut,
                "native messaging account preparation timed out",
            )
        })?;
        check_preparation_deadline(deadline)?;
        result
    }

    /// Prepares the protected socket and mint under an already verified selected-account operation.
    /// The caller retains its original overall deadline and any joined-session cleanup owner.
    pub(super) async fn prepare_messaging_borrowed(
        &self,
        operation: &mut Operation,
        target: protocol::NativeMessagingTarget,
        claims: Value,
        random_id: protocol::ClientRandomId,
        deadline: tokio::time::Instant,
    ) -> io::Result<(
        protocol::NativeConnectionTarget,
        protocol::NativeAuthentication,
    )> {
        let peer = target.recipient().clone();
        let (prepared, authentication) = self
            .prepare_messaging_socket_borrowed(operation, peer, claims, random_id, deadline)
            .await?;
        let endpoint = target.connection_target();
        let authentication = authentication.with_messaging(prepared, target)?;
        Ok((endpoint, authentication))
    }

    /// Acquires one protected socket and mint under the caller's original account lease and cutoff.
    /// Realm and joined-friend policy remain with their distinct callers.
    pub(super) async fn prepare_messaging_socket_borrowed(
        &self,
        operation: &mut Operation,
        peer: Recipient,
        claims: Value,
        random_id: protocol::ClientRandomId,
        deadline: tokio::time::Instant,
    ) -> io::Result<(PreparedMessaging, protocol::NativeAuthentication)> {
        if !std::ptr::eq(self, operation.account.as_ref()) {
            return Err(io::Error::new(
                io::ErrorKind::PermissionDenied,
                "native messaging operation belongs to another account",
            ));
        }
        if !claims.is_object() {
            return Err(io::Error::new(
                io::ErrorKind::InvalidInput,
                "native client data must be a JSON object",
            ));
        }
        if matches!(&peer, Recipient::Messaging(peer) if peer.is_nil())
            || matches!(&peer, Recipient::Legacy(peer) if peer.is_empty())
        {
            return Err(io::Error::new(
                io::ErrorKind::InvalidInput,
                "native messaging recipient is absent",
            ));
        }
        check_preparation_deadline(deadline)?;
        // Finish minting and public-key acquisition before opening a socket
        // that needs continuous heartbeat driving.
        let authentication = self.prepare_borrowed(operation, claims, random_id).await?;
        check_preparation_deadline(deadline)?;
        let network_id = rand::random();
        let request = match &peer {
            Recipient::Messaging(_) => operation.minecraft()?.messaging_connection().await,
            Recipient::Legacy(_) => {
                operation
                    .minecraft()?
                    .legacy_signaling_connection(network_id)
                    .await
            }
        }
        .map_err(|_| messaging_error())?;
        check_preparation_deadline(deadline)?;
        // Publish any credential rotation before awaiting an unrelated network upgrade.
        operation.persist()?;
        check_preparation_deadline(deadline)?;
        let started = tokio::time::Instant::now();
        let options = Options::default();
        let upgrade_deadline = deadline
            .min(authentication.expires_at().into())
            .min(started + options.connect_timeout);
        check_preparation_deadline(upgrade_deadline)?;
        let options = Options {
            connect_timeout: upgrade_deadline.saturating_duration_since(started),
            ..options
        };
        let socket = match tokio::time::timeout_at(
            upgrade_deadline,
            MessagingSocket::connect(&request, network_id, options),
        )
        .await
        .map_err(|_| {
            io::Error::new(
                io::ErrorKind::TimedOut,
                "native messaging credential expired during upgrade",
            )
        })? {
            Ok(socket) => socket,
            Err(error) => {
                if error == SocketError::UpgradeStatus(401) {
                    // A delayed refusal cannot invalidate a replacement or another owner's issuance.
                    operation.minecraft()?.reject_messaging(&request);
                    operation.persist()?;
                }
                return Err(messaging_error());
            }
        };
        check_preparation_deadline(upgrade_deadline)?;
        let prepared = match peer {
            Recipient::Messaging(peer) => PreparedMessaging::new(socket, network_id, peer),
            Recipient::Legacy(peer) => PreparedMessaging::new_legacy(socket, network_id, peer),
        }
        .map_err(|_| messaging_error())?;
        operation.persist()?;
        check_preparation_deadline(upgrade_deadline)?;
        Ok((prepared, authentication))
    }
}

/// Omits protected destination, authorization, frame and provider diagnostics.
fn messaging_error() -> io::Error {
    io::Error::other("native messaging acquisition failed; retry or sign in again")
}
