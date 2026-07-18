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
}

impl<T: Transport> PlaySession<T> {
    fn new(stream: BedrockStream<Play, Client, T>, cache: Option<ClientBlobCache>) -> Self {
        Self {
            stream,
            decode_errors: 0,
            world_skips: 0,
            blob_cache: cache.map(BlobCacheResolver::new),
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
            | McpePacketName::PacketUpdatePlayerGameType
            | McpePacketName::PacketSetDefaultGameType
            | McpePacketName::PacketItemRegistry
            | McpePacketName::PacketMobEquipment
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
    crate::codec::validate_raw_ui_frame(raw.inner_frame())?;
    if raw.id == McpePacketName::PacketMobEquipment
        && let Some(equipment) = decode_empty_mob_equipment(&raw)?
    {
        return Ok(Some(WorldEvent::Equipment(equipment)));
    }
    let packet = decode(raw)?;
    Ok(into_world_event(packet, current_dimension)?)
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
mod tests {
    use std::cell::Cell;

    use super::*;
    use crate::WorldEvent;
    use bytes::{Buf, BufMut, Bytes, BytesMut};
    use jolyne::raw::decode_packet_raw;
    use valentine::bedrock::codec::Nbt;
    use valentine::bedrock::context::BedrockSession;
    use valentine::bedrock::version::v1_26_30::{
        AddEntityPacket, AddPlayerPacket, AnimateEntityPacket, AnimatePacket,
        AnimatePacketActionId, BiomeDefinition, BiomeDefinitionListPacket, BlockCoordinates,
        BlockEntityDataPacket, CorrectPlayerMovePredictionPacket, GameRuleI32, GameRuleI32Type,
        GameRuleI32Value, GameRulesChangedPacket, ItemNew, ItemRegistryPacket, LevelChunkPacket,
        LevelChunkPacketBlobs, LevelEventPacket, LevelEventPacketEvent, McpePacketName,
        MobEquipmentPacket, MovePlayerPacket, TextPacket, TextPacketCategory, TextPacketContent,
        TextPacketContentJson, TextPacketType, UpdateBlockPacket, Vec2F, Vec3F, WindowId,
    };

    fn raw_packet(id: McpePacketName, body: &[u8]) -> jolyne::raw::RawPacket {
        let mut payload = BytesMut::new();
        wire::write_var_u32(&mut payload, id as u32);
        payload.put_slice(body);
        let mut frame = BytesMut::new();
        wire::write_var_u32(&mut frame, payload.len() as u32);
        frame.put_slice(&payload);
        decode_packet_raw(&mut frame.freeze()).expect("raw packet")
    }

    #[test]
    fn transfer_resets_pending_cache_transactions_but_change_dimension_is_ordered() {
        let cache = ClientBlobCache::default();
        let mut resolver = BlobCacheResolver::new(cache);
        let missing = crate::client_blob_hash(b"missing");
        resolver
            .accept_cached_packet(
                LevelChunkPacket {
                    sub_chunk_count: 0,
                    blobs: Some(LevelChunkPacketBlobs {
                        hashes: vec![missing],
                    }),
                    ..Default::default()
                }
                .into(),
            )
            .expect("pending cached column");

        assert!(!reset_cache_for_immediate_boundary(
            &mut resolver,
            McpePacketName::PacketChangeDimension
        ));
        assert_eq!(resolver.stats().pending_transactions, 1);
        assert!(reset_cache_for_immediate_boundary(
            &mut resolver,
            McpePacketName::PacketTransfer
        ));
        assert_eq!(resolver.stats().pending_transactions, 0);
        assert_eq!(resolver.stats().pending_resets, 1);
    }

    #[test]
    fn ignored_play_packet_is_not_materialized() {
        let raw = raw_packet(McpePacketName::PacketNetworkSettings, &[0x7f]);
        let decoder_called = Cell::new(false);

        let event = decode_world_raw_with(raw, 0, |raw| {
            decoder_called.set(true);
            raw.decode(&BedrockSession { shield_item_id: 0 })
        })
        .expect("ignored packet");

        assert!(event.is_none());
        assert!(!decoder_called.get());
    }

    #[test]
    fn allowlisted_ui_packet_is_validated_decoded_and_normalized() {
        let session = BedrockSession { shield_item_id: 0 };
        let packet: Packet = TextPacket {
            category: TextPacketCategory::MessageOnly,
            type_: TextPacketType::Raw,
            content: Some(TextPacketContent::Raw(TextPacketContentJson {
                message: "live UI".to_owned(),
            })),
            ..Default::default()
        }
        .into();
        let mut batch = crate::encode(&packet, &session).expect("encode text");
        batch.advance(1);
        let raw = decode_packet_raw(&mut batch).expect("raw text");
        let decoder_called = Cell::new(false);

        let event = decode_world_raw_with(raw, 0, |raw| {
            decoder_called.set(true);
            raw.decode(&session)
        })
        .expect("decode UI event");

        assert!(decoder_called.get());
        assert!(matches!(
            event,
            Some(WorldEvent::Ui(crate::UiEvent::Text(_)))
        ));
    }

    #[test]
    fn live_ui_path_rejects_invalid_utf8_before_owned_decoder() {
        let raw = raw_packet(McpePacketName::PacketText, &[0, 0, 0, 1, 0xff, 0, 0, 0]);
        let decoder_called = Cell::new(false);

        let error = decode_world_raw_with(raw, 0, |_| {
            decoder_called.set(true);
            panic!("invalid UI bytes must fail before owned decoding")
        })
        .expect_err("invalid UI UTF-8");

        assert!(!decoder_called.get());
        assert!(matches!(
            error,
            ProtocolError::Ui(crate::UiPacketError::InvalidUtf8 {
                field: "text.message"
            })
        ));
    }

    #[test]
    fn live_score_path_checks_text_bound_before_owned_decoder() {
        let mut body = BytesMut::new();
        body.put_u8(0);
        wire::write_var_u32(&mut body, 1);
        wire::write_var_u64(&mut body, 0);
        wire::write_var_u32(&mut body, (crate::MAX_UI_TEXT_BYTES + 1) as u32);
        body.put_bytes(b'x', crate::MAX_UI_TEXT_BYTES + 1);
        let raw = raw_packet(McpePacketName::PacketSetScore, &body);
        let decoder_called = Cell::new(false);

        let error = decode_world_raw_with(raw, 0, |_| {
            decoder_called.set(true);
            panic!("oversized score text must fail before owned decoding")
        })
        .expect_err("oversized score text");

        assert!(!decoder_called.get());
        assert!(matches!(
            error,
            ProtocolError::Ui(crate::UiPacketError::TextTooLong { .. })
        ));
    }

    #[test]
    fn allowlisted_world_packet_is_decoded_and_normalized() {
        let session = BedrockSession { shield_item_id: 0 };
        let packet: Packet = UpdateBlockPacket {
            position: BlockCoordinates { x: 17, y: 2, z: -3 },
            block_runtime_id: 99,
            layer: 0,
            ..Default::default()
        }
        .into();
        let mut batch = crate::encode(&packet, &session).expect("encode update");
        batch.advance(1);
        let raw = decode_packet_raw(&mut batch).expect("raw update");

        let event = decode_world_raw_with(raw, 2, |raw| raw.decode(&session))
            .expect("decode world update")
            .expect("world event");

        let WorldEvent::BlockUpdates(updates) = event else {
            panic!("expected block updates")
        };
        assert_eq!(updates[0].dimension, 2);
        assert_eq!(updates[0].position, [17, 2, -3]);
        assert_eq!(updates[0].network_id, 99);
    }

    #[test]
    fn allowlisted_block_entity_update_preserves_dimension_position_and_exact_nbt() {
        let session = BedrockSession { shield_item_id: 0 };
        let nbt = vec![10, 0, 0];
        let packet: Packet = BlockEntityDataPacket {
            position: BlockCoordinates { x: 17, y: 2, z: -3 },
            nbt: Nbt(Bytes::copy_from_slice(&nbt)),
        }
        .into();
        let mut batch = crate::encode(&packet, &session).expect("encode block entity update");
        batch.advance(1);
        let raw = decode_packet_raw(&mut batch).expect("raw block entity update");

        let event = decode_world_raw_with(raw, 2, |raw| raw.decode(&session))
            .expect("decode block entity update")
            .expect("world event");

        assert_eq!(
            event,
            WorldEvent::BlockEntityUpdate(crate::BlockEntityUpdateEvent {
                dimension: 2,
                position: [17, 2, -3],
                nbt,
            })
        );
    }

    #[test]
    fn allowlisted_weather_packet_is_decoded_and_normalized() {
        let session = BedrockSession { shield_item_id: 0 };
        let packet: Packet = LevelEventPacket {
            event: LevelEventPacketEvent::StartRain,
            data: 48_000,
            ..Default::default()
        }
        .into();
        let mut batch = crate::encode(&packet, &session).expect("encode weather event");
        batch.advance(1);
        let raw = decode_packet_raw(&mut batch).expect("raw weather event");
        let decoder_called = Cell::new(false);

        let event = decode_world_raw_with(raw, 0, |raw| {
            decoder_called.set(true);
            raw.decode(&session)
        })
        .expect("decode weather event");

        assert!(decoder_called.get());
        assert_eq!(
            event,
            Some(WorldEvent::Weather(crate::WeatherUpdateEvent {
                channel: crate::WeatherChannel::Rain,
                level: 1.0,
            }))
        );
    }

    #[test]
    fn allowlisted_daylight_cycle_rule_is_decoded_and_normalized() {
        let session = BedrockSession { shield_item_id: 0 };
        let packet: Packet = GameRulesChangedPacket {
            rules: vec![GameRuleI32 {
                name: "DoDaylightCycle".to_owned(),
                type_: GameRuleI32Type::Bool,
                value: Some(GameRuleI32Value::Bool(false)),
                ..Default::default()
            }],
        }
        .into();
        let mut batch = crate::encode(&packet, &session).expect("encode gamerule event");
        batch.advance(1);
        let raw = decode_packet_raw(&mut batch).expect("raw gamerule event");
        let decoder_called = Cell::new(false);

        let event = decode_world_raw_with(raw, 0, |raw| {
            decoder_called.set(true);
            raw.decode(&session)
        })
        .expect("decode gamerule event");

        assert!(decoder_called.get());
        assert_eq!(
            event,
            Some(WorldEvent::DaylightCycle(crate::DaylightCycleUpdateEvent {
                enabled: false
            }))
        );
    }

    #[test]
    fn allowlisted_move_player_is_materialized_and_normalized() {
        let session = BedrockSession { shield_item_id: 0 };
        let packet: Packet = MovePlayerPacket {
            runtime_id: 42,
            position: Vec3F {
                x: 1.25,
                y: 70.5,
                z: -8.75,
            },
            pitch: 15.0,
            yaw: -120.25,
            ..Default::default()
        }
        .into();
        let mut batch = crate::encode(&packet, &session).expect("encode move player");
        batch.advance(1);
        let raw = decode_packet_raw(&mut batch).expect("raw move player");
        let decoder_called = Cell::new(false);

        let event = decode_world_raw_with(raw, 0, |raw| {
            decoder_called.set(true);
            raw.decode(&session)
        })
        .expect("decode move player")
        .expect("move player event");

        assert!(decoder_called.get());
        assert_eq!(
            event,
            WorldEvent::MovePlayer(crate::MovePlayerEvent {
                runtime_id: 42,
                position: [1.25, 70.5, -8.75],
                pitch: 15.0,
                yaw: -120.25,
                head_yaw: 0.0,
                mode: crate::MovePlayerMode::Normal,
                on_ground: false,
                teleported: false,
                source_tick: 0,
            })
        );
    }

    #[test]
    fn allowlisted_actor_packet_is_materialized_and_normalized() {
        let session = BedrockSession { shield_item_id: 0 };
        let packet: Packet = AddEntityPacket {
            unique_id: 9,
            runtime_id: 42,
            entity_type: "minecraft:bee".to_owned(),
            ..Default::default()
        }
        .into();
        let mut batch = crate::encode(&packet, &session).expect("encode add entity");
        batch.advance(1);
        let raw = decode_packet_raw(&mut batch).expect("raw add entity");

        let event =
            decode_world_raw_with(raw, 2, |raw| raw.decode(&session)).expect("decode actor event");

        assert!(matches!(
            event,
            Some(WorldEvent::Actor(crate::ActorEvent::Spawn(spawn)))
                if spawn.dimension == 2 && spawn.runtime_id == 42
        ));
    }

    #[test]
    fn allowlisted_item_and_action_packets_are_materialized_and_normalized() {
        let session = BedrockSession { shield_item_id: 0 };
        let packets: [Packet; 5] = [
            AddPlayerPacket {
                runtime_id: 42,
                ..Default::default()
            }
            .into(),
            ItemRegistryPacket::default().into(),
            MobEquipmentPacket {
                runtime_entity_id: 42,
                item: ItemNew {
                    network_id: 5,
                    count: 1,
                    ..Default::default()
                },
                slot: 0,
                selected_slot: 0,
                window_id: WindowId::Inventory,
            }
            .into(),
            AnimatePacket {
                runtime_entity_id: 42,
                action_id: AnimatePacketActionId::SwingArm,
                ..Default::default()
            }
            .into(),
            AnimateEntityPacket {
                animation: "animation.test.attack".into(),
                controller: "controller.animation.test".into(),
                runtime_entity_ids: vec![42],
                ..Default::default()
            }
            .into(),
        ];

        for packet in packets {
            let mut batch = crate::encode(&packet, &session).expect("encode item/action packet");
            batch.advance(1);
            let raw = decode_packet_raw(&mut batch).expect("raw item/action packet");
            let event = decode_world_raw_with(raw, 0, |raw| raw.decode(&session))
                .expect("decode item/action event");
            assert!(event.is_some());
        }
    }

    #[test]
    fn canonical_empty_mob_equipment_is_materialized_and_normalized() {
        let mut body = BytesMut::new();
        wire::write_var_u64(&mut body, 42);
        body.put_i16_le(0);
        body.put_u16_le(0);
        wire::write_var_u32(&mut body, 0);
        body.put_u8(0);
        wire::write_var_u32(&mut body, 0);
        wire::write_var_u32(&mut body, 0);
        body.put_u8(0);
        body.put_u8(0);
        body.put_i8(0);
        let raw = raw_packet(McpePacketName::PacketMobEquipment, &body);

        let event = decode_world_raw_with(raw, 0, |raw| {
            raw.decode(&BedrockSession { shield_item_id: 0 })
        })
        .expect("decode empty equipment")
        .expect("empty equipment event");

        assert!(matches!(
            event,
            WorldEvent::Equipment(crate::EquipmentEvent { stack, .. }) if stack == crate::NetworkItemStack::empty()
        ));
    }

    fn raw_nonempty_mob_equipment(extra: &[u8]) -> RawPacket {
        let mut body = BytesMut::new();
        wire::write_var_u64(&mut body, 42);
        body.put_i16_le(5);
        body.put_u16_le(1);
        wire::write_var_u32(&mut body, 0);
        body.put_u8(0);
        wire::write_var_u32(&mut body, 0);
        wire::write_var_u32(&mut body, extra.len() as u32);
        body.put_slice(extra);
        body.put_u8(0);
        body.put_u8(0);
        body.put_i8(0);
        raw_packet(McpePacketName::PacketMobEquipment, &body)
    }

    fn raw_zero_count_mob_equipment() -> RawPacket {
        // A non-air network id (5) paired with a zero stack count is not a valid
        // item: it is neither the empty stack nor a real one.
        let mut body = BytesMut::new();
        wire::write_var_u64(&mut body, 42);
        body.put_i16_le(5);
        body.put_u16_le(0);
        wire::write_var_u32(&mut body, 0);
        body.put_u8(0);
        wire::write_var_u32(&mut body, 0);
        let extra = [0u8; 10]; // no NBT, no can-place-on/can-destroy entries
        wire::write_var_u32(&mut body, extra.len() as u32);
        body.put_slice(&extra);
        body.put_u8(0);
        body.put_u8(0);
        body.put_i8(0);
        raw_packet(McpePacketName::PacketMobEquipment, &body)
    }

    #[test]
    fn valid_equipment_is_retained_and_invalid_items_are_rejected() {
        // Byte-exact wire canonicalization was intentionally removed: the client
        // only renders retained items, so faithful round-tripping is unnecessary
        // and rejected legitimate servers. Semantic validation is what guards
        // retention now.
        let session = BedrockSession { shield_item_id: 0 };
        let valid_extra = [0; 10];
        let valid = decode_world_raw_with(raw_nonempty_mob_equipment(&valid_extra), 0, |raw| {
            raw.decode(&session)
        })
        .expect("valid equipment wire");
        assert!(matches!(valid, Some(WorldEvent::Equipment(_))));

        let error = decode_world_raw_with(raw_zero_count_mob_equipment(), 0, |raw| {
            raw.decode(&session)
        })
        .expect_err("zero-count item is semantically invalid");
        assert!(
            matches!(
                &error,
                ProtocolError::World(crate::WorldPacketError::Item(
                    crate::ItemPacketError::InvalidItemCount
                ))
            ),
            "unexpected error: {error:?}"
        );
    }

    #[test]
    fn absolute_actor_move_uses_bedrock_varuint_and_raw_byte_rotations() {
        let runtime_id = (u64::from(u32::MAX) + 42) << 1;
        let mut body = BytesMut::new();
        wire::write_var_u64(&mut body, runtime_id);
        body.put_u8(0b11);
        body.put_f32_le(12.5);
        body.put_f32_le(64.25);
        body.put_f32_le(-7.75);
        body.put_u8(32);
        body.put_u8(64);
        body.put_u8(128);
        let raw = raw_packet(McpePacketName::PacketMoveEntity, &body);
        let decoder_called = Cell::new(false);

        let event = decode_world_raw_with(raw, 2, |_| {
            decoder_called.set(true);
            panic!("absolute actor movement must bypass Valentine's incompatible Rotation shape")
        })
        .expect("decode absolute actor move")
        .expect("absolute actor move event");

        assert!(!decoder_called.get());
        assert_eq!(
            event,
            WorldEvent::Actor(crate::ActorEvent::Move(crate::ActorMoveEvent {
                dimension: 2,
                runtime_id,
                position: [Some(12.5), Some(64.25), Some(-7.75)],
                position_origin: crate::ActorPositionOrigin::NetworkOffset,
                pitch: Some(45.0),
                yaw: Some(90.0),
                head_yaw: Some(180.0),
                on_ground: Some(true),
                teleported: true,
                snap: true,
                player_mode: None,
                source_tick: None,
            }))
        );
    }

    #[test]
    fn absolute_actor_move_rejects_truncated_and_trailing_bodies() {
        let mut valid = BytesMut::new();
        wire::write_var_u64(&mut valid, 42);
        valid.put_u8(0);
        valid.put_f32_le(1.0);
        valid.put_f32_le(2.0);
        valid.put_f32_le(3.0);
        valid.put_slice(&[0, 0, 0]);

        for (body, actual) in [
            (&valid[..valid.len() - 1], 15_usize),
            (&[valid.as_ref(), &[0xff]].concat(), 17_usize),
        ] {
            let raw = raw_packet(McpePacketName::PacketMoveEntity, body);
            let error = decode_world_raw_with(raw, 0, |_| {
                panic!("malformed absolute actor movement must bypass Valentine")
            })
            .expect_err("malformed absolute actor move");
            assert!(matches!(
                error,
                ProtocolError::World(crate::WorldPacketError::Actor(
                    crate::ActorPacketError::InvalidAbsoluteMoveLength {
                        actual: found,
                        expected: 16,
                    }
                )) if found == actual
            ));
        }
    }

    #[test]
    fn allowlisted_movement_correction_is_materialized_and_normalized() {
        let session = BedrockSession { shield_item_id: 0 };
        let packet: Packet = CorrectPlayerMovePredictionPacket {
            position: Vec3F {
                x: 27.5,
                y: 111.0,
                z: 91.5,
            },
            delta: Vec3F {
                x: 0.5,
                y: -0.25,
                z: 1.0,
            },
            rotation: Vec2F { x: -15.0, z: 90.25 },
            on_ground: true,
            tick: 55,
            ..Default::default()
        }
        .into();
        let mut batch = crate::encode(&packet, &session).expect("encode movement correction");
        batch.advance(1);
        let raw = decode_packet_raw(&mut batch).expect("raw movement correction");
        let decoder_called = Cell::new(false);

        let event = decode_world_raw_with(raw, 0, |raw| {
            decoder_called.set(true);
            raw.decode(&session)
        })
        .expect("decode movement correction")
        .expect("movement correction event");

        assert!(decoder_called.get());
        assert_eq!(
            event,
            WorldEvent::PlayerMovementCorrection(crate::PlayerMovementCorrectionEvent {
                position: [27.5, 111.0, 91.5],
                delta: [0.5, -0.25, 1.0],
                pitch: -15.0,
                yaw: 90.25,
                on_ground: true,
                tick: 55,
            })
        );
    }

    #[test]
    fn allowlisted_biome_definitions_are_materialized_and_normalized() {
        let session = BedrockSession { shield_item_id: 0 };
        let packet: Packet = BiomeDefinitionListPacket {
            biome_definitions: vec![BiomeDefinition {
                name_index: 0,
                biome_id: u16::MAX,
                temperature: 0.8,
                downfall: 0.4,
                snow_foliage: 0.0,
                map_water_colour: 0xff44_6688_u32 as i32,
                ..Default::default()
            }],
            string_list: vec!["plains".into()],
        }
        .into();
        let mut batch = crate::encode(&packet, &session).expect("encode biome definitions");
        batch.advance(1);
        let raw = decode_packet_raw(&mut batch).expect("raw biome definitions");
        let decoder_called = Cell::new(false);

        let event = decode_world_raw_with(raw, 0, |raw| {
            decoder_called.set(true);
            raw.decode(&session)
        })
        .expect("decode biome definitions");

        assert!(decoder_called.get());
        let WorldEvent::BiomeDefinitions(event) = event.expect("biome definitions event") else {
            panic!("expected biome definitions")
        };
        assert_eq!(event.definitions[0].biome_id, None);
        assert_eq!(event.definitions[0].name.as_ref(), "minecraft:plains");
    }
}
