//! Original account mint, protected socket and bounded accepting ICE handoff.

pub(super) mod renewal;

use super::*;
use bedrock_net::messaging::server::PreparedMessagingServer;
use bedrock_services::signaling::{
    Event,
    websocket::{MessagingSocket, Options, SocketError},
};
use nethernet_tokio::{CallerServerSignalingConfig, ConnectionConfig, IceServer, ServerIceConfig};
use std::{
    net::{Ipv4Addr, SocketAddr},
    num::NonZeroUsize,
};

/// The same established accepting owner and its original service proof, before publication.
pub(super) struct Acquired {
    pub(super) server: MessagingServer,
    pub(super) identity: protocol::NativeHostingIdentity,
    pub(super) messaging_id: uuid::Uuid,
    pub(super) expires: Instant,
}

/// Acquires transport capability before the original unpublished directory may advertise it.
pub(super) async fn acceptor(
    world: &mut NativeHostedWorld,
    deadline: Instant,
    visitor_limit: usize,
) -> io::Result<Acquired> {
    let (identity, request) = world.directory_mut()?.hosting_request(deadline).await?;
    let deadline = deadline.min(identity.expires_at());
    let messaging_id = request.player_messaging_id();
    let network_id = loop {
        let candidate = rand::random();
        if candidate != 0 {
            break candidate;
        }
    };
    let bound = bind(&identity, &request, network_id, deadline, visitor_limit).await;
    let Bound {
        server, expires, ..
    } = check_issuance(world, &request, deadline, bound).await?;
    Ok(Acquired {
        server,
        identity,
        messaging_id,
        expires,
    })
}

/// A new listener may reuse the still-current original proof key and provider credential.
pub(super) struct Replacement {
    pub(super) server: MessagingServer,
    pub(super) identity: Option<protocol::NativeHostingIdentity>,
    pub(super) messaging_id: uuid::Uuid,
    pub(super) expires: Instant,
    pub(super) ice: ServerIceConfig,
}

struct Bound {
    server: MessagingServer,
    expires: Instant,
    ice: ServerIceConfig,
}

/// Reacquires the protected socket while the same original directory stages its peer continuations.
pub(super) async fn replacement(
    world: &mut NativeHostedWorld,
    original: &protocol::NativeHostingIdentity,
    network_id: u64,
    deadline: Instant,
    visitor_limit: usize,
) -> io::Result<Replacement> {
    let identity = if Instant::now() >= original.expires_at() {
        let NativeHostedWorld {
            directory, peers, ..
        } = world;
        let mint = directory
            .as_mut()
            .ok_or_else(unavailable)?
            .renew_hosting_identity(original, deadline);
        tokio::pin!(mint);
        Some(loop {
            tokio::select! { biased;
                result = &mut mint => break result?,
                _ = peers.next() => {},
            }
        })
    } else {
        None
    };
    let request = {
        let NativeHostedWorld {
            directory, peers, ..
        } = world;
        let request = directory
            .as_mut()
            .ok_or_else(unavailable)?
            .messaging_request(deadline);
        tokio::pin!(request);
        loop {
            tokio::select! { biased;
                result = &mut request => break result?,
                _ = peers.next() => {},
            }
        }
    };
    let selected = identity.as_ref().unwrap_or(original);
    let deadline = deadline.min(selected.expires_at());
    let messaging_id = request.player_messaging_id();
    let bound = world
        .peer_io(
            Some(deadline),
            bind(selected, &request, network_id, deadline, visitor_limit),
        )
        .await;
    let Bound {
        server,
        expires,
        ice,
    } = check_issuance(world, &request, deadline, bound).await?;
    Ok(Replacement {
        server,
        identity,
        messaging_id,
        expires,
        ice,
    })
}

/// Evicts only the actual rejected protected upgrade issuance, without replaying the upgrade.
async fn check_issuance(
    world: &mut NativeHostedWorld,
    request: &bedrock_services::signaling::ConnectionRequest,
    deadline: Instant,
    result: io::Result<Bound>,
) -> io::Result<Bound> {
    if result
        .as_ref()
        .is_err_and(|error| error.kind() == io::ErrorKind::PermissionDenied)
    {
        world
            .directory_mut()?
            .reject_messaging(request, deadline)
            .await?;
    }
    result
}

/// Retains one socket, its correlated TURN result and DNS work under the caller's first cutoff.
async fn bind(
    identity: &protocol::NativeHostingIdentity,
    request: &bedrock_services::signaling::ConnectionRequest,
    network_id: u64,
    deadline: Instant,
    visitor_limit: usize,
) -> io::Result<Bound> {
    let started = Instant::now();
    let options = Options::default();
    let cutoff = deadline.min(started + options.connect_timeout);
    let options = Options {
        connect_timeout: cutoff.saturating_duration_since(started),
        ..options
    };
    let mut socket = match bounded(
        cutoff,
        MessagingSocket::connect(request, network_id, options),
    )
    .await?
    {
        Ok(socket) => socket,
        Err(error) => {
            if error == SocketError::UpgradeStatus(401) {
                return Err(io::Error::new(
                    io::ErrorKind::PermissionDenied,
                    "native host messaging issuance was rejected",
                ));
            }
            return Err(unavailable());
        }
    };
    let request = socket
        .request_credentials(deadline)
        .map_err(|_| unavailable())?;
    loop {
        if let Event::Completed {
            request: completed,
            result,
        } = bounded(deadline, socket.drive())
            .await?
            .map_err(|_| unavailable())?
            && completed == request
        {
            result.map_err(|_| unavailable())?;
            break;
        }
    }
    let credentials = socket.credentials().ok_or_else(unavailable)?;
    let expires = identity.expires_at().min(credentials.expires_at());
    let servers = service_ice_servers(credentials);
    // Keep the original UDP binding; each stream retains its independent numeric target and family.
    let bind_addr = bind_address();
    let resolving = nethernet_tokio::resolve_ice_targets(&servers, bind_addr, expires, deadline);
    tokio::pin!(resolving);
    let resolved = loop {
        tokio::select! {
            _ = tokio::time::sleep_until(deadline.into()) => return Err(expired()),
            result = socket.drive() => { result.map_err(|_| unavailable())?; },
            result = &mut resolving => break result.map_err(|_| unavailable())?,
        }
    };
    let mut server_ice = ServerIceConfig {
        ice: Default::default(),
        bind_addr,
        turn_streams: resolved.turn_streams,
        turn_tls: None,
    };
    server_ice.ice.servers = resolved.datagrams;
    let config = ConnectionConfig {
        max_incoming_message_bytes: bedrock_net::BatchLimits::default().max_encoded_bytes,
        identity: Some(identity.sdp_identity()),
        token_trust: Some(identity.offer_trust()),
        allow_anonymous_offers: true,
        server_ice: Some(server_ice.clone()),
        max_sessions: visitor_limit,
        max_pending: visitor_limit,
        max_pending_per_network_id: visitor_limit,
        ..Default::default()
    };
    let limit = NonZeroUsize::new(visitor_limit).ok_or_else(unavailable)?;
    let signaling = CallerServerSignalingConfig {
        max_routes: limit,
        max_routes_per_sender: limit,
        ..Default::default()
    };
    let server = PreparedMessagingServer::new(socket, network_id)
        .bind(config, signaling, deadline)
        .await
        .map_err(|_| unavailable())?;
    Ok(Bound {
        server,
        expires,
        ice: server_ice,
    })
}

/// Retains the accepting transport's selected UDP family for initial and later offers.
fn bind_address() -> SocketAddr {
    SocketAddr::from((Ipv4Addr::UNSPECIFIED, 0))
}

/// Transfers fresh service endpoint values into the existing resolver's owned input format.
fn service_ice_servers(
    credentials: &bedrock_services::signaling::TurnCredentials,
) -> Vec<IceServer> {
    credentials
        .servers()
        .iter()
        .map(|server| IceServer {
            username: server.username().to_owned(),
            password: server.password().to_owned(),
            urls: server.urls().to_vec(),
        })
        .collect()
}
