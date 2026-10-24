//! One original listener and its caller-polled service socket, with no fixture payload queue.

mod legacy;

use super::*;
use bedrock_net::messaging::server::{MessagingServer, PreparedMessagingServer};
use nethernet_tokio::{
    AcceptedSession, CallerServerSignalingConfig, ConnectionConfig, ServerIceConfig,
};
use std::{
    future::Future,
    net::{Ipv4Addr, SocketAddr},
};

enum Listener {
    Messaging(MessagingServer),
    Legacy(legacy::Listener),
}

/// Retains the actual signed host, its original service membership observer and physical listener.
pub struct Host {
    listener: Listener,
    host: HostedFriendHost,
    key: Arc<SigningKey>,
    policy: protocol::test_support::friend_peer::Policy,
}

/// Original play terminal and independent physical listener/socket retirement result.
pub struct Outcome {
    /// Marker/terminal from the actual accepted peer, without replacing the first close cause.
    pub play: io::Result<bool>,
    /// The original accepting service and transport worker joins.
    pub retired: io::Result<()>,
}

impl Host {
    /// Chooses only the fixed scenario's original framing dialect, preserving strict identity trust.
    pub(super) async fn bind(
        mut host: HostedFriendHost,
        identity: protocol::NativeHostingIdentity,
        verifier: bedrock_net::login::provider::ProviderVerifier,
        clock: protocol::NativeServiceClock,
        deadline: Instant,
    ) -> io::Result<Self> {
        let socket = host.socket.take().unwrap();
        let config = ConnectionConfig {
            identity: Some(identity.sdp_identity()),
            token_trust: Some(identity.offer_trust()),
            allow_unsigned_answers: false,
            server_ice: Some(ServerIceConfig {
                turn_streams: Vec::new(),
                turn_tls: None,
                ice: Default::default(),
                bind_addr: SocketAddr::from((Ipv4Addr::LOCALHOST, 0)),
            }),
            max_sessions: 1,
            ..Default::default()
        };
        let listener = if socket.legacy_network_id().is_some() {
            Listener::Legacy(
                legacy::Listener::bind(socket, host.network_id, config, deadline).await?,
            )
        } else {
            Listener::Messaging(
                PreparedMessagingServer::new(socket, host.network_id)
                    .bind(config, CallerServerSignalingConfig::default(), deadline)
                    .await
                    .map_err(failed)?,
            )
        };
        Ok(Self {
            listener,
            host,
            key: identity.signing_key(),
            policy: protocol::test_support::friend_peer::Policy::new(verifier, clock),
        })
    }

    /// Requires real signed Login/member admission and Initialized before notifying the test observer.
    /// All service polling stays alongside the same original peer future and original finite cutoff.
    pub async fn serve(
        mut self,
        initialized: tokio::sync::oneshot::Sender<()>,
        marker: tokio::sync::oneshot::Sender<()>,
        deadline: Instant,
    ) -> Outcome {
        let result = tokio::time::timeout_at(deadline.into(), async {
            let accepted = self.listener.accept().await;
            let accepted = accepted?;
            let peer = protocol::test_support::friend_peer::initialize(
                accepted,
                Arc::clone(&self.key),
                &self.policy,
                |xuid, nonce| self.host.member_admitted(xuid, nonce),
                deadline,
            );
            let peer = self.listener.during(peer).await;
            let peer = peer??;
            let _ = initialized.send(());
            self.listener
                .during(peer.marker_and_close(marker, deadline))
                .await?
        })
        .await
        .unwrap_or_else(|_| {
            Err(io::Error::new(
                io::ErrorKind::TimedOut,
                "controlled friend peer deadline expired",
            ))
        });
        let closed = self.listener.close().await;
        Outcome {
            play: result,
            retired: closed,
        }
    }

    /// Joins only this listener and its original protected socket, including a canceled accept.
    pub async fn close(self) -> io::Result<()> {
        self.listener.close().await
    }
}

impl Listener {
    /// Retains service progress during the original accepting wait.
    async fn accept(&mut self) -> io::Result<AcceptedSession> {
        match self {
            Self::Messaging(listener) => listener.accept().await.map_err(failed),
            Self::Legacy(listener) => listener.accept().await,
        }
    }

    /// Uses one concrete exclusive driver beside the retained application operation.
    async fn during<F: Future>(&mut self, operation: F) -> io::Result<F::Output> {
        match self {
            Self::Messaging(listener) => tokio::select! {
                result = operation => Ok(result),
                terminal = listener.maintain() => Err(failed(terminal)),
            },
            Self::Legacy(listener) => listener.during(operation).await,
        }
    }

    /// Consumes accepting permission and joins the actual original listener/service owners.
    async fn close(self) -> io::Result<()> {
        match self {
            Self::Messaging(listener) => {
                listener.begin_close().finish_close().await.map_err(failed)
            }
            Self::Legacy(listener) => listener.close().await,
        }
    }
}

/// Keeps signed signaling payloads and identities out of fixture errors.
fn failed(_: impl Sized) -> io::Error {
    io::Error::other("controlled friend signaling ended")
}
