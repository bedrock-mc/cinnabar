use std::path::Path;

use bytes::Buf;
use jolyne::error::JolyneError;
use jolyne::raw::RawPacket;
use jolyne::stream::client::ClientHandshakeConfig;
use jolyne::stream::transport::{BedrockTransport, Transport};
use jolyne::stream::{BedrockStream, Client, Handshake, Play};
use valentine::bedrock::{
    codec::BedrockCodec,
    version::v1_26_30::{McpePacketData, McpePacketName, WindowId},
};
use valentine::protocol::wire;

use crate::socket_transport::SocketTransport;
use crate::{
    BlobCacheReady, BlobCacheResolver, BlobCacheStats, ClientBlobCache, GameData, Packet,
    ProtocolError, WorldEvent, into_world_event,
};

const MAX_DECOMPRESSED_BATCH_SIZE: usize = 16 * 1024 * 1024;

/// Entry point for the offline local-core login sequence.
pub struct LoginSequence;

impl LoginSequence {
    /// Connects to the Go core and completes the encrypted Bedrock spawn sequence.
    pub async fn connect(
        socket_dir: &Path,
        display_name: &str,
    ) -> Result<(PlaySession, GameData), ProtocolError> {
        let transport = SocketTransport::connect(socket_dir)
            .await
            .map_err(ProtocolError::Bridge)?;
        Self::connect_transport(transport, display_name).await
    }

    /// Connects with a persistent verified cache and a fresh session-owned resolver.
    pub async fn connect_with_blob_cache(
        socket_dir: &Path,
        display_name: &str,
        cache: ClientBlobCache,
    ) -> Result<(PlaySession, GameData), ProtocolError> {
        let transport = SocketTransport::connect(socket_dir)
            .await
            .map_err(ProtocolError::Bridge)?;
        Self::connect_transport_with_blob_cache(transport, display_name, cache).await
    }

    /// Generic transport seam used by deterministic protocol state tests.
    #[doc(hidden)]
    pub async fn connect_transport<T: Transport>(
        transport: T,
        display_name: &str,
    ) -> Result<(PlaySession<T>, GameData), ProtocolError> {
        Self::connect_transport_inner(transport, display_name, None).await
    }

    /// Deterministic enabled negotiation seam used by protocol tests and live integration.
    #[doc(hidden)]
    pub async fn connect_transport_with_blob_cache<T: Transport>(
        transport: T,
        display_name: &str,
        cache: ClientBlobCache,
    ) -> Result<(PlaySession<T>, GameData), ProtocolError> {
        Self::connect_transport_inner(transport, display_name, Some(cache)).await
    }

    async fn connect_transport_inner<T: Transport>(
        transport: T,
        display_name: &str,
        cache: Option<ClientBlobCache>,
    ) -> Result<(PlaySession<T>, GameData), ProtocolError> {
        let peer_addr = transport.peer_addr();
        let mut transport = BedrockTransport::new(transport);
        transport.set_max_decompressed_batch_size(Some(MAX_DECOMPRESSED_BATCH_SIZE));
        let stream: BedrockStream<Handshake, Client, T> = BedrockStream::from_transport(transport);
        let config = ClientHandshakeConfig::random(peer_addr, display_name)
            .with_client_cache_enabled(cache.is_some());
        let (stream, game_data) = stream.join(config).await?;
        Ok((PlaySession::new(stream, cache), game_data))
    }
}

/// An authenticated, spawned Bedrock session.
pub struct PlaySession<T: Transport = SocketTransport> {
    stream: BedrockStream<Play, Client, T>,
    decode_errors: u64,
    world_skips: u64,
    blob_cache: Option<BlobCacheResolver>,
    packet_id_trace: PacketIdTraceState,
}

const MAX_PACKET_ID_TRACE_ENTRIES: usize = 256;
const PACKET_ID_TRACE_DURATION: std::time::Duration = std::time::Duration::from_secs(30);

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PacketIdTraceSnapshot {
    pub packet_ids: Box<[u32]>,
    pub overflow: u64,
    pub timed_out: bool,
}

#[derive(Default)]
struct PacketIdTraceState {
    started_at: Option<std::time::Instant>,
    packet_ids: Vec<u32>,
    recorded: usize,
    overflow: u64,
    timed_out: bool,
}

impl PacketIdTraceState {
    fn begin(&mut self) {
        self.started_at = Some(std::time::Instant::now());
        self.packet_ids.clear();
        self.recorded = 0;
        self.overflow = 0;
        self.timed_out = false;
    }

    fn observe(&mut self, packet: McpePacketName) {
        let Some(started_at) = self.started_at else {
            return;
        };
        if started_at.elapsed() >= PACKET_ID_TRACE_DURATION {
            self.started_at = None;
            self.timed_out = true;
            return;
        }
        if self.recorded < MAX_PACKET_ID_TRACE_ENTRIES {
            self.packet_ids.push(packet as u32);
            self.recorded += 1;
        } else {
            self.overflow = self.overflow.saturating_add(1);
        }
    }

    fn cancel(&mut self) {
        *self = Self::default();
    }

    fn drain(&mut self) -> Option<PacketIdTraceSnapshot> {
        if self.packet_ids.is_empty() && !self.timed_out {
            return None;
        }
        let overflow = if self.timed_out {
            std::mem::take(&mut self.overflow)
        } else {
            0
        };
        Some(PacketIdTraceSnapshot {
            packet_ids: std::mem::take(&mut self.packet_ids).into_boxed_slice(),
            overflow,
            timed_out: std::mem::take(&mut self.timed_out),
        })
    }
}

impl<T: Transport> PlaySession<T> {
    fn new(stream: BedrockStream<Play, Client, T>, cache: Option<ClientBlobCache>) -> Self {
        Self {
            stream,
            decode_errors: 0,
            world_skips: 0,
            blob_cache: cache.map(BlobCacheResolver::new),
            packet_id_trace: PacketIdTraceState::default(),
        }
    }

    /// Skips a well-formed but semantically unusable world packet instead of
    /// tearing down the session, counting it for observability. Genuine wire
    /// decode/transport errors stay fatal and are returned unchanged.
    fn skip_or_fail_world(&mut self, error: ProtocolError) -> Result<(), ProtocolError> {
        if matches!(error, ProtocolError::World(_)) {
            self.world_skips = self.world_skips.saturating_add(1);
            self.reset_blob_cache_pending();
            Ok(())
        } else {
            Err(error)
        }
    }

    /// Count of world packets skipped because normalization rejected them.
    pub fn world_skip_count(&self) -> u64 {
        self.world_skips
    }

    /// Receives one packet, counting malformed/decompression failures.
    pub async fn recv(&mut self) -> Result<Packet, ProtocolError> {
        match self.stream.recv_packet().await {
            Ok(packet) => {
                if matches!(
                    &packet.data,
                    McpePacketData::PacketTransfer(_) | McpePacketData::PacketDisconnect(_)
                ) {
                    self.reset_blob_cache_pending();
                }
                Ok(packet)
            }
            Err(error) => {
                if is_decode_error(&error) {
                    self.decode_errors = self.decode_errors.saturating_add(1);
                }
                self.reset_blob_cache_pending();
                Err(error.into())
            }
        }
    }

    /// Receives the next world-streaming event without decoding unrelated play packets.
    pub async fn recv_world_event(
        &mut self,
        current_dimension: i32,
    ) -> Result<WorldEvent, ProtocolError> {
        if self.blob_cache.is_some() {
            return self
                .recv_world_event_with_blob_cache(current_dimension)
                .await;
        }
        loop {
            let raw = match self.stream.recv_packet_raw().await {
                Ok(raw) => raw,
                Err(error) => {
                    if is_decode_error(&error) {
                        self.decode_errors = self.decode_errors.saturating_add(1);
                    }
                    return Err(error.into());
                }
            };
            self.packet_id_trace.observe(raw.id);
            let decoded = decode_world_raw_with(raw, current_dimension, |raw| {
                self.stream.decode_raw_packet(raw)
            });
            match decoded {
                Ok(Some(event)) => return Ok(event),
                Ok(None) => {}
                Err(ProtocolError::Session(error)) => {
                    if is_decode_error(&error) {
                        self.decode_errors = self.decode_errors.saturating_add(1);
                    }
                    return Err(ProtocolError::Session(error));
                }
                Err(error) => self.skip_or_fail_world(error)?,
            }
        }
    }

    /// Sends one packet through the encrypted play session.
    pub async fn send(&mut self, packet: Packet) -> Result<(), ProtocolError> {
        crate::codec::validate_packet(&packet)?;
        self.stream.send_packet(packet).await?;
        Ok(())
    }

    /// Starts a bounded, secret-safe packet-ID trace for native acceptance.
    pub fn begin_packet_id_trace(&mut self) {
        self.packet_id_trace.begin();
    }

    /// Cancels an armed trace when the triggering packet was not sent.
    pub fn cancel_packet_id_trace(&mut self) {
        self.packet_id_trace.cancel();
    }

    /// Drains packet IDs observed since the last drain without packet payloads.
    pub fn drain_packet_id_trace(&mut self) -> Option<PacketIdTraceSnapshot> {
        self.packet_id_trace.drain()
    }

    /// Number of receive-side decode/decompression failures observed in play.
    pub fn decode_error_count(&self) -> u64 {
        self.decode_errors
    }

    /// Whether login advertised cache support and this session owns a resolver.
    #[must_use]
    pub const fn blob_cache_enabled(&self) -> bool {
        self.blob_cache.is_some()
    }

    /// Secret-safe cache counters for acceptance evidence.
    #[must_use]
    pub fn blob_cache_stats(&self) -> BlobCacheStats {
        self.blob_cache
            .as_ref()
            .map_or_else(BlobCacheStats::default, BlobCacheResolver::stats)
    }

    /// Drops only session-scoped transactions; verified cache entries remain shared.
    pub fn reset_blob_cache_pending(&mut self) {
        if let Some(resolver) = self.blob_cache.as_mut() {
            resolver.reset_pending();
        }
    }

    /// Arms a one-shot selective transaction rotation for the next raw
    /// LevelChunk/SubChunk candidate. Verified blobs and ready work survive.
    pub fn rotate_blob_cache_pending_for_fast_transfer(&mut self) {
        if let Some(resolver) = self.blob_cache.as_mut() {
            resolver.arm_fast_transfer_rotation();
        }
    }

    async fn recv_world_event_with_blob_cache(
        &mut self,
        current_dimension: i32,
    ) -> Result<WorldEvent, ProtocolError> {
        loop {
            if let Some(ready) = self
                .blob_cache
                .as_mut()
                .expect("enabled path owns a resolver")
                .pop_ready()
            {
                let event = match ready {
                    BlobCacheReady::Packet(packet) => {
                        match into_world_event(packet, current_dimension) {
                            Ok(Some(event)) => event,
                            Ok(None) => {
                                self.reset_blob_cache_pending();
                                continue;
                            }
                            Err(error) => {
                                self.skip_or_fail_world(error.into())?;
                                continue;
                            }
                        }
                    }
                    BlobCacheReady::WorldEvent(event) => event,
                };
                if matches!(event, WorldEvent::ChangeDimension(_)) {
                    self.reset_blob_cache_pending();
                }
                return Ok(event);
            }

            let raw = match self.stream.recv_packet_raw().await {
                Ok(raw) => raw,
                Err(error) => return Err(self.fail_session(error)),
            };
            self.packet_id_trace.observe(raw.id);
            let packet_bytes = raw.inner_frame().len();
            let packet_name = raw.id;

            if matches!(
                packet_name,
                McpePacketName::PacketTransfer | McpePacketName::PacketDisconnect
            ) {
                if let Err(error) = self.stream.decode_raw_packet(raw) {
                    return Err(self.fail_session(error));
                }
                let resolver = self
                    .blob_cache
                    .as_mut()
                    .expect("enabled path owns a resolver");
                reset_cache_for_immediate_boundary(resolver, packet_name);
                continue;
            }

            if matches!(
                packet_name,
                McpePacketName::PacketLevelChunk
                    | McpePacketName::PacketSubchunk
                    | McpePacketName::PacketClientCacheMissResponse
            ) {
                let packet = match self.stream.decode_raw_packet(raw) {
                    Ok(packet) => packet,
                    Err(error) => return Err(self.fail_session(error)),
                };
                rotate_blob_cache_for_decoded_candidate(
                    self.blob_cache
                        .as_mut()
                        .expect("enabled path owns a resolver"),
                    &packet,
                )?;
                if let McpePacketData::PacketClientCacheMissResponse(response) = packet.data {
                    if let Err(error) = self
                        .blob_cache
                        .as_mut()
                        .expect("enabled path owns a resolver")
                        .accept_miss_response(response)
                    {
                        return Err(error.into());
                    }
                    continue;
                }

                if is_cached_world_packet(&packet) {
                    let status = match self
                        .blob_cache
                        .as_mut()
                        .expect("enabled path owns a resolver")
                        .accept_cached_packet_with_size(packet, packet_bytes)
                    {
                        Ok(status) => status,
                        Err(error) => return Err(error.into()),
                    };
                    if let Err(error) = self.send(status.into()).await {
                        self.reset_blob_cache_pending();
                        return Err(error);
                    }
                    continue;
                }

                let event = match into_world_event(packet, current_dimension) {
                    Ok(event) => event,
                    Err(error) => {
                        self.skip_or_fail_world(error.into())?;
                        continue;
                    }
                };
                if let Some(event) = event
                    && let Err(error) = self
                        .blob_cache
                        .as_mut()
                        .expect("enabled path owns a resolver")
                        .accept_world_event(event, packet_bytes)
                {
                    return Err(error.into());
                }
                continue;
            }

            let decoded = decode_world_raw_with(raw, current_dimension, |raw| {
                self.stream.decode_raw_packet(raw)
            });
            match decoded {
                Ok(Some(event)) => {
                    if let Err(error) = self
                        .blob_cache
                        .as_mut()
                        .expect("enabled path owns a resolver")
                        .accept_world_event(event, packet_bytes)
                    {
                        return Err(error.into());
                    }
                }
                Ok(None) => {}
                Err(ProtocolError::Session(error)) => return Err(self.fail_session(error)),
                Err(error) => self.skip_or_fail_world(error)?,
            }
        }
    }

    fn fail_session(&mut self, error: JolyneError) -> ProtocolError {
        if is_decode_error(&error) {
            self.decode_errors = self.decode_errors.saturating_add(1);
        }
        self.reset_blob_cache_pending();
        ProtocolError::Session(error)
    }
}

fn rotate_blob_cache_for_decoded_candidate(
    resolver: &mut BlobCacheResolver,
    packet: &Packet,
) -> Result<bool, crate::BlobCacheError> {
    if matches!(
        &packet.data,
        McpePacketData::PacketLevelChunk(_) | McpePacketData::PacketSubchunk(_)
    ) {
        resolver.rotate_pending_for_fast_transfer_candidate()
    } else {
        Ok(false)
    }
}

fn is_cached_world_packet(packet: &Packet) -> bool {
    match &packet.data {
        McpePacketData::PacketLevelChunk(packet) => packet.blobs.is_some(),
        McpePacketData::PacketSubchunk(packet) => matches!(
            packet.entries,
            valentine::bedrock::version::v1_26_30::SubchunkPacketEntries::SubChunkEntryWithCaching(
                _
            )
        ),
        _ => false,
    }
}

fn reset_cache_for_immediate_boundary(
    resolver: &mut BlobCacheResolver,
    packet: McpePacketName,
) -> bool {
    if matches!(
        packet,
        McpePacketName::PacketTransfer | McpePacketName::PacketDisconnect
    ) {
        resolver.reset_pending();
        true
    } else {
        false
    }
}

fn is_decode_error(error: &JolyneError) -> bool {
    matches!(
        error,
        JolyneError::Decode(_)
            | JolyneError::PacketDecode { .. }
            | JolyneError::PacketTrailingBytes { .. }
            | JolyneError::Io(_)
            | JolyneError::Protocol(_)
    )
}

fn decode_world_raw_with(
    raw: RawPacket,
    current_dimension: i32,
    decode: impl FnOnce(RawPacket) -> Result<Packet, JolyneError>,
) -> Result<Option<WorldEvent>, ProtocolError> {
    if !matches!(
        raw.id,
        McpePacketName::PacketText
            | McpePacketName::PacketCommandOutput
            | McpePacketName::PacketPlayStatus
            | McpePacketName::PacketSetHealth
            | McpePacketName::PacketBossEvent
            | McpePacketName::PacketSetTitle
            | McpePacketName::PacketModalFormRequest
            | McpePacketName::PacketRemoveObjective
            | McpePacketName::PacketSetDisplayObjective
            | McpePacketName::PacketSetScore
            | McpePacketName::PacketToastRequest
            | McpePacketName::PacketUpdateSoftEnum
            | McpePacketName::PacketBiomeDefinitionList
            | McpePacketName::PacketAddPlayer
            | McpePacketName::PacketAddEntity
            | McpePacketName::PacketRemoveEntity
            | McpePacketName::PacketMoveEntity
            | McpePacketName::PacketMoveEntityDelta
            | McpePacketName::PacketSetEntityData
            | McpePacketName::PacketUpdateAttributes
            | McpePacketName::PacketPlayerList
            | McpePacketName::PacketItemRegistry
            | McpePacketName::PacketMobEquipment
            | McpePacketName::PacketMobArmorEquipment
            | McpePacketName::PacketMobEffect
            | McpePacketName::PacketSetEntityLink
            | McpePacketName::PacketSetPlayerGameType
            | McpePacketName::PacketInventoryContent
            | McpePacketName::PacketInventorySlot
            | McpePacketName::PacketPlayerHotbar
            | McpePacketName::PacketItemStackResponse
            | McpePacketName::PacketContainerOpen
            | McpePacketName::PacketContainerClose
            | McpePacketName::PacketContainerSetData
            | McpePacketName::PacketAnimate
            | McpePacketName::PacketAnimateEntity
            | McpePacketName::PacketLevelChunk
            | McpePacketName::PacketSubchunk
            | McpePacketName::PacketUpdateBlock
            | McpePacketName::PacketUpdateSubchunkBlocks
            | McpePacketName::PacketBlockEntityData
            | McpePacketName::PacketChunkRadiusUpdate
            | McpePacketName::PacketNetworkChunkPublisherUpdate
            | McpePacketName::PacketChangeDimension
            | McpePacketName::PacketMovePlayer
            | McpePacketName::PacketCorrectPlayerMovePrediction
            | McpePacketName::PacketSetTime
            | McpePacketName::PacketGameRulesChanged
            | McpePacketName::PacketLevelEvent
    ) {
        return Ok(None);
    }
    if raw.id == McpePacketName::PacketMoveEntity {
        return Ok(Some(WorldEvent::Actor(
            crate::actor::normalize_move_entity_body(raw.body(), current_dimension)
                .map_err(crate::world::WorldPacketError::from)?,
        )));
    }
    crate::inventory::validate_raw_inventory_packet(&raw)
        .map_err(crate::world::WorldPacketError::from)?;
    crate::codec::validate_raw_ui_frame(raw.inner_frame()).map_err(demote_ui_semantic_rejection)?;
    if raw.id == McpePacketName::PacketMobEquipment
        && let Some(equipment) = decode_empty_mob_equipment(&raw)?
    {
        return Ok(Some(WorldEvent::Equipment(equipment)));
    }
    let packet = decode(raw)?;
    Ok(into_world_event(packet, current_dimension)?)
}

/// Reclassifies the raw UI pre-validator's semantic rejections as skippable
/// world packets so a well-formed-but-odd UI packet (unknown text/score/soft-enum
/// discriminant, over-budget text/score/autocomplete counts) is skipped and
/// counted rather than tearing down the session. Genuine wire failures the same
/// validator can raise -- truncated varints, negative lengths, trailing bytes --
/// stay fatal, matching the sibling inventory pre-validator above.
fn demote_ui_semantic_rejection(error: ProtocolError) -> ProtocolError {
    match error {
        ProtocolError::Ui(ui) => ProtocolError::World(crate::world::WorldPacketError::Ui(ui)),
        other => other,
    }
}

fn decode_empty_mob_equipment(
    raw: &RawPacket,
) -> Result<Option<crate::EquipmentEvent>, ProtocolError> {
    let malformed = || {
        ProtocolError::World(crate::world::WorldPacketError::Item(
            crate::ItemPacketError::ItemEncodingFailed,
        ))
    };
    let contradictory = || {
        ProtocolError::World(crate::world::WorldPacketError::Item(
            crate::ItemPacketError::ContradictoryStackId,
        ))
    };
    let mut body = raw.body().clone();
    let actor_runtime_id = wire::read_var_u64(&mut body).map_err(|_| malformed())?;
    if body.remaining() < 2 {
        return Err(malformed());
    }
    let network_id = body.get_i16_le();
    if network_id != 0 {
        return Ok(None);
    }
    if body.remaining() < 3 {
        return Err(malformed());
    }
    let count = body.get_u16_le();
    let metadata = wire::read_var_u32(&mut body).map_err(|_| malformed())?;
    if !body.has_remaining() {
        return Err(malformed());
    }
    let has_stack_id = body.get_u8();
    if has_stack_id != 0 {
        return Err(contradictory());
    }
    let block_runtime_id = wire::read_var_u32(&mut body).map_err(|_| malformed())?;
    let extra_len = wire::read_var_u32(&mut body).map_err(|_| malformed())?;
    if count != 0 || metadata != 0 || block_runtime_id != 0 || extra_len != 0 {
        return Err(contradictory());
    }
    if body.remaining() < 3 {
        return Err(malformed());
    }
    let inventory_slot = body.get_u8();
    let selected_slot = body.get_u8();
    let window = WindowId::decode(&mut body, ())?;
    if body.has_remaining() {
        return Err(ProtocolError::TrailingPacketBytes {
            remaining: body.remaining(),
        });
    }
    Ok(Some(
        crate::item::normalize_empty_equipment(
            actor_runtime_id,
            inventory_slot,
            selected_slot,
            window,
        )
        .map_err(|error| ProtocolError::World(crate::world::WorldPacketError::Item(error)))?,
    ))
}

#[cfg(test)]
mod tests;
