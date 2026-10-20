//! Exclusive native connection state after Login and resource-pack negotiation.

use super::{
    Connection, NativeAuthentication, apply_key_exchange, authentication, failure,
    handshake_failure, packs, transport,
};
use crate::login::client_data::{NativeClaims, RawClientData};
use crate::session_wire::SessionIdentity;
use crate::{NativeConnectionTarget, ProtocolError, SessionLogin};
use bedrock_client::handshake::{
    ClientHandshake, ClientLogin, ClientPackOffer, HandshakeOptions, LoginEvent, SettingsEvent,
};
use bedrock_net::login::OfflineIdentity;
use bedrock_packs::ResourcePackHandoff;
use p384::{ecdsa::SigningKey, elliptic_curve::Generate};
use rand::rngs::SysRng;
use serde_json::{Value, json};
use std::time::Instant;
use std::{io, sync::Arc, time::SystemTime};
use zeroize::Zeroizing;
mod identity;

/// Moves the same codec, ciphers, unread FIFO and selected archives into startup.
pub(super) struct NegotiatedConnection {
    pub(super) connection: Connection,
    pub(super) packs: ResourcePackHandoff,
}

/// Completes the sole transport, Login and pack negotiation under the caller's join deadline.
pub(super) async fn negotiate(
    target: &NativeConnectionTarget,
    login: &SessionLogin<'_>,
    authentication: Option<NativeAuthentication>,
    deadline: tokio::time::Instant,
) -> Result<NegotiatedConnection, ProtocolError> {
    target.validate().map_err(failure)?;
    if target.transport == crate::NativeTransport::LocalNetherNetLan && authentication.is_some() {
        return Err(failure(io::Error::new(
            io::ErrorKind::InvalidInput,
            "local LAN requires its advertised offline admission",
        )));
    }
    if login.target.is_some() {
        return Err(failure(io::Error::new(
            io::ErrorKind::InvalidInput,
            "native join does not accept a core target override",
        )));
    }
    let mut credentials = Credentials::new(authentication)?;
    let (transport, address) = if target.transport == crate::NativeTransport::Addressed {
        let identity = credentials
            .transport_identity()
            .map_err(ProtocolError::Authentication)?;
        match transport::addressed::connect(target, identity, deadline).await {
            Ok((transport, address)) => (Ok(transport), address),
            Err(error) => (Err(error), target.address.clone()),
        }
    } else if matches!(
        target.transport,
        crate::NativeTransport::LocalNetherNetLan | crate::NativeTransport::LocalNetherNetHttp
    ) {
        let (transport, address) = connect_local(target, &credentials, deadline).await?;
        (Ok(transport), address)
    } else if target.transport == crate::NativeTransport::NetherNetLan
        && credentials.authentication.is_some()
    {
        let identity = credentials
            .transport_identity()
            .map_err(ProtocolError::Authentication)?;
        let cutoff = identity
            .expires
            .map_or(deadline, |expires| deadline.min(expires.into()));
        let connected = transport::Transport::connect(target, None, Some(&identity), cutoff).await;
        if let Some(expires) = identity.expires {
            authentication::ensure_current(expires, Instant::now())
                .map_err(ProtocolError::Authentication)?;
        }
        (connected, target.address.clone())
    } else {
        (
            transport::Transport::connect(target, credentials.messaging.take(), None, deadline)
                .await,
            target.address.clone(),
        )
    };
    let transport = transport.map_err(|error| {
        if authentication::is_expired(&error) {
            ProtocolError::Authentication(error)
        } else {
            failure(error)
        }
    })?;
    let mut connection = Connection::new(transport);
    let (packs, _) = negotiate_connected(
        &mut connection,
        &address,
        LoginData::Desktop(login),
        Credentials {
            messaging: None,
            ..credentials
        },
        packs::Acquisition::Desktop {
            cache_root: login.settings.resource_pack_cache_dir.as_deref(),
            parallel_urls: login.settings.parallel_resource_pack_urls,
        },
    )
    .await?;
    Ok(NegotiatedConnection {
        connection,
        packs: packs.handoff,
    })
}

/// Retains one original proof, credential deadline and provider-authenticated player projection.
pub(super) struct Credentials {
    key: Arc<SigningKey>,
    authentication: Option<(Zeroizing<String>, NativeClaims, Instant)>,
    pub(super) messaging: Option<authentication::MessagingAdmission>,
    identity: Option<SessionIdentity>,
    sdp: Option<authentication::SdpPolicy>,
}

impl Credentials {
    /// Uses only the verified guest's presentation/UUID for the original offline local backend.
    /// The host account and guest online XUID/PlayFab credentials never become BDS credentials.
    pub(super) fn local_guest(
        login: &bedrock_net::login::AuthenticatedLogin,
    ) -> Result<Self, ProtocolError> {
        let player = login
            .player_identity()
            .filter(|_| !login.xuid().is_empty())
            .ok_or_else(|| {
                failure(io::Error::new(
                    io::ErrorKind::PermissionDenied,
                    "local hosted upstream requires a verified player",
                ))
            })?;
        let mut credentials = Self::new(None)?;
        credentials.identity = Some(SessionIdentity {
            display_name: player.display_name().into(),
            xuid: String::new(),
            uuid: player.uuid().to_string(),
        });
        Ok(credentials)
    }

    /// Qualifies the original handoff before any connection is admitted or protocol output sent.
    pub(super) fn new(authentication: Option<NativeAuthentication>) -> Result<Self, ProtocolError> {
        let (key, authentication, messaging, identity, sdp) = match authentication {
            Some(authentication) => {
                authentication::ensure_current(authentication.expires, std::time::Instant::now())
                    .map_err(ProtocolError::Authentication)?;
                let handoff = authentication
                    .into_handoff()
                    .map_err(ProtocolError::Authentication)?;
                (
                    handoff.key,
                    Some((handoff.token, handoff.claims, handoff.expires)),
                    handoff.messaging,
                    Some(handoff.identity),
                    Some(handoff.sdp),
                )
            }
            None => (
                Arc::new(
                    SigningKey::try_generate_from_rng(&mut SysRng)
                        .map_err(io::Error::other)
                        .map_err(failure)?,
                ),
                None,
                None,
                None,
                None,
            ),
        };
        Ok(Self {
            key,
            authentication,
            messaging,
            identity,
            sdp,
        })
    }
}

/// Selects only the exact existing local transport and its original Login wire label.
pub(super) async fn connect_local(
    target: &NativeConnectionTarget,
    credentials: &Credentials,
    deadline: tokio::time::Instant,
) -> Result<(transport::Transport, String), ProtocolError> {
    target.validate().map_err(failure)?;
    let (transport, label) = match target.transport {
        crate::NativeTransport::LocalNetherNetLan => {
            let (address, level, budget) = target.local_lan_route().map_err(failure)?;
            let (transport, id) =
                transport::lan::LanTransport::connect_local(address, level, budget, deadline)
                    .await
                    .map_err(failure)?;
            (
                transport::Transport::NetherNetLan(Box::new(transport)),
                id.to_string(),
            )
        }
        crate::NativeTransport::LocalNetherNetHttp => {
            let address = target.local_http_address().map_err(failure)?;
            let identity = credentials
                .transport_identity()
                .map_err(ProtocolError::Authentication)?;
            let connected = transport::http::connect(address, identity, deadline)
                .await
                .map_err(|error| {
                    if authentication::is_expired(&error) {
                        ProtocolError::Authentication(error)
                    } else {
                        failure(error)
                    }
                })?;
            (connected, format!("http://{address}:{}", address.port()))
        }
        _ => {
            return Err(failure(io::Error::new(
                io::ErrorKind::Unsupported,
                "hosted upstream requires its original local HTTP or LAN route",
            )));
        }
    };
    Ok((transport, label))
}

/// Keeps desktop metadata construction in its original phase while accepting the gateway's owned claims.
pub(super) enum LoginData<'a> {
    Desktop(&'a SessionLogin<'a>),
    Relay {
        claims: RawClientData,
        client_cache: bool,
    },
}

impl LoginData<'_> {
    /// Uses only the cache support that the actual downstream claims to implement.
    fn client_cache(&self) -> bool {
        match self {
            Self::Desktop(login) => login.cache.is_some(),
            Self::Relay { client_cache, .. } => *client_cache,
        }
    }
}

/// Negotiates the sole typed Settings/Login/packs sequence on an already-owned concrete transport.
pub(super) async fn negotiate_connected(
    connection: &mut Connection,
    address: &str,
    input: LoginData<'_>,
    credentials: Credentials,
    acquisition: packs::Acquisition<'_>,
) -> Result<(packs::Acquired, SessionIdentity), ProtocolError> {
    let Credentials {
        key,
        authentication,
        messaging,
        identity,
        sdp: _,
    } = credentials;
    if messaging.is_some() {
        return Err(failure(io::Error::new(
            io::ErrorKind::InvalidInput,
            "prepared messaging cannot use a preconnected RakNet socket",
        )));
    }
    let configured = ClientHandshake::new(
        key,
        HandshakeOptions {
            batch_encryption: connection.transport.batch_encryption(),
            blob_cache: input.client_cache(),
            max_login_bytes: connection.max_decoded_bytes(),
            ..Default::default()
        },
    )
    .map_err(handshake_failure)?;
    let configured = if matches!(acquisition, packs::Acquisition::Hosted) {
        configured.retain_pack_metadata()
    } else {
        configured
    };
    let (mut settings, request) = configured.start().map_err(handshake_failure)?;
    connection.send(request).await?;
    let login_ready = loop {
        let packet = connection.next_packet().await.map_err(failure)?;
        match settings.receive(*packet.data).map_err(handshake_failure)? {
            SettingsEvent::Ready { login, compression } => {
                connection
                    .enable_compression(compression)
                    .map_err(failure)?;
                break login;
            }
            SettingsEvent::Pending {
                settings: pending,
                packet,
            } => {
                connection.defer_unhandled(*packet)?;
                settings = pending;
            }
        }
    };
    let (mut claims, token) = if let Some((token, claims, expires)) = authentication {
        (claims, Some((token, expires)))
    } else {
        let claims = match input {
            LoginData::Desktop(login) => {
                let mut claims = super::super::client_data::login_client_data(
                    login.display_name,
                    login.client_random_id,
                    login.skin.as_ref(),
                    login.settings,
                );
                // Signed-out desktop identity is independent of Xbox title/device authentication.
                claims["DeviceOS"] = json!(native_device_os());
                claims["DeviceModel"] = json!("");
                claims["DeviceId"] = json!(uuid::Uuid::new_v4().to_string());
                claims["DefaultInputMode"] = claims["CurrentInputMode"].clone();
                let Value::Object(claims) = claims else {
                    unreachable!("local claim builder returns an object")
                };
                NativeClaims::Canonical(claims)
            }
            LoginData::Relay { claims, .. } => NativeClaims::Relay(claims),
        };
        (claims, None)
    };
    let identity = match identity {
        Some(identity) => identity,
        None => SessionIdentity {
            // The existing typed OfflineIdentity owner retains its original metadata rejection.
            display_name: claims.display_name().map_err(failure)?,
            xuid: String::new(),
            uuid: uuid::Uuid::new_v4().to_string(),
        },
    };
    claims
        .set_transport(address, connection.transport.nonce())
        .map_err(failure)?;
    let claims = claims.into_raw().map_err(failure)?;
    let deadline = token.as_ref().map(|(_, expires)| *expires);
    let (handshake, packet) = if let Some((token, expires)) = token {
        authentication::ensure_current(expires, std::time::Instant::now())
            .map_err(ProtocolError::Authentication)?;
        login_ready.login_authenticated(&token, &claims)
    } else {
        login_ready.login_offline(
            &OfflineIdentity {
                display_name: &identity.display_name,
                identity: &identity.uuid,
                xuid: "",
                playfab_id: "",
                playfab_title_id: "",
            },
            &claims,
            SystemTime::now(),
        )
    }
    .map_err(|error| handshake_failure(error.into_parts().1))?;
    match deadline {
        Some(deadline) => connection.send_before(packet, deadline).await?,
        None => connection.send(packet).await?,
    }
    let offer = receive_offer(connection, handshake).await?;
    let packs = packs::acquire(connection, offer, acquisition).await?;
    connection.identity = Some(identity.clone());
    Ok((packs, identity))
}

/// Processes post-Login controls until the actual offer moves the same key into pack transfer.
async fn receive_offer(
    connection: &mut Connection,
    mut handshake: ClientLogin,
) -> Result<ClientPackOffer, ProtocolError> {
    loop {
        let packet = connection.next_packet().await.map_err(failure)?;
        match handshake.receive(*packet.data).map_err(handshake_failure)? {
            LoginEvent::Send { login, packets } => {
                packs::send_packets(connection, packets).await?;
                handshake = login;
            }
            LoginEvent::KeyExchange(exchange) => {
                handshake = apply_key_exchange(connection, exchange).await?;
            }
            LoginEvent::Packs {
                cache_status,
                packs,
            } => {
                if let Some(status) = cache_status {
                    connection.send(status).await?;
                }
                return Ok(packs);
            }
            LoginEvent::Unhandled { login, packet } => {
                connection.defer_unhandled(*packet)?;
                handshake = login;
            }
        }
    }
}

/// Reports the actual host family for a signed-out connection, independently of OAuth titles.
fn native_device_os() -> i32 {
    if cfg!(target_os = "android") {
        1
    } else if cfg!(target_os = "ios") {
        2
    } else if cfg!(target_os = "macos") {
        3
    } else if cfg!(target_os = "windows") {
        8
    } else {
        15
    }
}
