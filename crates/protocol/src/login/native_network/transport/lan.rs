//! The actual ready LAN client and its same receiver queues after local retirement.

use super::identity::TransportIdentity;
use super::{AcceptedTransport, LAN_CONNECTION_TYPE};
use bytes::Bytes;
use nethernet_tokio::{ClientIceConfig, ConnectionConfig, LanConfig, LanSignaling, NetherClient};
use std::{io, net::SocketAddr, time::Instant};

pub(in crate::login::native_network) struct LanTransport {
    pub(super) connection: AcceptedTransport,
    nonce: String,
}

impl LanTransport {
    /// Observes the same established transport without retaining its send capability.
    pub(super) fn observe_rtt(&self) -> Option<crate::RoundTripTime> {
        self.connection.observe_rtt()
    }

    /// Discovers and connects one compatible IPv4 endpoint under the selection owner's cutoff.
    pub(super) async fn connect(
        address: SocketAddr,
        identity: Option<&TransportIdentity>,
        deadline: tokio::time::Instant,
    ) -> io::Result<Self> {
        if let Some(expires) = identity.and_then(|identity| identity.expires) {
            super::super::authentication::ensure_current(expires, Instant::now())?;
        }
        let result = Self::connect_route(address, None, identity, identity.map(|_| deadline)).await;
        if let Some(expires) = identity.and_then(|identity| identity.expires) {
            super::super::authentication::ensure_current(expires, Instant::now())?;
        }
        result.map(|(transport, _)| transport)
    }

    /// Retains the selected local level and one finite discovery budget on the original LAN owner.
    pub(in crate::login::native_network) async fn connect_local(
        address: SocketAddr,
        level: &str,
        discovery_budget: std::time::Duration,
        deadline: tokio::time::Instant,
    ) -> io::Result<(Self, u64)> {
        if !address.is_ipv4()
            || !address.ip().is_loopback()
            || address.port() == 0
            || level.is_empty()
            || discovery_budget.is_zero()
        {
            return Err(io::Error::new(
                io::ErrorKind::InvalidInput,
                "invalid selected local LAN route",
            ));
        }
        check_deadline(deadline)?;
        let result = tokio::time::timeout_at(
            deadline,
            Self::connect_route(
                address,
                Some((level, discovery_budget)),
                None,
                Some(deadline),
            ),
        )
        .await;
        check_deadline(deadline)?;
        result.map_err(|_| timed_out())?
    }

    /// Shares the actual directed discovery/ready association without creating another protocol owner.
    async fn connect_route(
        address: SocketAddr,
        selected: Option<(&str, std::time::Duration)>,
        identity: Option<&TransportIdentity>,
        deadline: Option<tokio::time::Instant>,
    ) -> io::Result<(Self, u64)> {
        let _ = rustls::crypto::ring::default_provider().install_default();
        let local = if selected.is_some() {
            SocketAddr::new(address.ip(), 0)
        } else {
            let route = tokio::net::UdpSocket::bind("0.0.0.0:0").await?;
            route.connect(address).await?;
            SocketAddr::new(route.local_addr()?.ip(), 0)
        };
        let signaling = LanSignaling::with_config(
            rand::random(),
            local,
            LanConfig {
                broadcast_address: Some(address),
                ..LanConfig::default()
            },
        )
        .await
        .map_err(io::Error::other)?;
        let discovery_deadline = selected
            .map(|(_, budget)| {
                tokio::time::Instant::now()
                    .checked_add(budget)
                    .ok_or_else(|| {
                        io::Error::new(
                            io::ErrorKind::InvalidInput,
                            "invalid local LAN discovery budget",
                        )
                    })
            })
            .transpose()?
            .map(|until| deadline.map_or(until, |deadline| deadline.min(until)));
        let mut skipped = 0u64;
        let discovering = async {
            let mut polls = tokio::time::interval(std::time::Duration::from_millis(50));
            loop {
                polls.tick().await;
                let mut found = None;
                for (id, data) in signaling.discover().await {
                    if signaling.get_address(id).await == Some(address)
                        && selected.is_none_or(|(level, _)| data.level_name == level)
                    {
                        if selected.is_some() {
                            if !data.flag_b {
                                return Err(io::Error::new(
                                    io::ErrorKind::PermissionDenied,
                                    "local LAN peer does not advertise offline admission",
                                ));
                            }
                            if data.connection_type != LAN_CONNECTION_TYPE
                                || data.session_id.is_empty()
                                || data.session_id.len() > crate::MAX_NATIVE_JOIN_NONCE_BYTES
                                || data.session_id.chars().any(char::is_control)
                            {
                                skipped = skipped.saturating_add(1);
                                continue;
                            }
                        }
                        found = Some((id, data));
                        break;
                    }
                }
                if let Some(found) = found {
                    break Ok(found);
                }
            }
        };
        let discovered = match discovery_deadline {
            Some(until) => {
                check_deadline(until)?;
                let data = tokio::time::timeout_at(until, discovering).await;
                match check_deadline(until) {
                    Ok(()) => data.map_err(|_| timed_out()).and_then(|data| data),
                    Err(error) => Err(error),
                }
            }
            None => discovering.await,
        };
        if skipped != 0 {
            tracing::warn!(
                count = skipped,
                "skipped unsupported selected local LAN metadata"
            );
        }
        let (id, data) = discovered?;
        if identity.is_some() {
            if !data.flag_a {
                return Err(io::Error::new(
                    io::ErrorKind::PermissionDenied,
                    "LAN peer does not advertise online admission",
                ));
            }
            if data.session_id.is_empty()
                || data.session_id.len() > crate::MAX_NATIVE_JOIN_NONCE_BYTES
                || data.session_id.chars().any(char::is_control)
            {
                return Err(io::Error::new(
                    io::ErrorKind::InvalidData,
                    "LAN peer advertises an invalid admission nonce",
                ));
            }
        } else if !data.flag_b || data.session_id.is_empty() {
            return Err(io::Error::new(
                io::ErrorKind::PermissionDenied,
                "LAN peer does not advertise self-signed admission with a nonce",
            ));
        }
        if data.connection_type != LAN_CONNECTION_TYPE
            || (selected.is_none() && data.protocol_version != crate::PROTOCOL_VERSION as u32)
        {
            return Err(io::Error::new(
                io::ErrorKind::Unsupported,
                "LAN peer advertises an incompatible transport or protocol",
            ));
        }
        let mut config = ConnectionConfig {
            allow_unsigned_answers: true,
            attempts: 1,
            ..ConnectionConfig::default()
        };
        if let Some(identity) = identity {
            config.identity = Some(std::sync::Arc::clone(&identity.identity));
            config.server_trust = Some(identity.trust.clone());
        }
        if selected.is_some() {
            config.server_trust = Some(super::super::authentication::offline_answer_trust());
            config.allow_anonymous_answers = true;
        }
        if selected.is_some() || identity.is_some() {
            config.client_ice = Some(ClientIceConfig {
                turn_streams: Vec::new(),
                turn_tls: None,
                ice: Default::default(),
                bind_addr: Some(local),
            });
        }
        if let Some(deadline) = deadline {
            check_deadline(deadline)?;
            config.timeouts.negotiation = config
                .timeouts
                .negotiation
                .min(deadline.saturating_duration_since(tokio::time::Instant::now()));
        }
        let connection = NetherClient::connect_with(signaling, id.to_string(), config)
            .await
            .map_err(io::Error::other)?;
        if let Some(deadline) = deadline {
            check_deadline(deadline)?;
        }
        Ok((Self::new(connection, data.session_id), id))
    }

    /// Retains only an actual ready client and its discovery challenge on the same owner.
    pub(super) fn new(connection: NetherClient, nonce: String) -> Self {
        Self {
            connection: AcceptedTransport::new(connection.into_accepted()),
            nonce,
        }
    }

    /// Borrows the original discovery nonce without inferring any authenticated account authority.
    pub(super) fn nonce(&self) -> &str {
        &self.nonce
    }

    /// Polls original ingress beside established sending without duplicating lifecycle ownership.
    pub(super) async fn send_until(
        &mut self,
        message: Bytes,
        deadline: Option<Instant>,
    ) -> io::Result<()> {
        if let Some(deadline) = deadline {
            super::super::authentication::ensure_current(deadline, Instant::now())?;
        }
        let result = self.connection.send_until(message, deadline).await;
        if let Some(deadline) = deadline {
            super::super::authentication::ensure_current(deadline, Instant::now())?;
        }
        result
    }

    /// Drains the same ready/retired receiver and retained original send prefix.
    pub(super) async fn recv(&mut self) -> io::Result<Bytes> {
        self.connection.recv().await
    }

    /// Consumes ready admission while preserving the same original receiver queues.
    pub(super) fn close(&mut self) {
        self.connection.close();
    }
}

/// Rejects late discovery/ready results without replacing the original absolute cutoff.
fn check_deadline(deadline: tokio::time::Instant) -> io::Result<()> {
    if tokio::time::Instant::now() >= deadline {
        return Err(timed_out());
    }
    Ok(())
}

/// Keeps endpoint/level/nonce values out of timeout diagnostics.
fn timed_out() -> io::Error {
    io::Error::new(
        io::ErrorKind::TimedOut,
        "native local LAN discovery or join timed out",
    )
}

#[cfg(test)]
mod tests;
