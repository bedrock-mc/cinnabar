use std::path::Path;

use bytes::Buf;
use jolyne::error::JolyneError;
use jolyne::raw::{RawPacket, decode_packet_raw};
use jolyne::stream::client::ClientHandshakeConfig;
use jolyne::stream::transport::{BedrockTransport, Transport};
use jolyne::stream::{BedrockStream, Client, Handshake, Play};
use valentine::bedrock::{
    codec::BedrockCodec,
    context::BedrockSession,
    version::v1_26_30::{McpePacketName, WindowId},
};
use valentine::protocol::wire;

use crate::socket_transport::SocketTransport;
use crate::{GameData, Packet, ProtocolError, WorldEvent, into_world_event};

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

    /// Generic transport seam used by deterministic protocol state tests.
    #[doc(hidden)]
    pub async fn connect_transport<T: Transport>(
        transport: T,
        display_name: &str,
    ) -> Result<(PlaySession<T>, GameData), ProtocolError> {
        let peer_addr = transport.peer_addr();
        let mut transport = BedrockTransport::new(transport);
        transport.set_max_decompressed_batch_size(Some(MAX_DECOMPRESSED_BATCH_SIZE));
        let stream: BedrockStream<Handshake, Client, T> = BedrockStream::from_transport(transport);
        let config = ClientHandshakeConfig::random(peer_addr, display_name);
        let (stream, game_data) = stream.join(config).await?;
        Ok((PlaySession::new(stream), game_data))
    }
}

/// An authenticated, spawned Bedrock session.
pub struct PlaySession<T: Transport = SocketTransport> {
    stream: BedrockStream<Play, Client, T>,
    decode_errors: u64,
}

impl<T: Transport> PlaySession<T> {
    fn new(stream: BedrockStream<Play, Client, T>) -> Self {
        Self {
            stream,
            decode_errors: 0,
        }
    }

    /// Receives one packet, counting malformed/decompression failures.
    pub async fn recv(&mut self) -> Result<Packet, ProtocolError> {
        match self.stream.recv_packet().await {
            Ok(packet) => Ok(packet),
            Err(error) => {
                if is_decode_error(&error) {
                    self.decode_errors = self.decode_errors.saturating_add(1);
                }
                Err(error.into())
            }
        }
    }

    /// Receives the next world-streaming event without decoding unrelated play packets.
    pub async fn recv_world_event(
        &mut self,
        current_dimension: i32,
    ) -> Result<WorldEvent, ProtocolError> {
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
            let shield_item_id = self.stream.packet_args().shield_item_id;
            let decoded = decode_world_raw_with(raw, current_dimension, shield_item_id, |raw| {
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
                Err(error) => return Err(error),
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
    shield_item_id: i32,
    decode: impl FnOnce(RawPacket) -> Result<Packet, JolyneError>,
) -> Result<Option<WorldEvent>, ProtocolError> {
    if !matches!(
        raw.id,
        McpePacketName::PacketBiomeDefinitionList
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
    if raw.id == McpePacketName::PacketMobEquipment
        && let Some(equipment) = decode_empty_mob_equipment(&raw)?
    {
        return Ok(Some(WorldEvent::Equipment(equipment)));
    }
    let raw_body = raw.body().clone();
    let packet = decode(raw)?;
    if matches!(
        &packet.data,
        valentine::bedrock::version::v1_26_30::McpePacketData::PacketAddPlayer(_)
            | valentine::bedrock::version::v1_26_30::McpePacketData::PacketMobEquipment(_)
    ) {
        validate_canonical_item_wire(&raw_body, &packet, shield_item_id)?;
    }
    Ok(into_world_event(packet, current_dimension)?)
}

fn validate_canonical_item_wire(
    raw_body: &bytes::Bytes,
    packet: &Packet,
    shield_item_id: i32,
) -> Result<(), ProtocolError> {
    let mut encoded = crate::encode(packet, &BedrockSession { shield_item_id })?;
    encoded.advance(1);
    let canonical = decode_packet_raw(&mut encoded)?;
    if canonical.body() != raw_body {
        return Err(ProtocolError::World(crate::world::WorldPacketError::Item(
            crate::ItemPacketError::NonCanonicalItemWire,
        )));
    }
    Ok(())
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
    use valentine::bedrock::version::v1_26_30::{
        AddEntityPacket, AddPlayerPacket, AnimateEntityPacket, AnimatePacket,
        AnimatePacketActionId, BiomeDefinition, BiomeDefinitionListPacket, BlockCoordinates,
        BlockEntityDataPacket, CorrectPlayerMovePredictionPacket, GameRuleI32, GameRuleI32Type,
        GameRuleI32Value, GameRulesChangedPacket, ItemNew, ItemRegistryPacket, LevelEventPacket,
        LevelEventPacketEvent, McpePacketName, MobEquipmentPacket, MovePlayerPacket,
        UpdateBlockPacket, Vec2F, Vec3F, WindowId,
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
    fn ignored_play_packet_is_not_materialized() {
        let raw = raw_packet(McpePacketName::PacketText, &[0, 0, 0, 0x7f]);
        let decoder_called = Cell::new(false);

        let event = decode_world_raw_with(raw, 0, 0, |raw| {
            decoder_called.set(true);
            raw.decode(&BedrockSession { shield_item_id: 0 })
        })
        .expect("ignored packet");

        assert!(event.is_none());
        assert!(!decoder_called.get());
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

        let event = decode_world_raw_with(raw, 2, 0, |raw| raw.decode(&session))
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

        let event = decode_world_raw_with(raw, 2, 0, |raw| raw.decode(&session))
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

        let event = decode_world_raw_with(raw, 0, 0, |raw| {
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

        let event = decode_world_raw_with(raw, 0, 0, |raw| {
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

        let event = decode_world_raw_with(raw, 0, 0, |raw| {
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

        let event = decode_world_raw_with(raw, 2, 0, |raw| raw.decode(&session))
            .expect("decode actor event");

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
            let event = decode_world_raw_with(raw, 0, 0, |raw| raw.decode(&session))
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

        let event = decode_world_raw_with(raw, 0, 0, |raw| {
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

    #[test]
    fn item_wire_bytes_are_exact_or_rejected_before_retention() {
        let session = BedrockSession { shield_item_id: 0 };
        let valid_extra = [0; 10];
        let valid = decode_world_raw_with(raw_nonempty_mob_equipment(&valid_extra), 0, 0, |raw| {
            raw.decode(&session)
        })
        .expect("canonical equipment wire");
        assert!(matches!(valid, Some(WorldEvent::Equipment(_))));

        let mut trailing = valid_extra.to_vec();
        trailing.push(0xff);
        let invalid_utf8 = [
            0, 0, // no NBT
            1, 0, 0, 0, // one can-place-on entry
            1, 0,    // one-byte ShortString
            0xff, // invalid UTF-8
            0, 0, 0, 0, // no can-destroy entries
        ];
        for extra in [&trailing[..], &invalid_utf8[..]] {
            let error = decode_world_raw_with(raw_nonempty_mob_equipment(extra), 0, 0, |raw| {
                raw.decode(&session)
            })
            .expect_err("noncanonical item wire");
            assert!(
                matches!(
                    &error,
                    ProtocolError::World(crate::WorldPacketError::Item(
                        crate::ItemPacketError::NonCanonicalItemWire
                    )) | ProtocolError::Session(JolyneError::PacketTrailingBytes { .. })
                ),
                "unexpected error: {error:?}"
            );
        }
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

        let event = decode_world_raw_with(raw, 2, 0, |_| {
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
            let error = decode_world_raw_with(raw, 0, 0, |_| {
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

        let event = decode_world_raw_with(raw, 0, 0, |raw| {
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

        let event = decode_world_raw_with(raw, 0, 0, |raw| {
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
