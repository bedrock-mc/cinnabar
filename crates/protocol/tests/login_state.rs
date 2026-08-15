use std::collections::VecDeque;
use std::io;
use std::net::{IpAddr, Ipv4Addr, SocketAddr};
use std::pin::Pin;
use std::sync::{Arc, Mutex};
use std::task::{Context, Poll};

use aes::Aes256;
use base64::Engine;
use base64::engine::general_purpose::STANDARD;
use bytes::{Bytes, BytesMut};
use ctr::cipher::{KeyIvInit, StreamCipher};
use flate2::Compression;
use flate2::write::DeflateEncoder;
use jolyne::batch::decode_batch;
use jolyne::stream::transport::{Transport, TransportMessage, TransportRecvMessage};
use jolyne::valentine::{
    ActorRuntimeId, ChunkPos, ChunkRadiusUpdatedPacket, ClientCacheBlobStatusPacket,
    ClientCacheMissResponsePacket, ClientCacheStatusPacket, ClientToServerHandshakePacket,
    DimensionType, ItemData, ItemRegistryPacket, LevelChunkPacket,
    LevelChunkPacketPayloadSubChunkMetadata, McpePacket, McpePacketData, McpePacketName,
    MissingBlobData, NetworkSettingsPacket, NetworkSettingsPacketCompressionAlgorithm,
    PackInstanceId, PlayStatusPacket, PlayStatusPacketStatus, RequestChunkRadiusPacket,
    RequestNetworkSettingsPacket, ResourcePackClientResponsePacketResponse,
    ResourcePackStackPacket, ResourcePacksInfoPacket, ServerToClientHandshakePacket,
    ServerboundLoadingScreenPacket, ServerboundLoadingScreenPacketLoadingScreenPacketType,
    SetLocalPlayerAsInitializedPacket, SetTimePacket, StartGamePacket,
};
use jsonwebtoken::{Algorithm, EncodingKey, Header};
use p384::pkcs8::{DecodePublicKey, EncodePrivateKey, EncodePublicKey};
use p384::{PublicKey, SecretKey};
use protocol::{BedrockSession, ClientBlobCache, LoginSequence, Packet, ProtocolError, WorldEvent};
use serde::Serialize;
use sha2::{Digest, Sha256};
use valentine::bedrock::codec::BedrockCodec;
use valentine::protocol::wire;

type Aes256Ctr = ctr::Ctr32BE<Aes256>;

#[path = "login_state/level_chunk_wire_failure.rs"]
mod level_chunk_wire_failure;

const RUNTIME_ID: u64 = 0x1234_5678;
const OTHER_RUNTIME_ID: u64 = 0x7654_3210;
const MAX_DECOMPRESSED: usize = 16 * 1024 * 1024;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum CompressionMode {
    Deflate,
    Snappy,
    None,
}

impl CompressionMode {
    fn marker(self) -> u8 {
        match self {
            Self::Deflate => 0,
            Self::Snappy => 1,
            Self::None => 0xff,
        }
    }

    fn network_value(self) -> NetworkSettingsPacketCompressionAlgorithm {
        match self {
            // gophertunnel calls the zlib/deflate compressor `CompressionAlgorithmFlate`;
            // the 1.26.40 generated enum spells the same wire value `ZLib`.
            Self::Deflate => NetworkSettingsPacketCompressionAlgorithm::ZLib,
            Self::Snappy => NetworkSettingsPacketCompressionAlgorithm::Snappy,
            Self::None => NetworkSettingsPacketCompressionAlgorithm::Unknown(u16::MAX),
        }
    }
}

#[derive(Clone, Copy)]
enum SpawnOrder {
    RadiusThenSpawn,
    SpawnThenRadius,
}

#[derive(Clone, Copy)]
enum CachePlayScript {
    ResolveValid,
    TruncatedMissResponse,
    InvalidMissResponseThenTraffic,
    MalformedLevelChunk,
    TrailingLevelChunk,
}

struct ScriptTransport {
    script: Arc<Mutex<ServerScript>>,
}

impl ScriptTransport {
    fn new(mode: CompressionMode, order: SpawnOrder, conflicting_start: bool) -> Self {
        Self::new_with_options(
            mode,
            order,
            conflicting_start,
            false,
            false,
            CachePlayScript::ResolveValid,
        )
    }

    fn new_with_pack_stack(
        mode: CompressionMode,
        order: SpawnOrder,
        conflicting_start: bool,
        non_empty_pack_stack: bool,
    ) -> Self {
        Self::new_with_options(
            mode,
            order,
            conflicting_start,
            non_empty_pack_stack,
            false,
            CachePlayScript::ResolveValid,
        )
    }

    fn new_with_cache(mode: CompressionMode, order: SpawnOrder) -> Self {
        Self::new_with_cache_script(mode, order, CachePlayScript::ResolveValid)
    }

    fn new_with_cache_script(
        mode: CompressionMode,
        order: SpawnOrder,
        cache_play_script: CachePlayScript,
    ) -> Self {
        Self::new_with_options(mode, order, false, false, true, cache_play_script)
    }

    fn new_with_options(
        mode: CompressionMode,
        order: SpawnOrder,
        conflicting_start: bool,
        non_empty_pack_stack: bool,
        cache_enabled: bool,
        cache_play_script: CachePlayScript,
    ) -> Self {
        Self {
            script: Arc::new(Mutex::new(ServerScript::new(
                mode,
                order,
                conflicting_start,
                non_empty_pack_stack,
                cache_enabled,
                cache_play_script,
            ))),
        }
    }
}

impl Transport for ScriptTransport {
    type Error = io::Error;

    const USES_BATCH_PREFIX: bool = true;

    fn poll_send(
        self: Pin<&mut Self>,
        _cx: &mut Context<'_>,
        message: TransportMessage,
    ) -> Poll<Result<(), Self::Error>> {
        assert!(message.reliable, "login traffic must use reliable delivery");
        self.script
            .lock()
            .expect("script lock")
            .on_client_frame(message.buffer);
        Poll::Ready(Ok(()))
    }

    fn poll_recv(
        self: Pin<&mut Self>,
        _cx: &mut Context<'_>,
    ) -> Poll<Option<Result<TransportRecvMessage, Self::Error>>> {
        let next = self.script.lock().expect("script lock").inbound.pop_front();
        match next {
            Some(bytes) => Poll::Ready(Some(Ok(TransportRecvMessage::Contiguous(bytes)))),
            None => Poll::Pending,
        }
    }

    fn peer_addr(&self) -> SocketAddr {
        SocketAddr::new(IpAddr::V4(Ipv4Addr::LOCALHOST), 0)
    }
}

struct ServerScript {
    mode: CompressionMode,
    order: SpawnOrder,
    conflicting_start: bool,
    non_empty_pack_stack: bool,
    cache_enabled: bool,
    cache_play_script: CachePlayScript,
    stage: u8,
    inbound: VecDeque<Bytes>,
    crypto: Option<ScriptCrypto>,
}

impl ServerScript {
    fn new(
        mode: CompressionMode,
        order: SpawnOrder,
        conflicting_start: bool,
        non_empty_pack_stack: bool,
        cache_enabled: bool,
        cache_play_script: CachePlayScript,
    ) -> Self {
        Self {
            mode,
            order,
            conflicting_start,
            non_empty_pack_stack,
            cache_enabled,
            cache_play_script,
            stage: 0,
            inbound: VecDeque::new(),
            crypto: None,
        }
    }

    fn on_client_frame(&mut self, frame: Bytes) {
        match self.stage {
            0 => {
                let packets = decode_clear(frame, false);
                assert!(matches!(
                    packets.as_slice(),
                    [McpePacket {
                        data: McpePacketData::RequestNetworkSettingsPacket(
                            RequestNetworkSettingsPacket { .. }
                        ),
                        ..
                    }]
                ));
                self.enqueue_clear(
                    &[McpePacket::from(NetworkSettingsPacket {
                        compression_threshold: 0,
                        compression_algorithm: self.mode.network_value(),
                        ..Default::default()
                    })],
                    false,
                );
                self.stage = 1;
            }
            1 => {
                let packets = decode_clear(frame, true);
                let login = match packets.as_slice() {
                    [
                        McpePacket {
                            data: McpePacketData::LoginPacket(login),
                            ..
                        },
                    ] => login,
                    other => panic!("expected Login, got {other:?}"),
                };
                let client_public_key =
                    login_public_key(&identity_chain(&login.connection_request));
                let (handshake, crypto) = server_handshake(client_public_key);
                self.crypto = Some(crypto);
                self.enqueue_clear(&[McpePacket::from(handshake)], true);
                self.stage = 2;
            }
            2 => {
                let clear = self.crypto.as_mut().expect("crypto").decrypt_client(frame);
                assert_eq!(
                    clear.get(1).copied(),
                    Some(self.mode.marker()),
                    "encrypted acknowledgement must use the negotiated compressor"
                );
                let packets = decode_clear(clear, true);
                assert!(matches!(
                    packets.as_slice(),
                    [McpePacket {
                        data: McpePacketData::ClientToServerHandshakePacket(
                            ClientToServerHandshakePacket {}
                        ),
                        ..
                    }]
                ));
                self.enqueue_encrypted(&[
                    McpePacket::from(PlayStatusPacket {
                        status: PlayStatusPacketStatus::LoginSuccess,
                    }),
                    McpePacket::from(ResourcePacksInfoPacket::default()),
                ]);
                self.stage = 3;
            }
            3 => {
                let packets = self.decode_encrypted_client(frame);
                assert!(matches!(
                    packets.as_slice(),
                    [McpePacket {
                        data: McpePacketData::ClientCacheStatusPacket(ClientCacheStatusPacket {
                            iscachesupported
                        }),
                        ..
                    }] if *iscachesupported == self.cache_enabled
                ));
                self.stage = 4;
            }
            4 => {
                let packets = self.decode_encrypted_client(frame);
                assert!(matches!(
                    packets.as_slice(),
                    [McpePacket {
                        // 1.26.40 turns the flat `response_status` enum into a
                        // payload-carrying union whose body repeats the response
                        // name as a string; gophertunnel writes the same string
                        // from `resourcePackResponseToString`
                        // (packet/resource_pack_client_response.go).
                        // `HaveAllPacks` is `PackResponseAllPacksDownloaded`,
                        // i.e. `DownloadingFinished` here.
                        data: McpePacketData::ResourcePackClientResponsePacket(response),
                        ..
                    }] if matches!(
                        &response.response,
                        ResourcePackClientResponsePacketResponse::DownloadingFinished(payload)
                            if payload.response_type == "downloadingfinished"
                    )
                ));
                // `resource_packs` is `texture_pack_list`, whose entries are
                // `PackInstanceId { pack_id, version, sub_pack_name }`; the
                // display name the 1001 model carried is not on the wire.
                let texture_pack_list = self
                    .non_empty_pack_stack
                    .then(|| PackInstanceId {
                        pack_id: "pack-id".into(),
                        version: "1.0.0".into(),
                        sub_pack_name: "test pack".into(),
                    })
                    .into_iter()
                    .collect();
                self.enqueue_encrypted(&[McpePacket::from(ResourcePackStackPacket {
                    texture_pack_list,
                    ..Default::default()
                })]);
                self.stage = 5;
            }
            5 => {
                let packets = self.decode_encrypted_client(frame);
                assert!(matches!(
                    packets.as_slice(),
                    [McpePacket {
                        // `Completed` is gophertunnel's `PackResponseCompleted`,
                        // spelled `ResourcePackStackFinished` in 1.26.40.
                        data: McpePacketData::ResourcePackClientResponsePacket(response),
                        ..
                    }] if matches!(
                        &response.response,
                        ResourcePackClientResponsePacketResponse::ResourcePackStackFinished(payload)
                            if payload.response_type == "resourcepackstackfinished"
                    )
                ));
                if self.conflicting_start {
                    self.enqueue_encrypted(&[start_game(RUNTIME_ID), start_game(OTHER_RUNTIME_ID)]);
                } else {
                    self.enqueue_encrypted(&[
                        start_game(RUNTIME_ID),
                        McpePacket::from(SetTimePacket { time: 12_345 }),
                        McpePacket::from(SetTimePacket { time: 23_456 }),
                    ]);
                }
                self.stage = 6;
            }
            6 => {
                let packets = self.decode_encrypted_client(frame);
                assert!(matches!(
                    packets.as_slice(),
                    [
                        McpePacket {
                            // The raw `type_` int is a named enum in 1.26.40;
                            // 1 was StartLoadingScreen.
                            data: McpePacketData::ServerboundLoadingScreenPacket(
                                ServerboundLoadingScreenPacket {
                                    loading_screen_packet_type:
                                        ServerboundLoadingScreenPacketLoadingScreenPacketType::StartLoadingScreen,
                                    ..
                                }
                            ),
                            ..
                        },
                        McpePacket {
                            data: McpePacketData::RequestChunkRadiusPacket(
                                RequestChunkRadiusPacket {
                                    chunk_radius: 16,
                                    max_chunk_radius: 16,
                                }
                            ),
                            ..
                        }
                    ]
                ));
                let radius = McpePacket::from(ChunkRadiusUpdatedPacket { chunk_radius: 16 });
                let spawn = McpePacket::from(PlayStatusPacket {
                    status: PlayStatusPacketStatus::PlayerSpawn,
                });
                match self.order {
                    SpawnOrder::RadiusThenSpawn => {
                        self.enqueue_encrypted(&[item_registry(), radius, spawn])
                    }
                    SpawnOrder::SpawnThenRadius => {
                        self.enqueue_encrypted(&[item_registry(), spawn, radius])
                    }
                }
                self.stage = 7;
            }
            7 => {
                let packets = self.decode_encrypted_client(frame);
                assert!(matches!(
                    packets.as_slice(),
                    [
                        McpePacket {
                            data: McpePacketData::ServerboundLoadingScreenPacket(
                                ServerboundLoadingScreenPacket {
                                    loading_screen_packet_type:
                                        ServerboundLoadingScreenPacketLoadingScreenPacketType::EndLoadingScreen,
                                    ..
                                }
                            ),
                            ..
                        },
                        McpePacket {
                            // `runtime_entity_id` is the `player_id:
                            // ActorRuntimeId` wrapper in 1.26.40.
                            data: McpePacketData::SetLocalPlayerAsInitializedPacket(
                                SetLocalPlayerAsInitializedPacket {
                                    player_id: ActorRuntimeId {
                                        actor_runtime_id: RUNTIME_ID
                                    }
                                }
                            ),
                            ..
                        }
                    ]
                ));
                if self.cache_enabled
                    && matches!(
                        self.cache_play_script,
                        CachePlayScript::MalformedLevelChunk | CachePlayScript::TrailingLevelChunk
                    )
                {
                    let hash = protocol::client_blob_hash(b"pending-before-wire-failure");
                    self.enqueue_encrypted(&[
                        McpePacket::from(cached_level_chunk(6, -7, vec![hash], b"pending")),
                        McpePacket::from(SetTimePacket { time: 45_678 }),
                    ]);
                }
                if matches!(self.cache_play_script, CachePlayScript::MalformedLevelChunk) {
                    self.enqueue_encrypted_raw_packet(McpePacketName::LevelChunkPacket, &[0xff]);
                } else if matches!(self.cache_play_script, CachePlayScript::TrailingLevelChunk) {
                    let mut body = BytesMut::new();
                    LevelChunkPacket::default()
                        .encode(&mut body)
                        .expect("encode trailing LevelChunk body");
                    body.extend_from_slice(&[0xaa]);
                    self.enqueue_encrypted_raw_packet(McpePacketName::LevelChunkPacket, &body);
                }
                if self.cache_enabled
                    && matches!(
                        self.cache_play_script,
                        CachePlayScript::MalformedLevelChunk | CachePlayScript::TrailingLevelChunk
                    )
                {
                    self.enqueue_encrypted(&[McpePacket::from(LevelChunkPacket {
                        chunk_position: ChunkPos { x: 7, z: -8 },
                        dimension_id: DimensionType { value: 0 },
                        subchunks_count: 0,
                        serialized_chunk_data: vec![0x4d; 4096],
                        ..Default::default()
                    })]);
                } else if !matches!(
                    self.cache_play_script,
                    CachePlayScript::MalformedLevelChunk | CachePlayScript::TrailingLevelChunk
                ) && self.cache_enabled
                {
                    match self.cache_play_script {
                        CachePlayScript::ResolveValid => {
                            let payload = b"cached-column";
                            let hash = protocol::client_blob_hash(payload);
                            self.enqueue_encrypted(&[
                                McpePacket::from(LevelChunkPacket {
                                    chunk_position: ChunkPos { x: 8, z: -10 },
                                    dimension_id: DimensionType { value: 0 },
                                    subchunks_count: 0,
                                    serialized_chunk_data: vec![0x6b; 1024 * 1024],
                                    ..Default::default()
                                }),
                                McpePacket::from(cached_level_chunk(9, -11, vec![hash], b"tail")),
                                McpePacket::from(SetTimePacket { time: 34_567 }),
                            ]);
                        }
                        CachePlayScript::TruncatedMissResponse => {
                            self.enqueue_encrypted_raw_packet(
                                McpePacketName::ClientCacheMissResponsePacket,
                                &[0x01],
                            );
                        }
                        CachePlayScript::InvalidMissResponseThenTraffic => {
                            let hash = protocol::client_blob_hash(b"semantic-response-wanted");
                            self.enqueue_encrypted(&[McpePacket::from(cached_level_chunk(
                                31,
                                -47,
                                vec![hash],
                                b"",
                            ))]);
                        }
                        CachePlayScript::MalformedLevelChunk
                        | CachePlayScript::TrailingLevelChunk => unreachable!(),
                    }
                } else {
                    // A malformed world packet (invalid sub-chunk count) must be
                    // skipped, not disconnect the session; the following SetTime
                    // still arrives in order. A negative count is no longer a
                    // request-mode sentinel in 1.26.40, so it is simply invalid.
                    self.enqueue_encrypted(&[
                        McpePacket::from(LevelChunkPacket {
                            subchunks_count: u32::MAX,
                            ..Default::default()
                        }),
                        McpePacket::from(LevelChunkPacket {
                            chunk_position: ChunkPos { x: 7, z: -9 },
                            dimension_id: DimensionType { value: 0 },
                            subchunks_count: 0,
                            serialized_chunk_data: vec![0x5a; 1024 * 1024],
                            ..Default::default()
                        }),
                        McpePacket::from(SetTimePacket { time: 34_567 }),
                    ]);
                }
                self.stage = 8;
            }
            8 => {
                let packets = self.decode_encrypted_client(frame);
                if self.cache_enabled {
                    let expected_hash = match self.cache_play_script {
                        CachePlayScript::ResolveValid => {
                            protocol::client_blob_hash(b"cached-column")
                        }
                        CachePlayScript::InvalidMissResponseThenTraffic => {
                            protocol::client_blob_hash(b"semantic-response-wanted")
                        }
                        CachePlayScript::TruncatedMissResponse => {
                            panic!("truncated response script sends no cached request")
                        }
                        CachePlayScript::MalformedLevelChunk
                        | CachePlayScript::TrailingLevelChunk => return,
                    };
                    assert!(matches!(
                        packets.as_slice(),
                        [McpePacket {
                            data: McpePacketData::ClientCacheBlobStatusPacket(
                                ClientCacheBlobStatusPacket {
                                    missing_ids,
                                    found_ids
                                }
                            ),
                            ..
                        }] if missing_ids == &[expected_hash] && found_ids.is_empty()
                    ));
                    let response_payload = match self.cache_play_script {
                        CachePlayScript::ResolveValid => b"cached-column".as_slice(),
                        CachePlayScript::InvalidMissResponseThenTraffic => {
                            b"semantic-response-poison".as_slice()
                        }
                        CachePlayScript::TruncatedMissResponse => unreachable!(),
                        CachePlayScript::MalformedLevelChunk
                        | CachePlayScript::TrailingLevelChunk => unreachable!(),
                    };
                    let mut response = vec![McpePacket::from(ClientCacheMissResponsePacket {
                        missing_blobs: vec![MissingBlobData {
                            blob_id: expected_hash,
                            blob_data: response_payload.to_vec(),
                        }],
                    })];
                    if matches!(
                        self.cache_play_script,
                        CachePlayScript::InvalidMissResponseThenTraffic
                    ) {
                        response.push(McpePacket::from(SetTimePacket { time: 45_678 }));
                    }
                    self.enqueue_encrypted(&response);
                    self.stage = 9;
                    return;
                }
                assert!(matches!(
                    packets.as_slice(),
                    [McpePacket {
                        data: McpePacketData::ClientCacheStatusPacket(ClientCacheStatusPacket {
                            iscachesupported: true
                        }),
                        ..
                    }]
                ));
                let malformed = Bytes::from_static(&[0xfe, 0x7f]);
                let encrypted = self
                    .crypto
                    .as_mut()
                    .expect("crypto")
                    .encrypt_server(malformed);
                self.inbound.push_back(encrypted);
                self.stage = 9;
            }
            other => panic!("unexpected client frame in server stage {other}"),
        }
    }

    fn enqueue_clear(&mut self, packets: &[McpePacket], compressed: bool) {
        self.inbound.push_back(encode_server_batch(
            packets,
            compressed.then_some(self.mode),
        ));
    }

    fn enqueue_encrypted(&mut self, packets: &[McpePacket]) {
        let clear = encode_server_batch(packets, Some(self.mode));
        let encrypted = self.crypto.as_mut().expect("crypto").encrypt_server(clear);
        self.inbound.push_back(encrypted);
    }

    fn enqueue_encrypted_raw_packet(&mut self, id: McpePacketName, body: &[u8]) {
        let clear = encode_server_raw_packet(id, body, self.mode);
        let encrypted = self.crypto.as_mut().expect("crypto").encrypt_server(clear);
        self.inbound.push_back(encrypted);
    }

    fn decode_encrypted_client(&mut self, frame: Bytes) -> Vec<McpePacket> {
        let clear = self.crypto.as_mut().expect("crypto").decrypt_client(frame);
        decode_clear(clear, true)
    }
}

struct ScriptCrypto {
    key: [u8; 32],
    decrypt_client: Aes256Ctr,
    encrypt_server: Aes256Ctr,
    client_counter: u64,
    server_counter: u64,
}

impl ScriptCrypto {
    fn new(key: [u8; 32]) -> Self {
        let mut iv = [0u8; 16];
        iv[..12].copy_from_slice(&key[..12]);
        iv[15] = 2;
        Self {
            key,
            decrypt_client: Aes256Ctr::new_from_slices(&key, &iv).expect("fixed key and IV"),
            encrypt_server: Aes256Ctr::new_from_slices(&key, &iv).expect("fixed key and IV"),
            client_counter: 0,
            server_counter: 0,
        }
    }

    fn decrypt_client(&mut self, frame: Bytes) -> Bytes {
        let mut frame = BytesMut::from(frame.as_ref());
        assert_eq!(frame.first().copied(), Some(0xfe));
        self.decrypt_client.apply_keystream(&mut frame[1..]);
        assert!(frame.len() >= 9);
        let checksum_at = frame.len() - 8;
        let expected = checksum(self.client_counter, &frame[1..checksum_at], &self.key);
        assert_eq!(&frame[checksum_at..], &expected);
        self.client_counter += 1;
        frame.truncate(checksum_at);
        frame.freeze()
    }

    fn encrypt_server(&mut self, frame: Bytes) -> Bytes {
        let mut frame = BytesMut::from(frame.as_ref());
        assert_eq!(frame.first().copied(), Some(0xfe));
        let sum = checksum(self.server_counter, &frame[1..], &self.key);
        self.server_counter += 1;
        frame.extend_from_slice(&sum);
        self.encrypt_server.apply_keystream(&mut frame[1..]);
        frame.freeze()
    }
}

fn checksum(counter: u64, data: &[u8], key: &[u8; 32]) -> [u8; 8] {
    let mut digest = Sha256::new();
    digest.update(counter.to_le_bytes());
    digest.update(data);
    digest.update(key);
    let digest = digest.finalize();
    digest[..8].try_into().expect("eight bytes")
}

fn decode_clear(mut frame: Bytes, compressed: bool) -> Vec<McpePacket> {
    decode_batch(
        &mut frame,
        &BedrockSession { shield_item_id: 0 },
        compressed,
        Some(MAX_DECOMPRESSED),
    )
    .expect("decode client batch")
}

fn encode_server_batch(packets: &[McpePacket], mode: Option<CompressionMode>) -> Bytes {
    let mut payload = BytesMut::new();
    for packet in packets {
        packet
            .data
            .encode_inner_bytes_mut(
                &mut payload,
                packet.header.from_subclient,
                packet.header.to_subclient,
            )
            .expect("encode server packet");
    }
    encode_server_payload(&payload, mode)
}

fn encode_server_raw_packet(id: McpePacketName, body: &[u8], mode: CompressionMode) -> Bytes {
    let mut packet = BytesMut::new();
    wire::write_var_u32(&mut packet, id as u32);
    packet.extend_from_slice(body);
    let mut payload = BytesMut::new();
    wire::write_var_u32(&mut payload, packet.len() as u32);
    payload.extend_from_slice(&packet);
    encode_server_payload(&payload, Some(mode))
}

fn encode_server_payload(payload: &[u8], mode: Option<CompressionMode>) -> Bytes {
    let mut frame = BytesMut::from(&b"\xfe"[..]);
    match mode {
        None => frame.extend_from_slice(payload),
        Some(CompressionMode::Deflate) => {
            use std::io::Write;
            let mut encoder = DeflateEncoder::new(Vec::new(), Compression::new(6));
            encoder.write_all(payload).expect("deflate payload");
            frame.extend_from_slice(&[0]);
            frame.extend_from_slice(&encoder.finish().expect("finish deflate"));
        }
        Some(CompressionMode::Snappy) => {
            frame.extend_from_slice(&[1]);
            frame.extend_from_slice(
                &snap::raw::Encoder::new()
                    .compress_vec(payload)
                    .expect("snappy payload"),
            );
        }
        Some(CompressionMode::None) => {
            frame.extend_from_slice(&[0xff]);
            frame.extend_from_slice(payload);
        }
    }
    frame.freeze()
}

/// Extracts the identity chain from a Login connection request.
///
/// Protocol 1001 modelled the request as two `LittleString`s and exposed them
/// as `login.tokens.identity` / `.client`. 1.26.40 types the whole request as
/// one opaque byte slice - gophertunnel does the same
/// (`packet/login.go` writes `io.ByteSlice(&pk.ConnectionRequest)` and
/// `minecraft/protocol/login/request.go` splits it) - so the framing is decoded
/// here: a little-endian `u32` length and then that many bytes, twice.
fn identity_chain(connection_request: &[u8]) -> String {
    let (length, rest) = connection_request
        .split_first_chunk::<4>()
        .expect("connection request carries an identity chain length");
    let length = u32::from_le_bytes(*length) as usize;
    let chain = rest
        .get(..length)
        .expect("identity chain length must fit the connection request");
    String::from_utf8(chain.to_vec()).expect("identity chain is UTF-8 JSON")
}

fn login_public_key(chain_json: &str) -> PublicKey {
    let value: serde_json::Value = serde_json::from_str(chain_json).expect("login chain JSON");
    let token = value["chain"][0].as_str().expect("self-signed token");
    let header = jsonwebtoken::decode_header(token).expect("JWT header");
    let der = STANDARD.decode(header.x5u.expect("x5u")).expect("x5u DER");
    PublicKey::from_public_key_der(&der).expect("P-384 client key")
}

#[derive(Serialize)]
struct HandshakeClaims {
    salt: String,
}

fn server_handshake(client_public_key: PublicKey) -> (ServerToClientHandshakePacket, ScriptCrypto) {
    let mut scalar = [0u8; 48];
    scalar[47] = 7;
    let server_key = SecretKey::from_slice(&scalar).expect("deterministic server key");
    let salt = [0x5au8; 16];
    let shared = p384::ecdh::diffie_hellman(
        server_key.to_nonzero_scalar(),
        client_public_key.as_affine(),
    );
    let mut digest = Sha256::new();
    digest.update(salt);
    digest.update(shared.raw_secret_bytes());
    let key: [u8; 32] = digest.finalize().into();

    let public_der = server_key
        .public_key()
        .to_public_key_der()
        .expect("server public key DER");
    let private_der = server_key.to_pkcs8_der().expect("server private key DER");
    let mut header = Header::new(Algorithm::ES384);
    header.x5u = Some(STANDARD.encode(public_der.as_bytes()));
    let token = jsonwebtoken::encode(
        &header,
        &HandshakeClaims {
            salt: STANDARD.encode(salt),
        },
        &EncodingKey::from_ec_der(private_der.as_bytes()),
    )
    .expect("server handshake JWT");
    (
        ServerToClientHandshakePacket {
            handshake_web_token: token,
        },
        ScriptCrypto::new(key),
    )
}

fn start_game(runtime_entity_id: u64) -> McpePacket {
    McpePacket::from(StartGamePacket {
        runtime_id: ActorRuntimeId {
            actor_runtime_id: runtime_entity_id,
        },
        ..Default::default()
    })
}

/// Builds a cache-enabled LevelChunk for one column referencing `hashes`.
///
/// 1.26.40 writes the blob hashes unconditionally and states cache
/// participation with `cache_enabled`, so the protocol-1001 `blobs: Some(..)`
/// literal has no equivalent. gophertunnel `packet/level_chunk.go` expects
/// `SubChunkCount + 1` hashes, and the `-1` request-mode sentinel is gone.
fn cached_level_chunk(x: i32, z: i32, hashes: Vec<u64>, tail: &[u8]) -> LevelChunkPacket {
    let subchunks_count = u32::try_from(hashes.len().saturating_sub(1)).expect("fixture count");
    LevelChunkPacket {
        chunk_position: ChunkPos { x, z },
        dimension_id: DimensionType { value: 0 },
        subchunks_count,
        cache_enabled: true,
        cache_metadata: hashes
            .into_iter()
            .map(|blob_id| LevelChunkPacketPayloadSubChunkMetadata { blob_id })
            .collect(),
        serialized_chunk_data: tail.to_vec(),
        ..Default::default()
    }
}

fn item_registry() -> McpePacket {
    // `itemstates` is `item_data`, and its entries lost the prismarine
    // `name`/`runtime_id` spelling for gophertunnel's `Name`/`RuntimeID`.
    McpePacket::from(ItemRegistryPacket {
        item_data: vec![ItemData {
            item_name: "minecraft:shield".into(),
            item_id: 355,
            ..Default::default()
        }],
    })
}

async fn assert_success(mode: CompressionMode, order: SpawnOrder) {
    let transport = ScriptTransport::new(mode, order, false);
    let (mut session, game_data) = LoginSequence::connect_transport(transport, "RustClient")
        .await
        .expect("scripted login");
    assert_eq!(game_data.start_game.runtime_id.actor_runtime_id, RUNTIME_ID);
    assert_eq!(session.decode_error_count(), 0);

    for expected_time in [12_345, 23_456] {
        let deferred = tokio::time::timeout(
            std::time::Duration::from_secs(1),
            session.recv_world_event(0),
        )
        .await
        .expect("pre-spawn packet was discarded")
        .expect("pre-spawn packet must normalize in Play");
        assert_eq!(
            deferred,
            WorldEvent::SetTime(protocol::SetTimeEvent {
                time: expected_time,
            }),
            "pre-spawn SetTime packets must retain FIFO order"
        );
    }

    let initial_radius = tokio::time::timeout(
        std::time::Duration::from_secs(1),
        session.recv_world_event(0),
    )
    .await
    .expect("initial chunk radius acknowledgement was discarded")
    .expect("initial chunk radius acknowledgement must decode in Play");
    assert!(matches!(initial_radius, WorldEvent::ChunkRadiusUpdated(16)));

    let chunk = tokio::time::timeout(
        std::time::Duration::from_secs(1),
        session.recv_world_event_mapped(0, |_| None, |event, payload| Some((event, payload))),
    )
    .await
    .expect("uncached LevelChunk was discarded")
    .expect("uncached LevelChunk must normalize in Play")
    .expect("mapped ingress must select LevelChunk bytes");
    assert_eq!((chunk.0.x, chunk.0.z), (7, -9));
    assert!(chunk.0.payload.is_empty());
    assert_eq!(chunk.1.len(), 1024 * 1024);
    assert!(chunk.1.iter().all(|byte| *byte == 0x5a));

    let post_spawn_time = session
        .recv_world_event(0)
        .await
        .expect("post-spawn SetTime must normalize in Play");
    assert_eq!(
        post_spawn_time,
        WorldEvent::SetTime(protocol::SetTimeEvent { time: 34_567 })
    );
    // The preceding malformed chunk was skipped, not fatal: the session stayed
    // alive and counted one skip.
    assert_eq!(session.world_skip_count(), 1);

    let mut invalid = Packet::from(ClientCacheStatusPacket {
        iscachesupported: true,
    });
    invalid.header.id = McpePacketName::PlayStatusPacket;
    let error = session
        .send(invalid)
        .await
        .expect_err("play send must validate the public header");
    assert!(matches!(error, ProtocolError::HeaderIdMismatch { .. }));

    session
        .send(Packet::from(ClientCacheStatusPacket {
            iscachesupported: true,
        }))
        .await
        .expect("play send");
    let error = session.recv().await.expect_err("malformed batch must fail");
    assert!(matches!(error, ProtocolError::Session(_)));
    assert_eq!(session.decode_error_count(), 1);
}

#[tokio::test]
async fn deflate_login_waits_for_radius_then_spawn_and_enters_play() {
    assert_success(CompressionMode::Deflate, SpawnOrder::RadiusThenSpawn).await;
}

#[tokio::test]
async fn snappy_login_waits_for_spawn_then_radius_and_emits_encrypted_snappy_ack() {
    assert_success(CompressionMode::Snappy, SpawnOrder::SpawnThenRadius).await;
}

#[tokio::test]
async fn no_compression_login_uses_the_uncompressed_batch_marker() {
    assert_success(CompressionMode::None, SpawnOrder::RadiusThenSpawn).await;
}

#[tokio::test]
async fn encrypted_login_advertises_client_cache_only_when_resolver_is_installed() {
    let transport =
        ScriptTransport::new_with_cache(CompressionMode::Deflate, SpawnOrder::RadiusThenSpawn);
    let (session, _) = LoginSequence::connect_transport_with_blob_cache(
        transport,
        "RustClient",
        ClientBlobCache::default(),
    )
    .await
    .expect("cache-enabled scripted login");
    assert!(session.blob_cache_enabled());
}

#[tokio::test]
async fn encrypted_play_keeps_normal_output_moving_while_cached_chunk_resolves() {
    let transport =
        ScriptTransport::new_with_cache(CompressionMode::Deflate, SpawnOrder::RadiusThenSpawn);
    let (mut session, _) = LoginSequence::connect_transport_with_blob_cache(
        transport,
        "RustClient",
        ClientBlobCache::default(),
    )
    .await
    .expect("cache-enabled scripted login");

    for expected in [
        WorldEvent::SetTime(protocol::SetTimeEvent { time: 12_345 }),
        WorldEvent::SetTime(protocol::SetTimeEvent { time: 23_456 }),
        WorldEvent::ChunkRadiusUpdated(16),
    ] {
        assert_eq!(
            session.recv_world_event(0).await.expect("login prelude"),
            expected
        );
    }

    let (ordinary, ordinary_payload) = session
        .recv_world_event_mapped(0, |_| None, |event, payload| Some((event, payload)))
        .await
        .expect("resolver-enabled ordinary LevelChunk")
        .expect("ordinary LevelChunk must use byte ingress");
    assert_eq!((ordinary.x, ordinary.z), (8, -10));
    assert!(ordinary.payload.is_empty());
    assert_eq!(ordinary_payload.len(), 1024 * 1024);
    assert!(ordinary_payload.iter().all(|byte| *byte == 0x6b));

    assert_eq!(
        session
            .recv_world_event(0)
            .await
            .expect("independent SetTime"),
        WorldEvent::SetTime(protocol::SetTimeEvent { time: 34_567 })
    );

    let (chunk, payload) = session
        .recv_world_event_mapped(0, |_| None, |event, payload| Some((event, payload)))
        .await
        .expect("resolved cached chunk")
        .expect("cached transaction must resolve through byte ingress");
    assert_eq!((chunk.x, chunk.z), (9, -11));
    assert!(chunk.payload.is_empty());
    assert_eq!(payload, b"cached-columntail".as_slice());

    let stats = session.blob_cache_stats();
    assert_eq!(stats.hashes_classified, 1);
    assert_eq!(stats.misses, 1);
    assert_eq!(stats.admitted_blobs, 1);
    assert_eq!(stats.reconstructed_level_chunks, 1);
    assert_eq!(stats.pending_transactions, 0);
    assert_eq!(stats.pending_bytes, 0);
}

#[tokio::test]
async fn miss_response_wire_failure_is_fatal_but_semantic_failure_keeps_session_alive() {
    let transport = ScriptTransport::new_with_cache_script(
        CompressionMode::Deflate,
        SpawnOrder::RadiusThenSpawn,
        CachePlayScript::TruncatedMissResponse,
    );
    let (mut malformed_session, _) = LoginSequence::connect_transport_with_blob_cache(
        transport,
        "RustClient",
        ClientBlobCache::default(),
    )
    .await
    .expect("cache-enabled malformed-wire session");
    for expected in [
        WorldEvent::SetTime(protocol::SetTimeEvent { time: 12_345 }),
        WorldEvent::SetTime(protocol::SetTimeEvent { time: 23_456 }),
        WorldEvent::ChunkRadiusUpdated(16),
    ] {
        assert_eq!(
            malformed_session
                .recv_world_event(0)
                .await
                .expect("login prelude"),
            expected
        );
    }
    let error = malformed_session
        .recv_world_event(0)
        .await
        .expect_err("truncated miss-response wire must terminate the session");
    assert!(matches!(error, ProtocolError::Session(_)));
    assert_eq!(malformed_session.decode_error_count(), 1);

    let transport = ScriptTransport::new_with_cache_script(
        CompressionMode::Deflate,
        SpawnOrder::RadiusThenSpawn,
        CachePlayScript::InvalidMissResponseThenTraffic,
    );
    let (mut semantic_session, _) = LoginSequence::connect_transport_with_blob_cache(
        transport,
        "RustClient",
        ClientBlobCache::default(),
    )
    .await
    .expect("cache-enabled semantic-rejection session");
    for expected in [
        WorldEvent::SetTime(protocol::SetTimeEvent { time: 12_345 }),
        WorldEvent::SetTime(protocol::SetTimeEvent { time: 23_456 }),
        WorldEvent::ChunkRadiusUpdated(16),
    ] {
        assert_eq!(
            semantic_session
                .recv_world_event(0)
                .await
                .expect("login prelude"),
            expected
        );
    }
    assert_eq!(
        semantic_session
            .recv_world_event(0)
            .await
            .expect("dead transaction emits bounded recovery"),
        WorldEvent::ChunkResync(protocol::ChunkResyncEvent {
            dimension: 0,
            x: 31,
            z: -47,
            requested_sub_chunks: None,
            requested_sub_chunk_ys: None,
        })
    );
    assert_eq!(
        semantic_session
            .recv_world_event(0)
            .await
            .expect("unrelated traffic follows recovery"),
        WorldEvent::SetTime(protocol::SetTimeEvent { time: 45_678 })
    );
    let stats = semantic_session.blob_cache_stats();
    assert_eq!(stats.skipped_miss_responses, 1);
    assert_eq!(stats.rejected_blobs, 1);
    assert_eq!(stats.pending_transactions, 0);
    assert_eq!(stats.abandoned_cached_transactions, 1);
    assert_eq!(stats.recovery_requests, 1);
    assert_eq!(semantic_session.decode_error_count(), 0);
}

#[tokio::test]
async fn conflicting_start_game_runtime_ids_are_rejected() {
    let transport =
        ScriptTransport::new(CompressionMode::Deflate, SpawnOrder::RadiusThenSpawn, true);
    let error = match LoginSequence::connect_transport(transport, "RustClient").await {
        Ok(_) => panic!("conflicting StartGame packets must fail"),
        Err(error) => error,
    };
    assert!(
        error
            .to_string()
            .contains("conflicting StartGame runtime entity ID")
    );
}

#[tokio::test]
async fn unadvertised_resource_pack_stack_is_rejected_before_completed_response() {
    let transport = ScriptTransport::new_with_pack_stack(
        CompressionMode::Deflate,
        SpawnOrder::RadiusThenSpawn,
        false,
        true,
    );
    let error = match LoginSequence::connect_transport(transport, "RustClient").await {
        Ok(_) => panic!("unadvertised resource pack stack must fail login"),
        Err(error) => error,
    };
    assert!(error.to_string().contains("resource-pack handoff failed"));
}
