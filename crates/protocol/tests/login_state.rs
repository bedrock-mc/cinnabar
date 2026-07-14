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
    ChunkRadiusUpdatePacket, ClientCacheStatusPacket, ClientToServerHandshakePacket,
    ItemRegistryPacket, ItemstatesItem, McpePacket, McpePacketData, McpePacketName,
    NetworkSettingsPacket, NetworkSettingsPacketCompressionAlgorithm, PlayStatusPacket,
    PlayStatusPacketStatus, RequestChunkRadiusPacket, RequestNetworkSettingsPacket,
    ResourcePackClientResponsePacket, ResourcePackClientResponsePacketResponseStatus,
    ResourcePackIdVersionsItem, ResourcePackStackPacket, ResourcePacksInfoPacket,
    ServerToClientHandshakePacket, ServerboundLoadingScreenPacket,
    SetLocalPlayerAsInitializedPacket, SetTimePacket, StartGamePacket,
};
use jsonwebtoken::{Algorithm, EncodingKey, Header};
use p384::pkcs8::{DecodePublicKey, EncodePrivateKey, EncodePublicKey};
use p384::{PublicKey, SecretKey};
use protocol::{BedrockSession, LoginSequence, Packet, ProtocolError, WorldEvent};
use serde::Serialize;
use sha2::{Digest, Sha256};

type Aes256Ctr = ctr::Ctr32BE<Aes256>;

const RUNTIME_ID: i64 = 0x1234_5678;
const OTHER_RUNTIME_ID: i64 = 0x7654_3210;
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
            Self::Deflate => NetworkSettingsPacketCompressionAlgorithm::Deflate,
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

struct ScriptTransport {
    script: Arc<Mutex<ServerScript>>,
}

impl ScriptTransport {
    fn new(mode: CompressionMode, order: SpawnOrder, conflicting_start: bool) -> Self {
        Self::new_with_pack_stack(mode, order, conflicting_start, false)
    }

    fn new_with_pack_stack(
        mode: CompressionMode,
        order: SpawnOrder,
        conflicting_start: bool,
        non_empty_pack_stack: bool,
    ) -> Self {
        Self {
            script: Arc::new(Mutex::new(ServerScript::new(
                mode,
                order,
                conflicting_start,
                non_empty_pack_stack,
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
    ) -> Self {
        Self {
            mode,
            order,
            conflicting_start,
            non_empty_pack_stack,
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
                        data: McpePacketData::PacketRequestNetworkSettings(
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
                            data: McpePacketData::PacketLogin(login),
                            ..
                        },
                    ] => login,
                    other => panic!("expected Login, got {other:?}"),
                };
                let client_public_key = login_public_key(&login.tokens.identity);
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
                        data: McpePacketData::PacketClientToServerHandshake(
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
                        data: McpePacketData::PacketClientCacheStatus(ClientCacheStatusPacket {
                            enabled: false
                        }),
                        ..
                    }]
                ));
                self.stage = 4;
            }
            4 => {
                let packets = self.decode_encrypted_client(frame);
                assert!(matches!(
                    packets.as_slice(),
                    [McpePacket {
                        data: McpePacketData::PacketResourcePackClientResponse(
                            ResourcePackClientResponsePacket {
                                response_status:
                                    ResourcePackClientResponsePacketResponseStatus::HaveAllPacks,
                                ..
                            }
                        ),
                        ..
                    }]
                ));
                let resource_packs = self
                    .non_empty_pack_stack
                    .then(|| ResourcePackIdVersionsItem {
                        uuid: "pack-id".into(),
                        version: "1.0.0".into(),
                        name: "test pack".into(),
                    })
                    .into_iter()
                    .collect();
                self.enqueue_encrypted(&[McpePacket::from(ResourcePackStackPacket {
                    resource_packs,
                    ..Default::default()
                })]);
                self.stage = 5;
            }
            5 => {
                let packets = self.decode_encrypted_client(frame);
                assert!(matches!(
                    packets.as_slice(),
                    [McpePacket {
                        data: McpePacketData::PacketResourcePackClientResponse(
                            ResourcePackClientResponsePacket {
                                response_status:
                                    ResourcePackClientResponsePacketResponseStatus::Completed,
                                ..
                            }
                        ),
                        ..
                    }]
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
                            data: McpePacketData::PacketServerboundLoadingScreen(
                                ServerboundLoadingScreenPacket { type_: 1, .. }
                            ),
                            ..
                        },
                        McpePacket {
                            data: McpePacketData::PacketRequestChunkRadius(
                                RequestChunkRadiusPacket {
                                    chunk_radius: 16,
                                    max_radius: 16,
                                }
                            ),
                            ..
                        }
                    ]
                ));
                let radius = McpePacket::from(ChunkRadiusUpdatePacket { chunk_radius: 16 });
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
                            data: McpePacketData::PacketServerboundLoadingScreen(
                                ServerboundLoadingScreenPacket { type_: 2, .. }
                            ),
                            ..
                        },
                        McpePacket {
                            data: McpePacketData::PacketSetLocalPlayerAsInitialized(
                                SetLocalPlayerAsInitializedPacket {
                                    runtime_entity_id: RUNTIME_ID
                                }
                            ),
                            ..
                        }
                    ]
                ));
                self.stage = 8;
            }
            8 => {
                let packets = self.decode_encrypted_client(frame);
                assert!(matches!(
                    packets.as_slice(),
                    [McpePacket {
                        data: McpePacketData::PacketClientCacheStatus(ClientCacheStatusPacket {
                            enabled: true
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

    let mut frame = BytesMut::from(&b"\xfe"[..]);
    match mode {
        None => frame.extend_from_slice(&payload),
        Some(CompressionMode::Deflate) => {
            use std::io::Write;
            let mut encoder = DeflateEncoder::new(Vec::new(), Compression::new(6));
            encoder.write_all(&payload).expect("deflate payload");
            frame.extend_from_slice(&[0]);
            frame.extend_from_slice(&encoder.finish().expect("finish deflate"));
        }
        Some(CompressionMode::Snappy) => {
            frame.extend_from_slice(&[1]);
            frame.extend_from_slice(
                &snap::raw::Encoder::new()
                    .compress_vec(&payload)
                    .expect("snappy payload"),
            );
        }
        Some(CompressionMode::None) => {
            frame.extend_from_slice(&[0xff]);
            frame.extend_from_slice(&payload);
        }
    }
    frame.freeze()
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
        ServerToClientHandshakePacket { token },
        ScriptCrypto::new(key),
    )
}

fn start_game(runtime_entity_id: i64) -> McpePacket {
    McpePacket::from(StartGamePacket {
        runtime_entity_id,
        ..Default::default()
    })
}

fn item_registry() -> McpePacket {
    McpePacket::from(ItemRegistryPacket {
        itemstates: vec![ItemstatesItem {
            name: "minecraft:shield".into(),
            runtime_id: 355,
            ..Default::default()
        }],
    })
}

async fn assert_success(mode: CompressionMode, order: SpawnOrder) {
    let transport = ScriptTransport::new(mode, order, false);
    let (mut session, game_data) = LoginSequence::connect_transport(transport, "RustClient")
        .await
        .expect("scripted login");
    assert_eq!(game_data.start_game.runtime_entity_id, RUNTIME_ID);
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

    let mut invalid = Packet::from(ClientCacheStatusPacket { enabled: true });
    invalid.header.id = McpePacketName::PacketPlayStatus;
    let error = session
        .send(invalid)
        .await
        .expect_err("play send must validate the public header");
    assert!(matches!(error, ProtocolError::HeaderIdMismatch { .. }));

    session
        .send(Packet::from(ClientCacheStatusPacket { enabled: true }))
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
async fn non_empty_resource_pack_stack_is_rejected_before_completed_response() {
    let transport = ScriptTransport::new_with_pack_stack(
        CompressionMode::Deflate,
        SpawnOrder::RadiusThenSpawn,
        false,
        true,
    );
    let error = match LoginSequence::connect_transport(transport, "RustClient").await {
        Ok(_) => panic!("non-empty resource pack stack must fail login"),
        Err(error) => error,
    };
    assert!(
        error
            .to_string()
            .contains("Resource pack downloads are not implemented")
    );
}
