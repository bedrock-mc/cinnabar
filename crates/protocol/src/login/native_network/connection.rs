//! Compression, encryption and packet FIFO retained for the entire connection.

use super::super::startup::{DeferredPackets, MAX_DEFERRED_PACKET_BYTES};
use super::transport::Transport;
use crate::{Packet, packet::GameHeader};
use bedrock_net::encryption::{BatchDecryptor, BatchEncryptor};
use bedrock_net::{BatchCodec, BatchError, BatchLimits, CompressionSettings, DecodeOutcome};
use bedrock_protocol_1_26_51::{packets, wire};
use bytes::{Bytes, BytesMut};
use std::io;

mod codec;
mod identity;
mod pending;
use codec::Codec;
use pending::PendingPackets;
pub(super) mod send;
use send::{PendingSend, SendRetirement};

/// Exclusive packet I/O consumed by the native session pump.
pub(crate) trait PacketConnection: Send + 'static {
    /// Observes transport estimates without another I/O owner or application probe.
    fn observe_rtt(&self) -> Option<crate::RoundTripTime> {
        None
    }
    /// Sends one complete batch without cancelling cipher or transport progress.
    fn send_body(
        &mut self,
        body: Bytes,
    ) -> impl std::future::Future<Output = io::Result<()>> + Send;
    /// Retains partial input when the pump cancels a receive wait.
    fn recv_body(&mut self) -> impl std::future::Future<Output = io::Result<Bytes>> + Send;
    /// Progresses caller-owned service/control work without consuming application payloads.
    /// A terminal cause remains staged for ordinary reads to drain accepted data first.
    fn maintain(&mut self) -> impl std::future::Future<Output = ()> + Send {
        std::future::pending()
    }
    /// Flushes accepted transport output and ends this connection.
    fn close(&mut self) -> impl std::future::Future<Output = io::Result<()>> + Send;
}

pub(crate) struct Connection {
    pub(super) transport: Transport,
    codec: Option<Codec>,
    limits: BatchLimits,
    incoming_decoded_limit: usize,
    pending: PendingPackets,
    current: Option<Bytes>,
    deferred: DeferredPackets,
    ciphers: Option<(BatchEncryptor, BatchDecryptor)>,
    pending_send: Option<PendingSend>,
    send_retirement: Option<SendRetirement>,
    send_sealed: bool,
    negotiation_skips: u64,
    primary_only: bool,
    pub(super) identity: Option<crate::session_wire::SessionIdentity>,
    #[cfg(test)]
    observed_batches: Option<BatchLifetimes>,
}

/// Weak observations of actual socket input allocations; never retains the bytes being measured.
#[cfg(test)]
type BatchLifetimes = std::sync::Arc<std::sync::Mutex<Vec<(usize, std::sync::Weak<()>)>>>;

#[cfg(test)]
tokio::task_local! {
    /// Scopes existing weak batch observations to an actual public connection future.
    pub(super) static OBSERVE_BATCHES: BatchLifetimes;
}

/// Field order releases the real input allocation before its observation becomes expired.
#[cfg(test)]
struct ObservedBatch {
    bytes: Bytes,
    _lifetime: std::sync::Arc<()>,
}

#[cfg(test)]
impl AsRef<[u8]> for ObservedBatch {
    fn as_ref(&self) -> &[u8] {
        &self.bytes
    }
}

impl Connection {
    /// Retains usable redirects and counts unusable decoded targets without ending the join.
    pub(super) fn negotiation_failure(
        &mut self,
        packet: packets::Packet,
    ) -> Option<crate::ProtocolError> {
        match super::negotiation_failure(packet) {
            Ok(error) => Some(error),
            Err(reason) => {
                self.negotiation_skips = self.negotiation_skips.saturating_add(1);
                tracing::warn!(
                    ?reason,
                    skipped = self.negotiation_skips,
                    "skipped unusable negotiation transfer"
                );
                None
            }
        }
    }
    /// Starts an upstream session, retaining its larger incoming policy until actual settings.
    pub(super) fn new(transport: Transport) -> Self {
        let mut connection = Self::with_limits(transport, BatchLimits::default());
        connection.incoming_decoded_limit = bridge::contract::MAX_UPSTREAM_DECOMPRESSED_BATCH_BYTES;
        connection
    }

    /// Binds the accepting role's original byte/count policy before any codec transition.
    pub(super) fn with_limits(transport: Transport, limits: BatchLimits) -> Self {
        Self {
            codec: Some(Codec::Plain(BatchCodec::new(transport.header(), limits))),
            transport,
            limits,
            incoming_decoded_limit: limits.max_decoded_bytes,
            pending: PendingPackets::default(),
            current: None,
            deferred: DeferredPackets::default(),
            ciphers: None,
            pending_send: None,
            send_retirement: None,
            send_sealed: false,
            negotiation_skips: 0,
            primary_only: false,
            identity: None,
            #[cfg(test)]
            observed_batches: OBSERVE_BATCHES.try_with(std::sync::Arc::clone).ok(),
        }
    }

    /// Applies the local relay's primary-player policy before any generated body is decoded.
    pub(super) fn primary_only(&mut self) {
        self.primary_only = true;
    }

    /// Instruments actual received batch ownership without replacing transport or codec behavior.
    #[cfg(test)]
    pub(super) fn observe_batches(&mut self, observations: BatchLifetimes) {
        self.observed_batches = Some(observations);
    }

    /// Adds only a weak lifetime observation around the same real socket bytes before decoding.
    #[cfg(test)]
    fn observe_batch(&self, bytes: Bytes) -> Bytes {
        let Some(observations) = &self.observed_batches else {
            return bytes;
        };
        let lifetime = std::sync::Arc::new(());
        observations
            .lock()
            .unwrap()
            .push((bytes.len(), std::sync::Arc::downgrade(&lifetime)));
        Bytes::from_owner(ObservedBatch {
            bytes,
            _lifetime: lifetime,
        })
    }

    /// The shared decoded-byte bound for Login and subsequent packets.
    pub(super) fn max_decoded_bytes(&self) -> usize {
        self.limits.max_decoded_bytes
    }

    /// Applies negotiated compression without replacing unread packets.
    pub(super) fn enable_compression(&mut self, settings: CompressionSettings) -> io::Result<()> {
        self.ensure_send_idle()?;
        let current = self.codec.take().expect("codec transition is synchronous");
        match current {
            Codec::Plain(codec) => {
                self.codec = Some(Codec::Negotiated(
                    codec
                        .negotiate(settings)
                        .with_incoming_decoded_limit(self.incoming_decoded_limit),
                ));
                Ok(())
            }
            configured @ Codec::Negotiated(_) => {
                self.codec = Some(configured);
                Err(io::Error::new(
                    io::ErrorKind::InvalidData,
                    "repeated network settings",
                ))
            }
        }
    }

    /// Borrows the actual codec; its only move occurs in a synchronous settings transition.
    fn codec(&self) -> &Codec {
        self.codec
            .as_ref()
            .expect("codec transition is synchronous")
    }

    /// Installs both directional streams once, before the client handshake acknowledgment.
    pub(super) fn enable_encryption(
        &mut self,
        ciphers: (BatchEncryptor, BatchDecryptor),
    ) -> io::Result<()> {
        self.ensure_send_idle()?;
        if self.ciphers.is_some() {
            return Err(io::Error::new(
                io::ErrorKind::InvalidData,
                "repeated server key exchange",
            ));
        }
        self.ciphers = Some(ciphers);
        Ok(())
    }

    /// Sends one negotiation response, preserving received terminal input if writing fails.
    pub(super) async fn send<P>(&mut self, packet: P) -> Result<(), crate::ProtocolError>
    where
        packets::Packet: From<P>,
    {
        self.send_packets(&[Packet::new(packet)]).await
    }

    /// Sends a bounded negotiation batch immediately, retaining terminal input on a failed write.
    pub(super) async fn send_packets(
        &mut self,
        packets: &[Packet],
    ) -> Result<(), crate::ProtocolError> {
        self.send_packets_until(packets, None).await
    }

    /// Admits Login only while its original credential remains valid, including capacity waits.
    pub(super) async fn send_before<P>(
        &mut self,
        packet: P,
        deadline: std::time::Instant,
    ) -> Result<(), crate::ProtocolError>
    where
        packets::Packet: From<P>,
    {
        self.send_packets_until(&[Packet::new(packet)], Some(deadline))
            .await
    }

    /// Preserves received terminal input on transport failure, but reports expiry as authentication failure.
    pub(super) async fn send_packets_until(
        &mut self,
        packets: &[Packet],
        deadline: Option<std::time::Instant>,
    ) -> Result<(), crate::ProtocolError> {
        let body = crate::session_transport::batch_body(packets)
            .ok_or_else(|| {
                io::Error::new(io::ErrorKind::InvalidData, "native packet could not encode")
            })
            .map_err(super::failure)?;
        match self.send_body_until(body, deadline).await {
            Ok(()) => Ok(()),
            Err(error) if super::authentication::is_expired(&error) => {
                Err(crate::ProtocolError::Authentication(error))
            }
            Err(error) => Err(super::preserve_negotiation_boundary(self, error).await),
        }
    }

    /// Encodes without restarting a finite Login credential deadline.
    pub(super) async fn send_body_until(
        &mut self,
        body: Bytes,
        deadline: Option<std::time::Instant>,
    ) -> io::Result<()> {
        self.ensure_send_idle()?;
        for message in self.encode_body(body)? {
            let message = self.encrypt_message(message)?;
            self.transport.send_until(message, deadline).await?;
        }
        Ok(())
    }

    /// Encodes the original bounded packet entries once on the current codec.
    fn encode_body(&self, body: Bytes) -> io::Result<Vec<Bytes>> {
        Self::check_body_size(&body)?;
        let entries = wire::Frames::new(body, self.limits.max_packets)
            .map(|frame| {
                let frame = frame.map_err(io::Error::other)?;
                identity::entry(frame, self.identity.as_ref())
            })
            .collect::<io::Result<Vec<_>>>()?;
        self.codec().encode(&entries).map_err(io::Error::other)
    }

    /// Advances the sole original directional cipher and checks its actual envelope size.
    fn encrypt_message(&mut self, mut message: Bytes) -> io::Result<Bytes> {
        if let Some((encryptor, _)) = &mut self.ciphers {
            let marker = self.transport.header().marker();
            let body = if marker.is_some() {
                message.slice(1..)
            } else {
                message
            };
            message = envelope(marker, encryptor.encrypt(body).map_err(io::Error::other)?);
            self.check_wire_size(&message)?;
        }
        Ok(message)
    }

    /// Frames one original raw packet under the same decoded-body limit.
    pub(super) fn raw_body(&self, raw: Bytes) -> io::Result<Bytes> {
        let limit = self.max_decoded_bytes();
        let mut framed = Vec::new();
        wire::Writer::with_limit(&mut framed, limit)
            .write_len_var_u32(raw.len())
            .map_err(io::Error::other)?;
        if raw.is_empty()
            || framed
                .len()
                .checked_add(raw.len())
                .is_none_or(|n| n > limit)
        {
            return Err(io::Error::new(
                io::ErrorKind::InvalidData,
                "invalid native raw packet",
            ));
        }
        framed.extend_from_slice(&raw);
        Ok(Bytes::from(framed))
    }

    /// Refuses an oversized queue item before it can retain connection capacity.
    pub(crate) fn check_body_size(body: &[u8]) -> io::Result<()> {
        if body.len() > BatchLimits::default().max_decoded_bytes {
            return Err(io::Error::new(
                io::ErrorKind::InvalidInput,
                "native outgoing batch exceeds decoded byte limit",
            ));
        }
        Ok(())
    }

    /// Reads a complete batch, retaining decoded siblings across every protocol transition.
    async fn fill(&mut self) -> io::Result<()> {
        while !self.pending_ready()? {
            let mut message = self.transport.recv().await?;
            self.check_wire_size(&message)?;
            if let Some((_, decryptor)) = &mut self.ciphers {
                let marker = self.transport.header().marker();
                let body = if let Some(marker) = marker {
                    let actual = *message
                        .first()
                        .ok_or_else(|| io::Error::other(BatchError::MissingBatchHeader))?;
                    if actual != marker {
                        return Err(io::Error::other(BatchError::InvalidBatchHeader(actual)));
                    }
                    message.slice(1..)
                } else {
                    message
                };
                message = envelope(marker, decryptor.decrypt(body).map_err(io::Error::other)?);
            }
            #[cfg(test)]
            let message = self.observe_batch(message);
            match self.codec().decode(message).map_err(io::Error::other)? {
                DecodeOutcome::Packets {
                    packets,
                    skipped_empty_packets,
                    ..
                } => {
                    if skipped_empty_packets != 0 {
                        tracing::debug!(
                            skipped_empty_packets,
                            "skipped empty native batch entries"
                        );
                    }
                    self.pending.push_batch(packets);
                }
                DecodeOutcome::Dropped(reason) => tracing::debug!(?reason, "dropped native batch"),
            }
        }
        Ok(())
    }

    /// Filters only the actual FIFO front, keeping one lazy cursor behind every replayed prefix.
    fn pending_ready(&mut self) -> io::Result<bool> {
        while let Some(packet) = self.pending.front() {
            if !self.primary_only {
                return Ok(true);
            }
            let mut reader = wire::Reader::from_shared(packet);
            let header =
                wire::PacketHeader::from_raw(reader.read_var_u32().map_err(io::Error::other)?);
            if header.sender_subclient == 0 && header.target_subclient == 0 {
                return Ok(true);
            }
            drop(self.pending.pop_front());
            self.negotiation_skips = self.negotiation_skips.saturating_add(1);
            tracing::debug!(
                skipped = self.negotiation_skips,
                "skipped unused local relay subclient"
            );
        }
        Ok(false)
    }

    /// Restores early raw frames ahead of unread siblings on this same FIFO.
    pub(super) fn replay_deferred(&mut self) {
        for packet in self.take_deferred().into_iter().rev() {
            self.pending.push_front(packet.encoded());
        }
    }

    /// Removes one original complete header/body while retaining siblings for the next call.
    pub(super) async fn next_raw(&mut self) -> io::Result<Bytes> {
        self.current = None;
        self.fill().await?;
        let bytes = self.pending.pop_front().expect("fill supplies packets");
        self.current = Some(bytes.clone());
        Ok(bytes)
    }

    /// Restores actual already-consumed handoff frames ahead of the original unread siblings.
    pub(super) fn prepend_raw(&mut self, packets: Vec<Bytes>) {
        for packet in packets.into_iter().rev() {
            self.pending.push_front(packet);
        }
    }

    /// Restores an unconsumed actual terminal before retiring a partial relay negotiation.
    pub(super) fn replay_current(&mut self) {
        if let Some(bytes) = self.current.take() {
            self.pending.push_front(bytes);
        }
    }

    /// Releases an already consumed packet after its separate handoff owner has been committed.
    pub(super) fn finish_current(&mut self) {
        self.current = None;
    }

    /// Borrows a decoded terminal only when the last actual packet carried it.
    pub(super) fn current_terminal(&self) -> io::Result<Option<crate::session_wire::CoreMessage>> {
        self.current
            .as_ref()
            .map(super::relay::terminal)
            .transpose()
            .map(Option::flatten)
    }

    /// Shares the original header/body and skips only unknown IDs under the existing policy.
    async fn next_known_raw(&mut self) -> io::Result<(GameHeader, Bytes)> {
        loop {
            let bytes = self.next_raw().await?;
            self.current = None;
            let mut reader = wire::Reader::from_shared(&bytes);
            let header =
                wire::PacketHeader::from_raw(reader.read_var_u32().map_err(io::Error::other)?);
            let Some(id) = packets::PacketId::from_raw(header.id) else {
                self.negotiation_skips = self.negotiation_skips.saturating_add(1);
                tracing::debug!(
                    skipped = self.negotiation_skips,
                    id = header.id,
                    "skipped unused native negotiation packet"
                );
                continue;
            };
            let body = bytes.slice(reader.position()..);
            self.current = Some(bytes);
            return Ok((
                GameHeader {
                    id,
                    from_subclient: header.sender_subclient.into(),
                    to_subclient: header.target_subclient.into(),
                },
                body,
            ));
        }
    }

    /// Decodes one handshake packet while preserving every sibling from its batch.
    pub(super) async fn next_packet(&mut self) -> io::Result<Packet> {
        let (header, body) = self.next_known_raw().await?;
        let data = crate::codec::decode_body(header.id, &body)
            .map_err(io::Error::other)
            .inspect_err(|_| self.current = None)?;
        Ok(Packet::from_parts(header, data))
    }

    /// Handles terminals immediately and retains other unhandled input in its original wire form.
    pub(super) fn defer_unhandled(
        &mut self,
        packet: packets::Packet,
    ) -> Result<(), crate::ProtocolError> {
        if matches!(
            packet,
            packets::Packet::Disconnect(_) | packets::Packet::Transfer(_)
        ) {
            if let Some(error) = self.negotiation_failure(packet) {
                return Err(error);
            }
            return Ok(());
        }
        self.defer_current()
    }

    /// Moves the same unread encoded packet into the original aggregate startup bound.
    fn defer_current(&mut self) -> Result<(), crate::ProtocolError> {
        let bytes = self.current.take().ok_or_else(|| {
            super::failure(io::Error::new(
                io::ErrorKind::InvalidData,
                "native deferred packet has no received frame",
            ))
        })?;
        retain_early(&mut self.deferred, bytes)
    }

    /// Transfers bounded early input once, ahead of packets not yet consumed by negotiation.
    pub(super) fn take_deferred(&mut self) -> Vec<crate::raw_packet::RawPacket> {
        self.current = None;
        std::mem::take(&mut self.deferred).into_packets()
    }

    /// Retains the relay's startup prefix in the same bounded owner until first StartGame.
    pub(super) async fn defer_through_start_game(&mut self) -> Result<(), crate::ProtocolError> {
        for raw in self.deferred.as_slice() {
            if raw.id == packets::PacketId::StartGame {
                return Ok(());
            }
            if raw.id == packets::PacketId::PlayStatus {
                check_startup_packet(&raw.clone().decode()?.data)?;
            }
        }
        loop {
            let (header, body) = self.next_known_raw().await.map_err(super::failure)?;
            if matches!(
                header.id,
                packets::PacketId::StartGame
                    | packets::PacketId::Disconnect
                    | packets::PacketId::Transfer
                    | packets::PacketId::PlayStatus
            ) {
                let packet = crate::codec::decode_body(header.id, &body)
                    .map_err(io::Error::other)
                    .inspect_err(|_| self.current = None)
                    .map_err(super::failure)?;
                check_startup_packet(&packet)?;
                self.defer_unhandled(packet)?;
            } else {
                self.defer_current()?;
            }
            if header.id == packets::PacketId::StartGame {
                return Ok(());
            }
        }
    }

    /// Checks ciphertext input before decrypting or allocating decoded state.
    fn check_wire_size(&self, message: &[u8]) -> io::Result<()> {
        if message.len() > self.limits.max_encoded_bytes {
            return Err(io::Error::other(BatchError::EncodedTooLarge {
                actual: message.len(),
                maximum: self.limits.max_encoded_bytes,
            }));
        }
        Ok(())
    }
}

/// Preserves startup refusal and Disconnect causes before waiting for another original packet.
fn check_startup_packet(packet: &packets::Packet) -> Result<(), crate::ProtocolError> {
    if let packets::Packet::Disconnect(disconnect) = packet {
        return Err(super::super::startup::packet_disconnect(disconnect).into());
    }
    if let packets::Packet::PlayStatus(status) = packet
        && let Ok(status_kind) = crate::ui::player_status(status.status)
        && !matches!(
            status_kind,
            crate::ui::PlayerStatus::LoginSuccess | crate::ui::PlayerStatus::PlayerSpawn
        )
    {
        return Err(crate::codec::SessionFailure::ClientHandshake(
            bedrock_client::handshake::HandshakeError::LoginRefused(status.status),
        )
        .into());
    }
    Ok(())
}

/// Restores only the length prefix around original packet bytes under the existing startup bound.
fn retain_early(deferred: &mut DeferredPackets, bytes: Bytes) -> Result<(), crate::ProtocolError> {
    let mut frame = Vec::new();
    wire::Writer::with_limit(&mut frame, MAX_DEFERRED_PACKET_BYTES)
        .write_len_var_u32(bytes.len())
        .map_err(io::Error::other)
        .map_err(super::failure)?;
    let retained = frame.len().saturating_add(bytes.len());
    if retained > MAX_DEFERRED_PACKET_BYTES {
        return Err(crate::codec::SessionFailure::BatchTooLarge {
            actual: retained,
            max: MAX_DEFERRED_PACKET_BYTES,
        }
        .into());
    }
    frame.extend_from_slice(&bytes);
    let mut packets =
        crate::raw_packet::decode_batch_body(Bytes::from(frame), MAX_DEFERRED_PACKET_BYTES)?;
    let packet = packets.pop().expect("one original packet was framed");
    deferred.push(packet)?;
    Ok(())
}

/// Restores the cleartext marker around a directional cipher body when required.
fn envelope(marker: Option<u8>, body: Bytes) -> Bytes {
    if let Some(marker) = marker {
        let mut message = BytesMut::with_capacity(body.len() + 1);
        message.extend_from_slice(&[marker]);
        message.extend_from_slice(&body);
        message.freeze()
    } else {
        body
    }
}

impl PacketConnection for Connection {
    fn observe_rtt(&self) -> Option<crate::RoundTripTime> {
        self.transport.observe_rtt()
    }

    /// Advances the cipher only inside this owner's uninterrupted write operation.
    async fn send_body(&mut self, body: Bytes) -> io::Result<()> {
        self.send_body_until(body, None).await
    }

    /// Reframes a bounded prefix or one larger negotiated incoming packet on the same FIFO.
    async fn recv_body(&mut self) -> io::Result<Bytes> {
        self.fill().await?;
        let mut body = Vec::new();
        let mut accepted = 0;
        while accepted < self.limits.max_packets {
            match self.pending_ready() {
                Ok(true) => {}
                Ok(false) => break,
                // The original malformed front remains pending after this accepted prefix.
                Err(_) if accepted != 0 => break,
                Err(error) => return Err(error),
            }
            let checkpoint = body.len();
            let limit = if accepted == 0 && matches!(self.codec(), Codec::Negotiated(_)) {
                self.incoming_decoded_limit
            } else {
                self.limits.max_decoded_bytes
            };
            let remaining = limit.saturating_sub(checkpoint);
            let packet = self.pending.front().expect("validated original front");
            let result = wire::Writer::with_limit(&mut body, remaining).write_byte_slice(packet);
            if let Err(error) = result {
                body.truncate(checkpoint);
                if accepted == 0 {
                    return Err(io::Error::other(error));
                }
                break;
            }
            drop(self.pending.pop_front());
            accepted += 1;
            if body.len() > self.limits.max_decoded_bytes {
                break;
            }
        }
        Ok(Bytes::from(body))
    }

    /// Keeps protected service progress alive without decoding or consuming game input.
    async fn maintain(&mut self) {
        self.transport.maintain().await;
    }

    /// Ends the actual transport, without starting another connection.
    async fn close(&mut self) -> io::Result<()> {
        self.transport.close().await
    }
}

#[cfg(test)]
mod tests;
