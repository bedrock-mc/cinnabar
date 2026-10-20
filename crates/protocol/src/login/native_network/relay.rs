//! The same typed native Login/pack prefix, retained for an application-owned browser relay.

use super::{
    Connection, NativeAuthentication, PacketConnection, failure, negotiated, packs, transport,
};
use crate::RawClientData;
use crate::{
    ProtocolError,
    session_wire::{
        CoreMessage, HandoffPack, PackContentKey, SessionDisconnect, SessionHandoff,
        SessionTransfer,
    },
};
use bedrock_net::raknet::RakNetConnection;
use bedrock_protocol_1_26_51::{
    packets::{Packet, PacketId},
    wire,
};
use bytes::Bytes;
use sha2::{Digest, Sha256};
use std::io;

mod pump;
pub use pump::{
    NativeRelayClosing, NativeRelayDriver, NativeRelayInbound, NativeRelaySender,
    NativeRelayWritten,
};

/// An already-established RakNet socket retained through one caller-polled Login and pack attempt.
///
/// The application must admit the exact socket address, account, controller and deadline before
/// constructing this owner. This type performs no DNS or application authorization. There is no
/// application Send capability until actual Login, packs and first StartGame have completed.
#[must_use]
pub struct NativeRelayConnecting {
    connection: Connection,
    address: String,
    started: bool,
    complete: Option<NativeRelayHandoff>,
    terminal: Option<CoreMessage>,
    startup: Vec<Bytes>,
    startup_bytes: usize,
}

/// Verified selected archives and original startup through the first raw StartGame.
/// The application supplies delivery identity; this owner never manufactures a controller grant.
pub struct NativeRelayHandoff {
    /// Canonical verified identity, selected archive metadata and exact original startup packets.
    pub handoff: SessionHandoff,
    /// Verified archives in the same order as the handoff metadata.
    pub archives: Vec<Vec<u8>>,
}

/// One upstream Login whose first StartGame is handed off to the browser's existing spawn owner.
#[must_use]
pub struct NativeRelayConnection {
    connection: Connection,
    terminal: bool,
}

/// Consumed local send admission, retaining the same accepted FIFO during finite transport teardown.
#[must_use]
pub struct NativeRelayRetired {
    connection: Connection,
    terminal: bool,
}

impl NativeRelayConnecting {
    /// Takes the caller's actual established numeric socket; `address` is Login metadata only.
    /// Admission, original overall deadline and controller cancellation remain the caller's job.
    /// Only known primary-player input reaches Login, the startup handoff and play.
    pub fn from_raknet(connection: RakNetConnection, address: String) -> Self {
        let mut connection = Connection::new(transport::Transport::from_raknet(connection));
        connection.primary_only();
        Self::from_connection(connection, address)
    }

    /// Starts the same private protocol owner on an actual already-established transport.
    fn from_connection(connection: Connection, address: String) -> Self {
        Self {
            connection,
            address,
            started: false,
            complete: None,
            terminal: None,
            startup: Vec::new(),
            startup_bytes: 0,
        }
    }

    /// Runs the sole typed handshake and guarded URL/chunk pack transfer, then captures raw startup.
    /// Dropping this borrowed future seals the attempt but retains the socket for explicit retirement.
    /// The caller must validate downstream client metadata before passing it here; authenticated
    /// identity comes only from the qualified credential's original provider projection.
    pub async fn negotiate(
        &mut self,
        claims: RawClientData,
        client_cache: bool,
        authentication: Option<NativeAuthentication>,
    ) -> Result<(), ProtocolError> {
        if self.started {
            return Err(failure(io::Error::new(
                io::ErrorKind::InvalidInput,
                "relay Login attempt already started",
            )));
        }
        self.started = true;
        let result = self
            .negotiate_inner(claims, client_cache, authentication)
            .await;
        if result.is_err() && self.terminal.is_none() {
            self.terminal = self.connection.current_terminal().map_err(failure)?;
        }
        result
    }

    /// Keeps all intermediate protocol work on the same borrowed transport and original queues.
    async fn negotiate_inner(
        &mut self,
        claims: RawClientData,
        client_cache: bool,
        authentication: Option<NativeAuthentication>,
    ) -> Result<(), ProtocolError> {
        let credentials = negotiated::Credentials::new(authentication)?;
        let (packs, identity) = negotiated::negotiate_connected(
            &mut self.connection,
            &self.address,
            negotiated::LoginData::Relay {
                claims,
                client_cache,
            },
            credentials,
            packs::Acquisition::Public,
        )
        .await?;
        let packs = packs.handoff;
        self.connection.replay_deferred();
        loop {
            let raw = self.connection.next_raw().await.map_err(failure)?;
            if let Some(message) = terminal(&raw).map_err(failure)? {
                self.terminal = Some(message);
                return Err(failure(io::Error::new(
                    io::ErrorKind::ConnectionAborted,
                    "upstream ended before relay handoff",
                )));
            }
            if packet_id(&raw).map_err(failure)? == PacketId::Transfer {
                continue;
            }
            self.startup_bytes = self
                .startup_bytes
                .checked_add(raw.len())
                .filter(|bytes| *bytes <= super::super::startup::MAX_DEFERRED_PACKET_BYTES)
                .ok_or_else(|| {
                    failure(io::Error::new(
                        io::ErrorKind::InvalidData,
                        "relay startup exceeds retained byte limit",
                    ))
                })?;
            let start_game = packet_id(&raw).map_err(failure)? == PacketId::StartGame;
            self.startup.push(Bytes::copy_from_slice(&raw));
            if start_game {
                break;
            }
        }
        let required = packs.required();
        let mut metadata = Vec::with_capacity(packs.len());
        let mut archives = Vec::with_capacity(packs.len());
        for pack in packs.into_archives() {
            metadata.push(HandoffPack {
                sha256: Some(Sha256::digest(&pack.archive).into()),
                uuid: pack.pack_id.to_string(),
                version: pack.version,
                sub_pack: pack.sub_pack_name,
                content_key: PackContentKey::new(
                    String::from_utf8(pack.content_key.expose().to_vec()).map_err(|_| {
                        failure(io::Error::new(
                            io::ErrorKind::InvalidData,
                            "pack content key is not UTF-8",
                        ))
                    })?,
                ),
                size: u64::try_from(pack.archive.len())
                    .map_err(|_| failure(io::Error::other("pack size overflow")))?,
                cache: None,
            });
            archives.push(pack.archive);
        }
        self.complete = Some(NativeRelayHandoff {
            handoff: SessionHandoff {
                identity,
                client_cache,
                packs_required: required,
                packs: metadata,
                startup: std::mem::take(&mut self.startup),
                ..Default::default()
            },
            archives,
        });
        Ok(())
    }

    /// Moves established application I/O only after the actual handoff boundary was reached.
    #[expect(
        clippy::result_large_err,
        reason = "unfinished typestate returns the same transport and pending progress without allocating on retry"
    )]
    pub fn try_finish(mut self) -> Result<(NativeRelayConnection, NativeRelayHandoff), Self> {
        match self.complete.take() {
            Some(handoff) => {
                self.connection.finish_current();
                Ok((
                    NativeRelayConnection {
                        connection: self.connection,
                        terminal: false,
                    },
                    handoff,
                ))
            }
            None => Err(self),
        }
    }

    /// Takes an actual decoded upstream terminal, if negotiation ended at one.
    pub fn take_terminal(&mut self) -> Option<CoreMessage> {
        self.terminal.take()
    }

    /// Retires this attempt without discarding its same-socket cleanup owner.
    pub fn begin_close(mut self) -> NativeRelayRetired {
        if self.terminal.is_some() {
            // This is the same actual decoded frame, rather than a synthesized second terminal.
            self.connection.replay_current();
        }
        self.connection.replay_deferred();
        let startup = self
            .complete
            .take()
            .map_or(self.startup, |handoff| handoff.handoff.startup);
        self.connection.prepend_raw(startup);
        NativeRelayRetired {
            connection: self.connection,
            terminal: false,
        }
    }
}

impl NativeRelayConnection {
    /// Reads one original packet or terminal; siblings remain in the existing connection FIFO.
    pub async fn recv(&mut self) -> io::Result<CoreMessage> {
        receive(&mut self.connection, &mut self.terminal).await
    }

    /// Writes one validated post-Login Batch body through the same codec and directional cipher.
    pub async fn send_body(&mut self, body: Bytes) -> io::Result<()> {
        if self.terminal {
            return Err(io::ErrorKind::NotConnected.into());
        }
        // Validate complete packet framing before advancing the directional cipher.
        crate::raw_packet::decode_batch_body(
            body.clone(),
            bedrock_net::BatchLimits::default().max_decoded_bytes,
        )
        .map_err(io::Error::other)?;
        self.connection.send_body(body).await
    }

    /// Consumes application Send while preserving the same accepted input and transport owner.
    pub fn begin_close(self) -> NativeRelayRetired {
        NativeRelayRetired {
            connection: self.connection,
            terminal: self.terminal,
        }
    }
}

impl NativeRelayRetired {
    /// Finishes the existing transport's finite teardown; a canceled wait retains its original owner.
    pub async fn finish_close(&mut self) -> io::Result<()> {
        self.connection.close().await
    }

    /// Drains accepted input after local Send has been consumed.
    pub async fn recv(&mut self) -> io::Result<CoreMessage> {
        receive(&mut self.connection, &mut self.terminal).await
    }
}

/// Retains accepted packet order and stops after the first actual terminal.
async fn receive(connection: &mut Connection, ended: &mut bool) -> io::Result<CoreMessage> {
    if *ended {
        return Err(io::ErrorKind::UnexpectedEof.into());
    }
    loop {
        let raw = connection.next_raw().await?;
        if packet_id(&raw)? == PacketId::Transfer {
            if let Some(message) = terminal(&raw)? {
                *ended = true;
                return Ok(message);
            }
            continue;
        }
        if let Some(message) = terminal(&raw)? {
            *ended = true;
            return Ok(message);
        }
        return Ok(CoreMessage::Batch(vec![raw]));
    }
}

/// Reads the actual header, including subclients, without reconstructing its original bytes.
fn packet_id(raw: &Bytes) -> io::Result<PacketId> {
    let mut reader = wire::Reader::from_shared(raw);
    let header = wire::PacketHeader::from_raw(reader.read_var_u32().map_err(io::Error::other)?);
    PacketId::from_raw(header.id)
        .ok_or_else(|| io::Error::new(io::ErrorKind::InvalidData, "unknown relay packet"))
}

/// Projects only actual decoded terminal packets, preserving original reason/text/transfer data.
pub(super) fn terminal(raw: &Bytes) -> io::Result<Option<CoreMessage>> {
    let id = packet_id(raw)?;
    if !matches!(id, PacketId::Disconnect | PacketId::Transfer) {
        return Ok(None);
    }
    let mut reader = wire::Reader::from_shared(raw);
    reader.read_var_u32().map_err(io::Error::other)?;
    let packet =
        crate::codec::decode_body(id, &raw.slice(reader.position()..)).map_err(io::Error::other)?;
    match packet {
        Packet::Disconnect(packet) => {
            let (message, filtered_message) = crate::disconnect::packet_texts(&packet);
            Ok(Some(CoreMessage::Disconnect(SessionDisconnect {
                reason: packet.reason.to_raw(),
                message,
                filtered_message,
                hide_screen: matches!(
                    packet.messages,
                    bedrock_protocol_1_26_51::types::DisconnectMessages::Empty
                ),
            })))
        }
        Packet::Transfer(packet) => {
            match crate::ServerTransferEvent::from_packet_data(&Packet::Transfer(packet)) {
                Ok(Some(target)) => Ok(Some(CoreMessage::Transfer(SessionTransfer {
                    address: target.host,
                    port: target.port,
                    reload_world: target.reload_world,
                }))),
                Ok(None) => unreachable!("decoded Transfer selects its actual projection"),
                Err(reason) => {
                    tracing::warn!(?reason, "skipped unusable relay transfer");
                    Ok(None)
                }
            }
        }
        _ => unreachable!("terminal ID selected its decoder"),
    }
}

#[cfg(test)]
mod tests;
