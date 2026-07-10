use std::marker::PhantomData;
use std::sync::Arc;
use std::time::Duration;

use crate::config::BedrockListenerConfig;

use crate::valentine::{
    ChunkRadiusUpdatePacket, NetworkChunkPublisherUpdatePacket, PlayStatusPacket,
    PlayStatusPacketStatus, ResourcePackStackPacket, ResourcePacksInfoPacket,
    ServerToClientHandshakePacket,
};
use aes_gcm::Aes256Gcm;
use base64::Engine;
use base64::engine::general_purpose::{STANDARD, STANDARD_NO_PAD};
use jsonwebtoken::{Algorithm, EncodingKey, Header, encode};
use p384::{
    PublicKey, SecretKey,
    pkcs8::{DecodePublicKey, EncodePrivateKey, EncodePublicKey},
};
use rand::{RngCore, thread_rng};
use sha2::{Digest, Sha256};
use tracing::instrument;
use uuid::Uuid;

use crate::auth::{ValidatedIdentity, authenticate_login};
use crate::error::{AuthError, JolyneError, ProtocolError};
use crate::stream::{
    BedrockStream, Handshake, Login, Play, ResourcePacks, SecurePending, Server, StartGame,
    transport::{BedrockTransport, Transport},
};
use crate::valentine::types::Experiments;
use crate::valentine::types::ResourcePackIdVersions;
use crate::valentine::{BorrowedMcpePacketData, BorrowedRequestNetworkSettingsPacket};
use crate::valentine::{McpePacket, McpePacketData};
use crate::valentine::{NetworkSettingsPacket, NetworkSettingsPacketCompressionAlgorithm};
use crate::world::{WorldJoinParams, WorldTemplate};

const START_GAME_STEP_TIMEOUT: Duration = Duration::from_secs(60);

/// Configuration for the Server Handshake.
#[derive(Debug, Clone)]
pub struct ServerHandshakeConfig {
    pub server_key: SecretKey, // Persistent key for the session
}

// --- State: Handshake (Initial) ---

impl<T: Transport> BedrockStream<Handshake, Server, T> {
    /// Creates a server handshake stream from a transport.
    ///
    /// Used for NetherNet and other non-listener transports where you have
    /// the raw stream and want to start the Bedrock handshake.
    pub fn from_transport(
        mut transport: BedrockTransport<T>,
        config: Arc<BedrockListenerConfig>,
    ) -> Self {
        transport.apply_listener_config(&config);
        Self {
            transport,
            state: Handshake {
                config: Some(config),
            },
            _role: PhantomData,
        }
    }

    /// Accepts a new connection and negotiates network settings.

    #[instrument(skip_all, level = "trace")]

    pub async fn accept_network_settings(
        mut self,
    ) -> Result<BedrockStream<Login, Server, T>, JolyneError> {
        let packet = self.transport.recv_packet_borrowed().await?;

        match packet.data {
            BorrowedMcpePacketData::PacketRequestNetworkSettings(req) => {
                let req: BorrowedRequestNetworkSettingsPacket = req;
                let server_protocol = crate::valentine::PROTOCOL_VERSION;

                let client_protocol = req.client_protocol;

                // Add protocol version context to the current span

                tracing::Span::current().record("client_protocol", client_protocol);

                if client_protocol != server_protocol {
                    let status = if client_protocol < server_protocol {
                        PlayStatusPacketStatus::FailedClient
                    } else {
                        PlayStatusPacketStatus::FailedSpawn
                    };

                    self.transport
                        .send_raw(McpePacket::from(PlayStatusPacket { status }))
                        .await?;

                    tracing::warn!(client_protocol, server_protocol, "Protocol mismatch");

                    return Err(ProtocolError::IncompatibleProtocol {
                        client_protocol,
                        server_protocol,
                    }
                    .into());
                }

                let listener_config = self.state.config.as_ref().expect("config");

                let settings = NetworkSettingsPacket {
                    compression_threshold: listener_config.compression_threshold,
                    compression_algorithm: NetworkSettingsPacketCompressionAlgorithm::Deflate,
                    client_throttle: false,
                    client_throttle_threshold: 0,
                    client_throttle_scalar: 0.0,
                };

                self.transport.send_raw(McpePacket::from(settings)).await?;

                self.transport.set_compression(
                    true,
                    listener_config.compression_level,
                    listener_config.compression_threshold,
                );

                tracing::debug!("Network settings negotiated");

                Ok(BedrockStream {
                    transport: self.transport,
                    state: Login {
                        config: self.state.config,
                    },
                    _role: PhantomData,
                })
            }
            _ => Err(
                ProtocolError::UnexpectedHandshake("Expected RequestNetworkSettings".into()).into(),
            ),
        }
    }

    /// Helper: Orchestrates the entire join sequence using a WorldTemplate.
    /// This replaces the old `simple_login`.
    pub async fn accept_join_sequence(
        self,
        template: &WorldTemplate,
        server_key: &SecretKey,
    ) -> Result<(BedrockStream<Play, Server, T>, ValidatedIdentity), JolyneError> {
        // 1. Network Settings
        let login = self.accept_network_settings().await?;

        let require_resource_packs = login
            .state
            .config
            .as_ref()
            .map(|config| config.require_resource_packs)
            .unwrap_or(false);

        // 2. Auth
        let (secure, identity) = login.authenticate().await?;

        // 3. Encryption
        let packs = secure
            .finish_handshake(
                &ServerHandshakeConfig {
                    server_key: server_key.clone(),
                },
                &identity.identity_public_key,
            )
            .await?;

        // 4. Resource Packs (None/Default)
        let start_game_state = packs.negotiate_packs(require_resource_packs).await?;

        // 5. Personalize Template
        let join_params = template.to_join_params(rand::random());

        // 6. Join
        let play = start_game_state.start_game(join_params).await?;
        Ok((play, identity))
    }
}

// --- State: Login ---

impl<T: Transport> BedrockStream<Login, Server, T> {
    #[instrument(skip_all, level = "trace")]

    pub async fn authenticate(
        mut self,
    ) -> Result<(BedrockStream<SecurePending, Server, T>, ValidatedIdentity), JolyneError> {
        let login_data = self.recv_expect_login().await?;

        let listener_config = self.state.config.as_ref().expect("config");
        let identity = login_data
            .tokens
            .identity
            .as_str()
            .map_err(|_| AuthError::InvalidUtf8)?;
        let client = login_data
            .tokens
            .client
            .as_str()
            .map_err(|_| AuthError::InvalidUtf8)?;

        let identity = authenticate_login(
            identity,
            client,
            listener_config.online_mode,
            listener_config.allow_legacy_auth,
        )
        .await?;

        tracing::debug!(display_name = ?identity.display_name, uuid = ?identity.uuid, "Client authenticated");

        Ok((
            BedrockStream {
                transport: self.transport,
                state: SecurePending {
                    config: self.state.config,
                },
                _role: PhantomData,
            },
            identity,
        ))
    }

    async fn recv_expect_login(
        &mut self,
    ) -> Result<crate::valentine::BorrowedLoginPacket, JolyneError> {
        let packet = self.transport.recv_packet_borrowed().await?;

        match packet.data {
            BorrowedMcpePacketData::PacketLogin(login) => Ok(login),
            _ => Err(ProtocolError::MissingLoginPacket.into()),
        }
    }
}

// --- State: SecurePending ---

impl<T: Transport> BedrockStream<SecurePending, Server, T> {
    #[instrument(skip_all, level = "trace")]

    pub async fn finish_handshake(
        mut self,
        config: &ServerHandshakeConfig,
        client_pub_b64: &str,
    ) -> Result<BedrockStream<ResourcePacks, Server, T>, JolyneError> {
        let listener_config = self.state.config.as_ref().expect("config");

        if listener_config.encryption_enabled {
            self.perform_encryption_handshake(&config.server_key, client_pub_b64)
                .await?;
        } else {
            tracing::debug!("Encryption disabled by config");
        }

        self.transport
            .send_batch(&[McpePacket::from(PlayStatusPacket {
                status: PlayStatusPacketStatus::LoginSuccess,
            })])
            .await?;

        Ok(BedrockStream {
            transport: self.transport,
            state: ResourcePacks { early_packet: None },
            _role: PhantomData,
        })
    }

    #[instrument(skip_all, level = "trace")]

    async fn perform_encryption_handshake(
        &mut self,
        server_key: &SecretKey,
        client_pub_b64: &str,
    ) -> Result<(), JolyneError> {
        let client_der = STANDARD
            .decode(client_pub_b64)
            .map_err(|e| ProtocolError::UnexpectedHandshake(e.to_string()))?;

        let client_pub = PublicKey::from_public_key_der(&client_der)
            .map_err(|e| ProtocolError::UnexpectedHandshake(e.to_string()))?;
        let mut salt = [0u8; 16];
        thread_rng().fill_bytes(&mut salt);
        let shared_secret =
            p384::ecdh::diffie_hellman(server_key.to_nonzero_scalar(), client_pub.as_affine());
        let shared_bytes = shared_secret.raw_secret_bytes();

        let mut h = Sha256::new();
        h.update(salt);
        h.update(shared_bytes);

        let key_bytes = h.finalize();
        let key = aes_gcm::Key::<Aes256Gcm>::from_slice(&key_bytes);

        let mut iv = [0u8; 12];

        iv.copy_from_slice(&key_bytes[0..12]);
        let server_pub_der = server_key.public_key().to_public_key_der().unwrap();
        let server_pub_b64 = STANDARD.encode(server_pub_der.as_bytes());
        let mut header = Header::new(Algorithm::ES384);
        header.x5u = Some(server_pub_b64);

        #[derive(serde::Serialize)]
        struct SaltClaims {
            salt: String,
        }

        let claims = SaltClaims {
            salt: STANDARD_NO_PAD.encode(salt),
        };

        let server_priv_der = server_key.to_pkcs8_der().unwrap();

        let encoding_key = EncodingKey::from_ec_der(server_priv_der.as_bytes());

        let token = encode(&header, &claims, &encoding_key).unwrap();

        let handshake_pkt = ServerToClientHandshakePacket { token };

        self.transport
            .send_batch(&[McpePacket::from(handshake_pkt)])
            .await?;

        self.transport.enable_encryption(*key, iv);

        let packet = self.transport.recv_packet_raw().await?;
        if packet.id != crate::valentine::McpePacketName::PacketClientToServerHandshake {
            return Err(ProtocolError::UnexpectedHandshake(
                "Expected ClientToServerHandshake".into(),
            )
            .into());
        }

        tracing::debug!("Server encryption enabled");

        Ok(())
    }
}

// --- State: ResourcePacks ---

impl<T: Transport> BedrockStream<ResourcePacks, Server, T> {
    #[instrument(skip_all, level = "trace")]

    pub async fn negotiate_packs(
        mut self,

        required: bool,
    ) -> Result<BedrockStream<StartGame, Server, T>, JolyneError> {
        let info = ResourcePacksInfoPacket {
            must_accept: required,
            has_addons: false,
            has_scripts: false,
            disable_vibrant_visuals: false,
            world_template: crate::valentine::types::ResourcePacksInfoPacketWorldTemplate {
                uuid: Uuid::nil(),
                version: "1.0.0".to_string(),
            },
            texture_packs: vec![],
        };

        let stack = ResourcePackStackPacket {
            must_accept: required,
            resource_packs: ResourcePackIdVersions::new(),
            game_version: crate::valentine::GAME_VERSION.to_string(),
            experiments: Experiments::new(),
            experiments_previously_used: false,
            has_editor_packs: false,
        };

        self.transport
            .send_batch(&[McpePacket::from(info), McpePacket::from(stack)])
            .await?;

        tokio::time::timeout(START_GAME_STEP_TIMEOUT, async {
            loop {
                let packets = self.transport.recv_batch().await?;
                for pkt in packets {
                    if let McpePacketData::PacketResourcePackClientResponse(resp) = pkt.data {
                        use crate::valentine::ResourcePackClientResponsePacketResponseStatus as Status;
                        match resp.response_status {
                            Status::Refused if required => {
                                tracing::warn!("Client refused required resource packs");
                                return Err(ProtocolError::UnexpectedHandshake(
                                    "Client refused required packs".into(),
                                )
                                .into());
                            }

                            Status::Refused => {
                                tracing::debug!("Client refused optional resource packs");

                                return Ok(BedrockStream {
                                    transport: self.transport,
                                    state: StartGame,
                                    _role: PhantomData,
                                });
                            }

                            Status::SendPacks => {}

                            Status::HaveAllPacks => {
                                tracing::debug!("Client has all resource packs");
                            }
                            Status::Completed => {
                                tracing::debug!("Client completed resource pack negotiation");

                                return Ok(BedrockStream {
                                    transport: self.transport,
                                    state: StartGame,
                                    _role: PhantomData,
                                });
                            }
                            _ => {}
                        }
                    }
                }
            }
        })
        .await
        .map_err(|_| {
            ProtocolError::UnexpectedHandshake("Timeout waiting for resource pack response".into())
        })?
    }
}

// --- State: StartGame ---

impl<T: Transport> BedrockStream<StartGame, Server, T> {
    /// Completes the sequence by sending StartGame and waiting for client initialization.
    #[instrument(skip_all, level = "trace")]
    pub async fn start_game(
        mut self,
        params: WorldJoinParams,
    ) -> Result<BedrockStream<Play, Server, T>, JolyneError> {
        // 1. Send StartGame Packet & ItemRegistry
        let publisher_position = params.start_game.spawn_position.clone();
        self.transport
            .send_batch(&[
                McpePacket::from(params.start_game),
                McpePacket::from(params.item_registry.as_ref().clone()), // Clone for now
            ])
            .await?;

        tracing::debug!("StartGame packet sent");

        // 2. Wait for RequestChunkRadius
        let requested_radius = tokio::time::timeout(START_GAME_STEP_TIMEOUT, async {
            loop {
                let pkt = self.transport.recv_packet().await?;

                if let McpePacketData::PacketRequestChunkRadius(req) = pkt.data {
                    return Ok::<_, JolyneError>(req.chunk_radius);
                }
                // ignore all other packets. Maybe add a configable logging here?
            }
        })
        .await
        .map_err(|_| {
            ProtocolError::UnexpectedHandshake("Timeout waiting for RequestChunkRadius".into())
        })??;

        let radius = requested_radius.clamp(2, 32);

        // 3. Send World Data
        // IMPORTANT: CreativeContent must come BEFORE PlayerSpawn status

        tracing::debug!("Sending CreativeContent and PlayerSpawn status...");
        self.transport
            .send_batch(&[
                McpePacket::from(ChunkRadiusUpdatePacket {
                    chunk_radius: radius,
                }),
                McpePacket::from(NetworkChunkPublisherUpdatePacket {
                    coordinates: publisher_position,
                    radius,
                    saved_chunks: vec![],
                }),
                McpePacket::from(params.biome_definitions.as_ref().clone()),
                McpePacket::from(params.available_entities.as_ref().clone()),
                McpePacket::from(params.creative_content.as_ref().clone()),
                McpePacket::from(PlayStatusPacket {
                    status: PlayStatusPacketStatus::PlayerSpawn,
                }),
            ])
            .await?;

        tracing::debug!("Batch sent, waiting for ServerboundLoadingScreen...");

        // 4. Loading Screen Handshake (Types 1 & 2)
        tokio::time::timeout(START_GAME_STEP_TIMEOUT, async {
            loop {
                let pkt = self.transport.recv_packet().await?;

                if let McpePacketData::PacketServerboundLoadingScreen(pk) = pkt.data
                    && pk.type_ == 1
                {
                    return Ok::<_, JolyneError>(());
                }
            }
        })
        .await
        .map_err(|_| {
            ProtocolError::UnexpectedHandshake("Timeout waiting for StartLoadingScreen".into())
        })??;

        let end_loading = tokio::time::timeout(START_GAME_STEP_TIMEOUT, async {
            loop {
                let pkt = self.transport.recv_packet().await?;

                if let McpePacketData::PacketServerboundLoadingScreen(pk) = pkt.data
                    && pk.type_ == 2
                {
                    return Ok::<_, JolyneError>(());
                }
            }
        })
        .await;

        match end_loading {
            Ok(Ok(())) => {}
            Ok(Err(e)) => return Err(e),
            Err(_) => {
                return Err(ProtocolError::UnexpectedHandshake(
                    "Timeout waiting for EndLoadingScreen".into(),
                )
                .into());
            }
        }

        tracing::debug!("Client finished loading");
        // 5. Wait for SetLocalPlayerAsInitialized
        tokio::time::timeout(START_GAME_STEP_TIMEOUT, async {
            loop {
                let pkt = self.transport.recv_packet().await?;

                if matches!(
                    pkt.data,
                    McpePacketData::PacketSetLocalPlayerAsInitialized(_)
                ) {
                    return Ok::<_, JolyneError>(());
                } else {
                    tracing::debug!("Waiting for init, got: {:?}", pkt.data.packet_id());
                }
            }
        })
        .await
        .map_err(|_| {
            ProtocolError::UnexpectedHandshake(
                "Timeout waiting for SetLocalPlayerAsInitialized".into(),
            )
        })??;

        tracing::debug!("Client initialized, entering Play state");
        Ok(BedrockStream {
            transport: self.transport,
            state: Play,
            _role: PhantomData,
        })
    }
}

// --- State: Play ---

impl<T: Transport> BedrockStream<Play, Server, T> {
    /// Receive the next packet as a borrowed protocol view.
    #[instrument(skip_all, level = "trace")]
    pub async fn recv_packet_borrowed(
        &mut self,
    ) -> Result<crate::valentine::BorrowedMcpePacket, JolyneError> {
        self.transport.recv_packet_borrowed().await
    }

    /// Receive the next packet from the client.
    ///
    /// This materializes an owned packet. Prefer [`Self::recv_packet_borrowed`]
    /// on hot ingress paths when ownership is not required yet.
    #[instrument(skip_all, level = "trace")]
    pub async fn recv_packet(&mut self) -> Result<McpePacket, JolyneError> {
        self.transport.recv_packet().await
    }

    /// Send a packet to the client.
    #[instrument(skip_all, level = "trace")]
    pub async fn send_packet(&mut self, packet: McpePacket) -> Result<(), JolyneError> {
        self.transport.send(packet).await
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::batch::{decode_batch, encode_batch_multi};
    use crate::stream::transport::{BedrockTransport, TransportMessage, TransportRecvMessage};
    use crate::valentine::{
        ClientCacheStatusPacket, ResourcePackClientResponsePacket,
        ResourcePackClientResponsePacketResponseStatus,
    };
    use bytes::Bytes;
    use std::collections::VecDeque;
    use std::io;
    use std::net::{IpAddr, Ipv4Addr, SocketAddr};
    use std::pin::Pin;
    use std::sync::{Arc, Mutex};
    use std::task::{Context, Poll};

    struct ScriptedTransport {
        inbound: VecDeque<TransportRecvMessage>,
        sent: Arc<Mutex<Vec<TransportMessage>>>,
        received: Arc<Mutex<usize>>,
    }

    impl ScriptedTransport {
        fn new(
            inbound: Vec<Bytes>,
            sent: Arc<Mutex<Vec<TransportMessage>>>,
            received: Arc<Mutex<usize>>,
        ) -> Self {
            Self {
                inbound: inbound
                    .into_iter()
                    .map(TransportRecvMessage::Contiguous)
                    .collect(),
                sent,
                received,
            }
        }
    }

    impl Transport for ScriptedTransport {
        type Error = io::Error;

        const USES_BATCH_PREFIX: bool = true;

        fn poll_send(
            self: Pin<&mut Self>,
            _cx: &mut Context<'_>,
            msg: TransportMessage,
        ) -> Poll<Result<(), Self::Error>> {
            self.sent.lock().expect("sent lock").push(msg);
            Poll::Ready(Ok(()))
        }

        fn poll_recv(
            self: Pin<&mut Self>,
            _cx: &mut Context<'_>,
        ) -> Poll<Option<Result<TransportRecvMessage, Self::Error>>> {
            let this = self.get_mut();
            let msg = this.inbound.pop_front();
            if msg.is_some() {
                *this.received.lock().expect("received lock") += 1;
            }
            Poll::Ready(msg.map(Ok))
        }

        fn peer_addr(&self) -> SocketAddr {
            SocketAddr::new(IpAddr::V4(Ipv4Addr::LOCALHOST), 0)
        }
    }

    fn compressed_frame(packet: McpePacket) -> Bytes {
        encode_batch_multi(&[packet], true, 0, 0, true).expect("encode packet")
    }

    #[tokio::test]
    async fn resource_pack_negotiation_ignores_client_cache_status_before_pack_response() {
        let sent = Arc::new(Mutex::new(Vec::new()));
        let received = Arc::new(Mutex::new(0usize));
        let inbound = vec![
            compressed_frame(McpePacket::from(ClientCacheStatusPacket { enabled: false })),
            compressed_frame(McpePacket::from(ResourcePackClientResponsePacket {
                response_status: ResourcePackClientResponsePacketResponseStatus::HaveAllPacks,
                resourcepackids: vec![],
            })),
            compressed_frame(McpePacket::from(ResourcePackClientResponsePacket {
                response_status: ResourcePackClientResponsePacketResponseStatus::Completed,
                resourcepackids: vec![],
            })),
        ];

        let mut transport = BedrockTransport::new(ScriptedTransport::new(
            inbound,
            sent.clone(),
            received.clone(),
        ));
        transport.set_compression(true, 0, 0);
        let stream = BedrockStream {
            transport,
            state: ResourcePacks { early_packet: None },
            _role: PhantomData,
        };

        let _start = stream
            .negotiate_packs(false)
            .await
            .expect("ClientCacheStatus should not block pack negotiation");

        let sent = sent.lock().expect("sent lock");
        assert_eq!(
            sent.len(),
            1,
            "server should send ResourcePacksInfo and ResourcePackStack in one batch"
        );

        let mut frame = sent[0].buffer.clone();
        let decoded = decode_batch(
            &mut frame,
            &valentine::bedrock::context::BedrockSession { shield_item_id: 0 },
            true,
            None,
        )
        .expect("decode resource pack negotiation batch");

        assert!(matches!(
            decoded.as_slice(),
            [
                McpePacket {
                    data: McpePacketData::PacketResourcePacksInfo(_),
                    ..
                },
                McpePacket {
                    data: McpePacketData::PacketResourcePackStack(_),
                    ..
                }
            ]
        ));
        assert_eq!(
            *received.lock().expect("received lock"),
            3,
            "server must wait for Completed before entering StartGame"
        );
    }
}
