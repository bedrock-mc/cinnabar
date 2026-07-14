use std::path::Path;

use jolyne::error::JolyneError;
use jolyne::raw::RawPacket;
use jolyne::stream::client::ClientHandshakeConfig;
use jolyne::stream::transport::{BedrockTransport, Transport};
use jolyne::stream::{BedrockStream, Client, Handshake, Play};
use valentine::bedrock::version::v1_26_30::McpePacketName;

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
    decode: impl FnOnce(RawPacket) -> Result<Packet, JolyneError>,
) -> Result<Option<WorldEvent>, ProtocolError> {
    if !matches!(
        raw.id,
        McpePacketName::PacketBiomeDefinitionList
            | McpePacketName::PacketLevelChunk
            | McpePacketName::PacketSubchunk
            | McpePacketName::PacketUpdateBlock
            | McpePacketName::PacketUpdateSubchunkBlocks
            | McpePacketName::PacketChunkRadiusUpdate
            | McpePacketName::PacketNetworkChunkPublisherUpdate
            | McpePacketName::PacketChangeDimension
            | McpePacketName::PacketMovePlayer
            | McpePacketName::PacketCorrectPlayerMovePrediction
            | McpePacketName::PacketSetTime
            | McpePacketName::PacketLevelEvent
    ) {
        return Ok(None);
    }
    let packet = decode(raw)?;
    Ok(into_world_event(packet, current_dimension)?)
}

#[cfg(test)]
mod tests {
    use std::cell::Cell;

    use bytes::{Buf, BufMut, BytesMut};
    use jolyne::raw::decode_packet_raw;
    use valentine::bedrock::context::BedrockSession;
    use valentine::bedrock::version::v1_26_30::{
        BiomeDefinition, BiomeDefinitionListPacket, BlockCoordinates,
        CorrectPlayerMovePredictionPacket, LevelEventPacket, LevelEventPacketEvent, McpePacketName,
        MovePlayerPacket, UpdateBlockPacket, Vec2F, Vec3F,
    };
    use valentine::protocol::wire;

    use super::*;
    use crate::WorldEvent;

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

        let event = decode_world_raw_with(raw, 0, |raw| {
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
            })
        );
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
