use std::marker::PhantomData;
use std::net::SocketAddr;

use aes_gcm::Aes256Gcm;
use base64::Engine;
use base64::engine::general_purpose::{STANDARD, STANDARD_NO_PAD, URL_SAFE_NO_PAD};
use jsonwebtoken::decode_header;
use p384::ecdsa::{Signature, VerifyingKey, signature::Verifier};
use p384::{PublicKey, SecretKey, pkcs8::DecodePublicKey};
use serde::Deserialize;
use sha2::{Digest, Sha256};
#[cfg(feature = "raknet")]
use tokio_raknet::RaknetStream;
use tracing::instrument;
use uuid::Uuid;

use crate::error::{JolyneError, ProtocolError};
use crate::gamedata::GameData;
#[cfg(feature = "raknet")]
use crate::stream::transport::RakNetTransport;
use crate::stream::{
    BedrockStream, Client, Handshake, Login, Play, ResourcePacks, SecurePending, StartGame,
    transport::{BedrockTransport, Transport},
};
use crate::valentine::BorrowedMcpePacketData;
use crate::valentine::{
    AvailableEntityIdentifiersPacket, BiomeDefinitionListPacket, ClientCacheStatusPacket,
    ClientToServerHandshakePacket, CreativeContentPacket, ItemRegistryPacket, LoginPacket,
    PlayStatusPacketStatus, RequestChunkRadiusPacket, RequestNetworkSettingsPacket,
    ResourcePackClientResponsePacket, ResourcePackClientResponsePacketResponseStatus,
    ServerboundLoadingScreenPacket, SetLocalPlayerAsInitializedPacket, StartGamePacket,
};
use crate::valentine::{McpePacket, McpePacketData, NetworkSettingsPacketCompressionAlgorithm};

// --- Config ---

/// Xbox Live credentials for authenticated connections.
#[derive(Debug, Clone)]
pub struct XblCredentials {
    /// The XBL authorization token (from BEDROCK_MULTIPLAYER relying party)
    pub token: String,
    /// The user hash for the XBL auth header
    pub user_hash: String,
    /// Xbox User ID (numeric string)
    pub xuid: String,
}

impl XblCredentials {
    pub fn new(
        token: impl Into<String>,
        user_hash: impl Into<String>,
        xuid: impl Into<String>,
    ) -> Self {
        Self {
            token: token.into(),
            user_hash: user_hash.into(),
            xuid: xuid.into(),
        }
    }
}

#[derive(Debug, Clone)]
pub struct ClientHandshakeConfig {
    pub server_addr: SocketAddr,
    pub identity_key: SecretKey, // Client's private key
    pub display_name: String,
    pub uuid: Uuid,
    /// Xbox Live credentials for authenticated servers (optional)
    pub xbl_credentials: Option<XblCredentials>,
}

impl ClientHandshakeConfig {
    /// Generates a configuration with a random identity key and UUID.
    /// Useful for testing or simple bots that don't need Xbox Live auth.
    pub fn random(server_addr: SocketAddr, display_name: impl Into<String>) -> Self {
        Self {
            server_addr,
            identity_key: SecretKey::random(&mut rand::thread_rng()),
            display_name: display_name.into(),
            uuid: Uuid::new_v4(),
            xbl_credentials: None,
        }
    }

    /// Creates a configuration with Xbox Live credentials for authenticated servers.
    pub fn with_xbox_live(
        server_addr: SocketAddr,
        identity_key: SecretKey,
        display_name: impl Into<String>,
        uuid: Uuid,
        xbl_credentials: XblCredentials,
    ) -> Self {
        Self {
            server_addr,
            identity_key,
            display_name: display_name.into(),
            uuid,
            xbl_credentials: Some(xbl_credentials),
        }
    }
}

// --- State: Handshake (Initial) ---

// RakNet-specific connect method
#[cfg(feature = "raknet")]
impl BedrockStream<Handshake, Client, RakNetTransport> {
    /// Connects to a Bedrock server and initializes the stream in the `Handshake` state.
    #[instrument(skip_all, level = "trace", fields(addr = %addr))]
    pub async fn connect(addr: SocketAddr) -> Result<Self, JolyneError> {
        let stream = RaknetStream::connect(addr).await?;
        tracing::debug!("Connected to server");

        Ok(Self {
            transport: BedrockTransport::new(RakNetTransport::new(stream)),
            state: Handshake { config: None },
            _role: PhantomData,
        })
    }
}

// Generic methods for any transport
impl<T: Transport> BedrockStream<Handshake, Client, T> {
    /// Creates a client handshake stream from a transport.
    ///
    /// Used for NetherNet and other non-RakNet transports where you have
    /// the raw stream and want to start the Bedrock handshake.
    pub fn from_transport(transport: BedrockTransport<T>) -> Self {
        Self {
            transport,
            state: Handshake { config: None },
            _role: PhantomData,
        }
    }

    /// Requests network settings from the server and enables compression.
    #[instrument(skip_all, level = "trace")]
    pub async fn request_settings(
        mut self,
    ) -> Result<BedrockStream<Login, Client, T>, JolyneError> {
        let req = RequestNetworkSettingsPacket {
            client_protocol: crate::valentine::PROTOCOL_VERSION,
        };
        self.transport.send_raw(McpePacket::from(req)).await?;

        let settings_pkt = self.transport.recv_packet_borrowed().await?;

        match settings_pkt.data {
            BorrowedMcpePacketData::PacketNetworkSettings(settings) => {
                match settings.compression_algorithm {
                    NetworkSettingsPacketCompressionAlgorithm::Deflate => {
                        self.transport
                            .set_compression(true, 7, settings.compression_threshold);
                    }
                    NetworkSettingsPacketCompressionAlgorithm::Snappy => {
                        return Err(ProtocolError::UnexpectedHandshake(
                            "Snappy compression is not supported".into(),
                        )
                        .into());
                    }
                    NetworkSettingsPacketCompressionAlgorithm::Unknown(value) => {
                        return Err(ProtocolError::UnexpectedHandshake(format!(
                            "Unknown compression algorithm {}",
                            value
                        ))
                        .into());
                    }
                }

                tracing::debug!("Network settings received, enabled compression");

                Ok(BedrockStream {
                    transport: self.transport,
                    state: Login {
                        config: self.state.config,
                    },
                    _role: PhantomData,
                })
            }
            _ => Err(ProtocolError::UnexpectedHandshake("Expected NetworkSettings".into()).into()),
        }
    }

    /// Helper: Orchestrates the entire login sequence.
    ///
    /// Returns both the stream in Play state and the captured [`GameData`].
    pub async fn join(
        self,
        config: ClientHandshakeConfig,
    ) -> Result<(BedrockStream<Play, Client, T>, GameData), JolyneError> {
        let key = config.identity_key.clone();

        // 1. Settings
        let login = self.request_settings().await?;

        // 2. Login
        let secure = login.send_login(&config).await?;

        // 3. Encryption
        let packs = secure.await_handshake(&key).await?;

        // 4. Resource Packs
        let start = packs.handle_packs().await?;

        // 5. Start Game - returns (stream, game_data)
        start.await_start_game().await
    }
}

// --- State: Login ---

impl<T: Transport> BedrockStream<Login, Client, T> {
    #[instrument(skip_all, level = "trace", fields(uuid = %config.uuid, display_name = %config.display_name))]
    pub async fn send_login(
        mut self,
        config: &ClientHandshakeConfig,
    ) -> Result<BedrockStream<SecurePending, Client, T>, JolyneError> {
        // Generate JWT Chain - use Xbox Live auth if credentials provided
        let (chain, client_token) = if let Some(xbl) = &config.xbl_credentials {
            // Get Mojang-signed chain from Minecraft authentication service
            tracing::debug!("Requesting Mojang-signed authentication chain...");
            let mojang_chain = crate::auth::client::request_minecraft_chain(
                &config.identity_key,
                &xbl.token,
                &xbl.user_hash,
            )
            .await?;
            tracing::debug!("Got Mojang chain, encoding login request");

            // Encode the login request with the Mojang chain
            crate::auth::client::encode_with_mojang_chain(
                &config.identity_key,
                &config.display_name,
                config.uuid,
                &mojang_chain,
            )?
        } else {
            crate::auth::client::generate_self_signed_chain(
                &config.identity_key,
                &config.display_name,
                config.uuid,
            )?
        };

        let login_pkt = LoginPacket {
            protocol_version: crate::valentine::PROTOCOL_VERSION,
            tokens: crate::valentine::LoginTokens {
                identity: chain,
                client: client_token,
            },
        };
        self.transport
            .send_batch(&[McpePacket::from(login_pkt)])
            .await?;

        tracing::debug!("Login packet sent");

        Ok(BedrockStream {
            transport: self.transport,
            state: SecurePending {
                config: None, // Client doesn't store config in state for now
            },
            _role: PhantomData,
        })
    }
}

// --- State: SecurePending ---

#[derive(Debug, Deserialize)]
struct ServerHandshakeClaims {
    salt: String,
}

impl<T: Transport> BedrockStream<SecurePending, Client, T> {
    #[instrument(skip_all, level = "trace")]
    pub async fn await_handshake(
        mut self,
        client_identity_key: &SecretKey,
    ) -> Result<BedrockStream<ResourcePacks, Client, T>, JolyneError> {
        tracing::debug!("Waiting for ServerToClientHandshake...");
        let next_pkt = self.transport.recv_packet().await?;
        tracing::debug!("Received packet ID: {:?}", next_pkt.data.packet_id());

        match next_pkt.data {
            McpePacketData::PacketServerToClientHandshake(hs) => {
                tracing::debug!("Processing ServerToClientHandshake");
                // 1. Decode Header to find Server Public Key (x5u)
                let header = decode_header(&hs.token).map_err(|e| {
                    ProtocolError::UnexpectedHandshake(format!("Invalid JWT Header: {}", e))
                })?;

                let x5u = header.x5u.clone().ok_or_else(|| {
                    ProtocolError::UnexpectedHandshake(
                        "Missing x5u (Server Public Key) in handshake token".into(),
                    )
                })?;

                let server_der = STANDARD.decode(&x5u).map_err(|e| {
                    ProtocolError::UnexpectedHandshake(format!("Invalid base64 key: {}", e))
                })?;

                let server_pub = PublicKey::from_public_key_der(&server_der).map_err(|e| {
                    ProtocolError::UnexpectedHandshake(format!("Invalid server public key: {}", e))
                })?;

                // 2. Verify Token (Manually using p384, as jsonwebtoken fails with these keys)
                let parts: Vec<&str> = hs.token.split('.').collect();
                if parts.len() != 3 {
                    return Err(
                        ProtocolError::UnexpectedHandshake("Invalid JWT format".into()).into(),
                    );
                }

                let signed_part = format!("{}.{}", parts[0], parts[1]);
                let signature_bytes = URL_SAFE_NO_PAD.decode(parts[2]).map_err(|e| {
                    ProtocolError::UnexpectedHandshake(format!("Invalid signature base64: {}", e))
                })?;

                let signature = Signature::try_from(signature_bytes.as_slice()).map_err(|e| {
                    ProtocolError::UnexpectedHandshake(format!("Invalid signature length: {}", e))
                })?;

                let verifying_key = VerifyingKey::from(&server_pub);

                if let Err(e) = verifying_key.verify(signed_part.as_bytes(), &signature) {
                    tracing::error!("Handshake Signature Verification Failed: {}", e);
                    return Err(ProtocolError::UnexpectedHandshake(format!(
                        "Invalid handshake token signature: {}",
                        e
                    ))
                    .into());
                }

                // Decode Payload
                let payload_json = URL_SAFE_NO_PAD.decode(parts[1]).map_err(|e| {
                    ProtocolError::UnexpectedHandshake(format!("Invalid payload base64: {}", e))
                })?;

                let token_data: ServerHandshakeClaims = serde_json::from_slice(&payload_json)
                    .map_err(|e| {
                        ProtocolError::UnexpectedHandshake(format!("Invalid payload JSON: {}", e))
                    })?;

                // Try standard base64 first (with padding), fall back to no-pad
                let salt = STANDARD
                    .decode(&token_data.salt)
                    .or_else(|_| STANDARD_NO_PAD.decode(&token_data.salt))
                    .map_err(|e| {
                        ProtocolError::UnexpectedHandshake(format!("Invalid salt base64: {}", e))
                    })?;

                // 3. ECDH Shared Secret
                let shared_secret = p384::ecdh::diffie_hellman(
                    client_identity_key.to_nonzero_scalar(),
                    server_pub.as_affine(),
                );
                let shared_bytes = shared_secret.raw_secret_bytes();

                // 4. Derive Key & IV
                let mut h = Sha256::new();
                h.update(&salt);
                h.update(shared_bytes);
                let key_bytes = h.finalize();

                let key = aes_gcm::Key::<Aes256Gcm>::from_slice(&key_bytes);
                let mut iv = [0u8; 12];
                iv.copy_from_slice(&key_bytes[0..12]);

                // 5. Send ClientToServerHandshake (Ack)
                // Note: This must be sent BEFORE enabling encryption?
                // Bedrock: Server sends Handshake (Unencrypted) -> Client sends Handshake (Unencrypted?? or Encrypted?)
                // Usually Client enables encryption immediately after sending the packet, OR the packet itself is encrypted?
                // Standard: Server sends Handshake. Client computes key. Client sends Handshake (Encrypted? No, usually unencrypted then switches).
                // Let's check `server.rs`.
                // Server: Sends Handshake. Enables Encryption. Waits for Handshake.
                // So Server expects the Client's Ack to be ENCRYPTED.

                // Client side:
                // 1. Recv Handshake (Unencrypted).
                // 2. Compute Key.
                // 3. Enable Encryption.
                // 4. Send Handshake (Encrypted).

                // Let's verify `server.rs` flow:
                // 3. Send ServerToClientHandshake
                // 4. Enable Encryption locally
                // 5. Wait for ClientToServerHandshake

                // Yes, Server enables encryption right after sending. So it expects the NEXT packet (Ack) to be encrypted.
                // So Client must enable encryption BEFORE sending Ack.

                tracing::debug!("Enabling encryption...");
                self.transport.enable_encryption(*key, iv);

                tracing::debug!("Sending ClientToServerHandshake...");
                let ack = ClientToServerHandshakePacket {};
                self.transport.send_batch(&[McpePacket::from(ack)]).await?;
                tracing::debug!("ClientToServerHandshake sent");

                // 6. Wait for PlayStatus::LoginSuccess (Encrypted)
                // Note: Some servers (like LBSG) send ResourcePacksInfo BEFORE PlayStatus,
                // so we need to handle both orders.
                tracing::debug!("Waiting for PlayStatus (may receive ResourcePacksInfo first)...");

                let mut received_play_status = false;
                let mut early_resource_packs_info: Option<McpePacket> = None;

                // Loop until we get PlayStatus (LoginSuccess)
                while !received_play_status {
                    let pkt = self.transport.recv_packet().await?;
                    tracing::debug!("Received packet: {:?}", pkt.data.packet_id());

                    match &pkt.data {
                        McpePacketData::PacketPlayStatus(status) => {
                            tracing::debug!("Received PlayStatus: {:?}", status.status);
                            if status.status != PlayStatusPacketStatus::LoginSuccess {
                                return Err(ProtocolError::UnexpectedHandshake(format!(
                                    "Login failed: {:?}",
                                    status.status
                                ))
                                .into());
                            }
                            received_play_status = true;
                        }
                        McpePacketData::PacketResourcePacksInfo(_) => {
                            // LBSG sends ResourcePacksInfo before PlayStatus
                            tracing::debug!("Received early ResourcePacksInfo (before PlayStatus)");
                            early_resource_packs_info = Some(pkt);
                        }
                        _ => {
                            // Ignore other packets during handshake
                            tracing::debug!(
                                "Ignoring packet during handshake: {:?}",
                                pkt.data.packet_id()
                            );
                        }
                    }
                }

                // Send ClientCacheStatus AFTER PlayStatus - tells server we're ready for ResourcePacksInfo
                tracing::debug!("Sending ClientCacheStatus (enabled=false)...");
                let cache_status = ClientCacheStatusPacket { enabled: false };
                self.transport
                    .send_batch(&[McpePacket::from(cache_status)])
                    .await?;
                tracing::debug!("ClientCacheStatus sent");

                tracing::debug!("Handshake complete, encryption active");

                // Store early ResourcePacksInfo in stream state if received
                return Ok(BedrockStream {
                    transport: self.transport,
                    state: ResourcePacks {
                        early_packet: early_resource_packs_info,
                    },
                    _role: PhantomData,
                });
            }
            McpePacketData::PacketPlayStatus(status) => {
                // Encryption skipped by server?
                use crate::valentine::PlayStatusPacketStatus;
                if status.status != PlayStatusPacketStatus::LoginSuccess {
                    return Err(ProtocolError::UnexpectedHandshake(format!(
                        "Login failed: {:?}",
                        status.status
                    ))
                    .into());
                }
                self.transport
                    .send_batch(&[McpePacket::from(ClientCacheStatusPacket { enabled: false })])
                    .await?;
            }
            _ => {
                return Err(ProtocolError::UnexpectedHandshake(
                    "Expected ServerToClientHandshake or LoginSuccess".into(),
                )
                .into());
            }
        }

        Ok(BedrockStream {
            transport: self.transport,
            state: ResourcePacks { early_packet: None },
            _role: PhantomData,
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::batch::{decode_batch, encode_batch_multi};
    use crate::stream::transport::{BedrockTransport, TransportMessage, TransportRecvMessage};
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
    }

    impl ScriptedTransport {
        fn new(inbound: Vec<Bytes>, sent: Arc<Mutex<Vec<TransportMessage>>>) -> Self {
            Self {
                inbound: inbound
                    .into_iter()
                    .map(TransportRecvMessage::Contiguous)
                    .collect(),
                sent,
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
            Poll::Ready(self.get_mut().inbound.pop_front().map(Ok))
        }

        fn peer_addr(&self) -> SocketAddr {
            SocketAddr::new(IpAddr::V4(Ipv4Addr::LOCALHOST), 0)
        }
    }

    fn compressed_frame(packet: McpePacket) -> Bytes {
        encode_batch_multi(&[packet], true, 0, 0, true).expect("encode packet")
    }

    #[tokio::test]
    async fn unencrypted_login_success_sends_client_cache_status_before_resource_packs() {
        let sent = Arc::new(Mutex::new(Vec::new()));
        let inbound = vec![compressed_frame(McpePacket::from(
            crate::valentine::PlayStatusPacket {
                status: PlayStatusPacketStatus::LoginSuccess,
            },
        ))];

        let mut transport = BedrockTransport::new(ScriptedTransport::new(inbound, sent.clone()));
        transport.set_compression(true, 0, 0);
        let stream = BedrockStream {
            transport,
            state: SecurePending { config: None },
            _role: PhantomData,
        };

        let _packs = stream
            .await_handshake(&SecretKey::random(&mut rand::thread_rng()))
            .await
            .expect("unencrypted LoginSuccess should advance to resource packs");

        let sent = sent.lock().expect("sent lock");
        assert_eq!(sent.len(), 1, "client must send ClientCacheStatus");

        let mut frame = sent[0].buffer.clone();
        let decoded = decode_batch(
            &mut frame,
            &valentine::bedrock::context::BedrockSession { shield_item_id: 0 },
            true,
            None,
        )
        .expect("decode ClientCacheStatus frame");

        assert!(matches!(
            decoded.as_slice(),
            [McpePacket {
                data: McpePacketData::PacketClientCacheStatus(status),
                ..
            }] if !status.enabled
        ));
    }
}

// --- State: ResourcePacks ---

impl<T: Transport> BedrockStream<ResourcePacks, Client, T> {
    #[instrument(skip_all, level = "trace")]
    pub async fn handle_packs(
        mut self,
    ) -> Result<BedrockStream<StartGame, Client, T>, JolyneError> {
        // Check if we already received ResourcePacksInfo during handshake (LBSG sends it early)
        let info_pkt = if let Some(early) = self.state.early_packet.take() {
            tracing::debug!("Using early ResourcePacksInfo received during handshake");
            early
        } else {
            tracing::debug!("Waiting for ResourcePacksInfo (with 30s timeout)...");
            // Loop to handle any unexpected packets, with a timeout
            let timeout_duration = std::time::Duration::from_secs(30);
            let start = std::time::Instant::now();

            loop {
                // Check timeout
                if start.elapsed() > timeout_duration {
                    tracing::error!(
                        "Timeout waiting for ResourcePacksInfo after {:?}",
                        start.elapsed()
                    );
                    return Err(ProtocolError::UnexpectedHandshake(
                        "Timeout waiting for ResourcePacksInfo".into(),
                    )
                    .into());
                }

                // Use tokio timeout for the recv - use raw packet to catch any packets
                let recv_result = tokio::time::timeout(
                    std::time::Duration::from_secs(5),
                    self.transport.recv_packet_raw(),
                )
                .await;

                match recv_result {
                    Ok(Ok(raw_pkt)) => {
                        let packet_id = raw_pkt.id;
                        let body_len = raw_pkt.body().len();
                        tracing::debug!(
                            "Received raw packet: {:?} (body_len={})",
                            packet_id,
                            body_len
                        );

                        // Try to decode
                        let pkt = match raw_pkt.decode(&self.transport.session) {
                            Ok(pkt) => pkt,
                            Err(e) => {
                                tracing::warn!("Failed to decode packet {:?}: {:?}", packet_id, e);
                                continue;
                            }
                        };

                        match &pkt.data {
                            McpePacketData::PacketResourcePacksInfo(_) => break pkt,
                            McpePacketData::PacketDisconnect(dc) => {
                                tracing::warn!("Server disconnected: {:?}", dc.reason);
                                return Err(ProtocolError::UnexpectedHandshake(format!(
                                    "Server disconnected: {:?}",
                                    dc.reason
                                ))
                                .into());
                            }
                            _ => {
                                tracing::debug!(
                                    "Ignoring unexpected packet while waiting for ResourcePacksInfo: {:?}",
                                    pkt.data.packet_id()
                                );
                            }
                        }
                    }
                    Ok(Err(e)) => {
                        tracing::error!("Error receiving packet: {:?}", e);
                        return Err(e);
                    }
                    Err(_) => {
                        tracing::debug!("No packet received in 5s, still waiting...");
                    }
                }
            }
        };

        // Extract pack info for logging
        if let McpePacketData::PacketResourcePacksInfo(ref info) = info_pkt.data {
            tracing::debug!(
                "ResourcePacksInfo: must_accept={}, texture_packs={}",
                info.must_accept,
                info.texture_packs.len()
            );
            for pack in &info.texture_packs {
                tracing::debug!("  Pack: {} v{}", pack.uuid, pack.version);
            }

            if info.must_accept && !info.texture_packs.is_empty() {
                return Err(ProtocolError::UnexpectedHandshake(
                    "Required resource pack downloads are not implemented".into(),
                )
                .into());
            }
        }

        // For now, claim we have all packs (don't download any)
        // This is equivalent to gophertunnel's "AllPacksDownloaded" response
        tracing::debug!("Sending HaveAllPacks response...");
        let resp = ResourcePackClientResponsePacket {
            response_status: ResourcePackClientResponsePacketResponseStatus::HaveAllPacks,
            resourcepackids: vec![],
        };
        self.transport.send_batch(&[McpePacket::from(resp)]).await?;

        // Wait for ResourcePackStack
        tracing::debug!("Waiting for ResourcePackStack...");
        let stack_pkt = tokio::time::timeout(
            std::time::Duration::from_secs(30),
            self.transport.recv_packet(),
        )
        .await
        .map_err(|_| {
            ProtocolError::UnexpectedHandshake("Timeout waiting for ResourcePackStack".into())
        })??;

        if let McpePacketData::PacketResourcePackStack(ref stack) = stack_pkt.data {
            tracing::debug!(
                "ResourcePackStack: must_accept={}, game_version={}, resource_packs={}",
                stack.must_accept,
                stack.game_version,
                stack.resource_packs.len()
            );
            for pack in &stack.resource_packs {
                tracing::debug!("  Stack pack: {} v{}", pack.uuid, pack.version);
            }
        } else {
            tracing::warn!(
                "Expected ResourcePackStack, got: {:?}",
                stack_pkt.data.packet_id()
            );
        }

        // Send Completed to finish resource pack negotiation
        tracing::debug!("Sending Completed response...");
        let complete = ResourcePackClientResponsePacket {
            response_status: ResourcePackClientResponsePacketResponseStatus::Completed,
            resourcepackids: vec![],
        };
        self.transport
            .send_batch(&[McpePacket::from(complete)])
            .await?;

        tracing::debug!("Resource packs negotiated successfully");

        Ok(BedrockStream {
            transport: self.transport,
            state: StartGame,
            _role: PhantomData,
        })
    }
}

// --- State: StartGame ---

impl<T: Transport> BedrockStream<StartGame, Client, T> {
    /// Awaits the start game sequence and captures all game data packets.
    ///
    /// Returns both the stream in Play state and the captured [`GameData`].
    #[instrument(skip_all, level = "trace")]
    pub async fn await_start_game(
        mut self,
    ) -> Result<(BedrockStream<Play, Client, T>, GameData), JolyneError> {
        let mut runtime_entity_id: Option<i64> = None;
        let mut sent_chunk_radius = false;

        // Captured game data
        let mut start_game: Option<StartGamePacket> = None;
        let mut item_registry: Option<ItemRegistryPacket> = None;
        let mut biome_definitions: Option<BiomeDefinitionListPacket> = None;
        let mut entity_identifiers: Option<AvailableEntityIdentifiersPacket> = None;
        let mut creative_content: Option<CreativeContentPacket> = None;

        tracing::debug!("Waiting for StartGame sequence...");

        // 1. Receive StartGame -> Request Radius -> Receive Spawn
        // Use raw packet receiving to handle decode errors gracefully
        let start_time = std::time::Instant::now();
        loop {
            // Log periodic status
            if start_time.elapsed().as_secs().is_multiple_of(10)
                && start_time.elapsed().as_secs() > 0
            {
                tracing::debug!(
                    "Still waiting for StartGame... elapsed={:?}",
                    start_time.elapsed()
                );
            }

            if start_time.elapsed() > std::time::Duration::from_secs(120) {
                return Err(ProtocolError::UnexpectedHandshake(
                    "Timeout waiting for PlayerSpawn during StartGame".into(),
                )
                .into());
            }

            // Use timeout to prevent individual receives from blocking forever
            let recv_result = tokio::time::timeout(
                std::time::Duration::from_secs(5),
                self.transport.recv_packet_raw(),
            )
            .await;

            let raw_pkt = match recv_result {
                Ok(Ok(pkt)) => pkt,
                Ok(Err(e)) => return Err(e),
                Err(_) => {
                    tracing::debug!("No packet received in 5s during StartGame, still waiting...");
                    continue;
                }
            };

            // Try to decode the packet - skip on decode errors for non-essential packets
            let packet_id = raw_pkt.id;
            let pkt = match raw_pkt.decode(&self.transport.session) {
                Ok(pkt) => pkt,
                Err(e) => {
                    tracing::warn!(
                        packet_id = ?packet_id,
                        "Skipping packet due to decode error: {:?}",
                        e
                    );
                    continue;
                }
            };

            match pkt.data {
                McpePacketData::PacketStartGame(start) => {
                    tracing::debug!(runtime_id = %start.runtime_entity_id, "StartGame received");
                    runtime_entity_id = Some(start.runtime_entity_id);
                    start_game = Some(*start);
                }
                McpePacketData::PacketItemRegistry(registry) => {
                    tracing::debug!(items = %registry.itemstates.len(), "ItemRegistry received");
                    item_registry = Some(registry);
                    if !sent_chunk_radius {
                        let req = RequestChunkRadiusPacket {
                            chunk_radius: 4,
                            max_radius: 32,
                        };
                        self.transport.send_batch(&[McpePacket::from(req)]).await?;
                        sent_chunk_radius = true;
                    }
                }
                McpePacketData::PacketBiomeDefinitionList(biomes) => {
                    tracing::debug!(biomes = %biomes.biome_definitions.len(), "BiomeDefinitionList received");
                    biome_definitions = Some(biomes);
                }
                McpePacketData::PacketAvailableEntityIdentifiers(entities) => {
                    tracing::debug!("AvailableEntityIdentifiers received");
                    entity_identifiers = Some(entities);
                }
                McpePacketData::PacketCreativeContent(content) => {
                    tracing::debug!(
                        groups = %content.groups.len(),
                        items = %content.items.len(),
                        "CreativeContent received"
                    );
                    creative_content = Some(content);
                }
                McpePacketData::PacketPlayStatus(status) => {
                    tracing::debug!("PlayStatus received: {:?}", status.status);
                    if status.status == PlayStatusPacketStatus::PlayerSpawn {
                        tracing::debug!("PlayerSpawn received");
                        break;
                    }
                }
                McpePacketData::PacketDisconnect(dc) => {
                    tracing::warn!("Server disconnected: {:?}", dc.reason);
                    return Err(ProtocolError::UnexpectedHandshake(format!(
                        "Server disconnected during StartGame: {:?}",
                        dc.reason
                    ))
                    .into());
                }
                _ => {
                    tracing::debug!("StartGame: ignoring packet {:?}", pkt.data.packet_id());
                }
            }
        }

        // 2. Send Loading Screen (Start & End)
        self.transport
            .send_batch(&[
                McpePacket::from(ServerboundLoadingScreenPacket {
                    type_: 1,
                    loading_screen_id: None,
                }),
                McpePacket::from(ServerboundLoadingScreenPacket {
                    type_: 2,
                    loading_screen_id: None,
                }),
            ])
            .await?;

        // 3. Send Initialized
        if let Some(rid) = runtime_entity_id {
            self.transport
                .send_batch(&[McpePacket::from(SetLocalPlayerAsInitializedPacket {
                    runtime_entity_id: rid,
                })])
                .await?;
        }

        // Build GameData from captured packets
        let game_data = GameData {
            start_game: start_game.ok_or_else(|| {
                ProtocolError::UnexpectedHandshake("Never received StartGame packet".into())
            })?,
            item_registry: item_registry.ok_or_else(|| {
                ProtocolError::UnexpectedHandshake("Never received ItemRegistry packet".into())
            })?,
            biome_definitions,
            entity_identifiers,
            creative_content,
        };

        tracing::debug!("Game initialization complete, entering Play state");

        Ok((
            BedrockStream {
                transport: self.transport,
                state: Play,
                _role: PhantomData,
            },
            game_data,
        ))
    }
}

// --- State: Play ---

impl<T: Transport> BedrockStream<Play, Client, T> {
    /// Receive the next packet as a borrowed protocol view.
    #[instrument(skip_all, level = "trace")]
    pub async fn recv_packet_borrowed(
        &mut self,
    ) -> Result<crate::valentine::BorrowedMcpePacket, JolyneError> {
        self.transport.recv_packet_borrowed().await
    }

    /// Receive the next packet from the server.
    ///
    /// This materializes an owned packet. Prefer [`Self::recv_packet_borrowed`]
    /// when the caller can stay on borrowed packet data.
    #[instrument(skip_all, level = "trace")]
    pub async fn recv_packet(&mut self) -> Result<McpePacket, JolyneError> {
        self.transport.recv_packet().await
    }

    /// Send a packet to the server.
    #[instrument(skip_all, level = "trace")]
    pub async fn send_packet(&mut self, packet: McpePacket) -> Result<(), JolyneError> {
        self.transport.send(packet).await
    }
}
