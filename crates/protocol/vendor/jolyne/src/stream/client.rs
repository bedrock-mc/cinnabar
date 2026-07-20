#![allow(clippy::items_after_test_module)]

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

use crate::batch::BatchCompression;
use crate::error::{JolyneError, ProtocolError};
use crate::gamedata::GameData;
use crate::raw::{MAX_RAW_BATCH_PACKETS, RawPacket};
#[cfg(feature = "raknet")]
use crate::stream::transport::RakNetTransport;
use crate::stream::{
    BedrockStream, Client, Handshake, Login, Play, ResourcePacks, SecurePending, StartGame,
    transport::{BedrockTransport, Transport},
};
use crate::valentine::BorrowedMcpePacketData;
use crate::valentine::{
    ClientCacheStatusPacket, ClientToServerHandshakePacket, ItemRegistryPacket, LoginPacket,
    PlayStatusPacketStatus, RequestChunkRadiusPacket, RequestNetworkSettingsPacket,
    ResourcePackClientResponsePacket, ResourcePackClientResponsePacketResponseStatus,
    ServerboundLoadingScreenPacket, SetLocalPlayerAsInitializedPacket, StartGamePacket,
};
use crate::valentine::{
    McpePacket, McpePacketData, McpePacketName, NetworkSettingsPacketCompressionAlgorithm,
};

const DEFAULT_LOGIN_TIMEOUT: std::time::Duration = std::time::Duration::from_secs(120);
const MAX_DEFERRED_PACKET_BYTES: usize = 16 * 1024 * 1024;
const EXEMPTED_RESOURCE_PACKS: &[(&str, &str)] = &[
    ("0fba4063-dba1-4281-9b89-ff9390653530", "1.0.0"),
    ("b41c2785-c512-4a49-af56-3a87afd47c57", "1.21.30"),
    ("a4df0cb3-17be-4163-88d7-fcf7002b935d", "1.21.20"),
    ("d19adffe-a2e1-4b02-8436-ca4583368c89", "1.21.10"),
    ("85d5603d-2824-4b21-8044-34f441f4fce1", "1.21.0"),
    ("e977cd13-0a11-4618-96fb-03dfe9c43608", "1.20.60"),
    ("0674721c-a0aa-41a1-9ba8-1ed33ea3e7ed", "1.20.50"),
];

fn is_exempted_resource_pack(uuid: &str, version: &str) -> bool {
    EXEMPTED_RESOURCE_PACKS
        .iter()
        .any(|&(known_uuid, known_version)| uuid == known_uuid && version == known_version)
}

#[derive(Default)]
struct DeferredPackets {
    packets: Vec<RawPacket>,
    bytes: usize,
}

impl DeferredPackets {
    fn push(&mut self, packet: RawPacket) -> Result<(), JolyneError> {
        if self.packets.len() == MAX_RAW_BATCH_PACKETS {
            return Err(ProtocolError::TooManyPackets {
                max: MAX_RAW_BATCH_PACKETS,
            }
            .into());
        }

        let bytes = self.bytes.saturating_add(packet.inner_frame().len());
        if bytes > MAX_DEFERRED_PACKET_BYTES {
            return Err(ProtocolError::BatchTooLarge {
                actual: bytes,
                max: MAX_DEFERRED_PACKET_BYTES,
            }
            .into());
        }

        self.bytes = bytes;
        self.packets.push(packet.into_compact());
        Ok(())
    }

    fn into_packets(self) -> Vec<RawPacket> {
        self.packets
    }
}

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
    /// Advertise the Bedrock client blob cache only when the caller installed a resolver.
    pub client_cache_enabled: bool,
    /// Exact decoded skin authority encoded into this session's ClientData JWT.
    pub advertised_skin: crate::auth::client::AdvertisedSkin,
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
            client_cache_enabled: false,
            advertised_skin: crate::auth::client::default_advertised_skin(),
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
            client_cache_enabled: false,
            advertised_skin: crate::auth::client::default_advertised_skin(),
        }
    }

    /// Binds login negotiation to the caller's installed cache resolver.
    #[must_use]
    pub fn with_client_cache_enabled(mut self, enabled: bool) -> Self {
        self.client_cache_enabled = enabled;
        self
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

        let settings_raw = self.transport.recv_packet_raw().await?;
        if settings_raw.id != McpePacketName::PacketNetworkSettings {
            return Err(ProtocolError::UnexpectedHandshake(format!(
                "Expected NetworkSettings, got {:?}",
                settings_raw.id
            ))
            .into());
        }
        let settings_pkt = settings_raw.decode_borrowed()?;

        match settings_pkt.data {
            BorrowedMcpePacketData::PacketNetworkSettings(settings) => {
                match settings.compression_algorithm {
                    NetworkSettingsPacketCompressionAlgorithm::Deflate => {
                        self.transport.set_compression_algorithm(
                            true,
                            BatchCompression::Deflate,
                            7,
                            settings.compression_threshold,
                        );
                    }
                    NetworkSettingsPacketCompressionAlgorithm::Snappy => {
                        self.transport.set_compression_algorithm(
                            true,
                            BatchCompression::Snappy,
                            1,
                            settings.compression_threshold,
                        );
                    }
                    NetworkSettingsPacketCompressionAlgorithm::Unknown(value)
                        if value == u16::MAX =>
                    {
                        self.transport.set_compression_algorithm(
                            true,
                            BatchCompression::None,
                            0,
                            settings.compression_threshold,
                        );
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
        self.join_with_timeout(config, DEFAULT_LOGIN_TIMEOUT).await
    }

    /// Orchestrates login with one deadline spanning every protocol phase.
    pub async fn join_with_timeout(
        self,
        config: ClientHandshakeConfig,
        timeout: std::time::Duration,
    ) -> Result<(BedrockStream<Play, Client, T>, GameData), JolyneError> {
        tokio::time::timeout(timeout, self.join_inner(config))
            .await
            .map_err(|_| {
                ProtocolError::UnexpectedHandshake(format!(
                    "login deadline exceeded after {timeout:?}"
                ))
            })?
    }

    async fn join_inner(
        self,
        config: ClientHandshakeConfig,
    ) -> Result<(BedrockStream<Play, Client, T>, GameData), JolyneError> {
        let key = config.identity_key.clone();

        // 1. Settings
        let login = self.request_settings().await?;

        // 2. Login
        let secure = login.send_login(&config).await?;

        // 3. Encryption
        let packs = secure
            .await_handshake_with_client_cache(&key, config.client_cache_enabled)
            .await?;

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
            crate::auth::client::encode_with_mojang_chain_and_skin(
                &config.identity_key,
                &config.display_name,
                config.uuid,
                &mojang_chain,
                &config.advertised_skin,
            )?
        } else {
            crate::auth::client::generate_self_signed_chain_with_skin(
                &config.identity_key,
                &config.display_name,
                config.uuid,
                &config.advertised_skin,
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

fn observe_login_success_packet(
    packet: McpePacket,
    early_resource_packs_info: &mut Option<McpePacket>,
) -> Result<bool, JolyneError> {
    if let McpePacketData::PacketPlayStatus(status) = &packet.data {
        if status.status != PlayStatusPacketStatus::LoginSuccess {
            return Err(ProtocolError::UnexpectedHandshake(format!(
                "Login failed: {:?}",
                status.status
            ))
            .into());
        }
        return Ok(true);
    }
    if let McpePacketData::PacketDisconnect(disconnect) = &packet.data {
        return Err(ProtocolError::UnexpectedHandshake(format!(
            "Server disconnected during login: {:?}",
            disconnect.reason
        ))
        .into());
    }
    if matches!(&packet.data, McpePacketData::PacketResourcePacksInfo(_)) {
        *early_resource_packs_info = Some(packet);
    }
    Ok(false)
}

impl<T: Transport> BedrockStream<SecurePending, Client, T> {
    #[instrument(skip_all, level = "trace")]
    pub async fn await_handshake(
        self,
        client_identity_key: &SecretKey,
    ) -> Result<BedrockStream<ResourcePacks, Client, T>, JolyneError> {
        self.await_handshake_with_client_cache(client_identity_key, false)
            .await
    }

    /// Completes encryption and advertises cache support only for an installed resolver.
    #[instrument(skip_all, level = "trace")]
    pub async fn await_handshake_with_client_cache(
        mut self,
        client_identity_key: &SecretKey,
        client_cache_enabled: bool,
    ) -> Result<BedrockStream<ResourcePacks, Client, T>, JolyneError> {
        tracing::debug!("Waiting for ServerToClientHandshake...");
        let next_raw = self.transport.recv_packet_raw().await?;
        if !matches!(
            next_raw.id,
            McpePacketName::PacketServerToClientHandshake
                | McpePacketName::PacketPlayStatus
                | McpePacketName::PacketDisconnect
        ) {
            return Err(ProtocolError::UnexpectedHandshake(format!(
                "Expected ServerToClientHandshake or LoginSuccess, got {:?}",
                next_raw.id
            ))
            .into());
        }
        let next_pkt = next_raw.decode(&self.transport.session)?;
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
                let mut parts = hs.token.split('.');
                let protected = parts.next();
                let claims = parts.next();
                let signature = parts.next();
                if protected.is_none()
                    || claims.is_none()
                    || signature.is_none()
                    || parts.next().is_some()
                {
                    return Err(
                        ProtocolError::UnexpectedHandshake("Invalid JWT format".into()).into(),
                    );
                }
                let protected = protected.expect("checked above");
                let claims = claims.expect("checked above");
                let signature = signature.expect("checked above");

                let signed_part = format!("{protected}.{claims}");
                let signature_bytes = URL_SAFE_NO_PAD.decode(signature).map_err(|e| {
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
                let payload_json = URL_SAFE_NO_PAD.decode(claims).map_err(|e| {
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
                    let raw = self.transport.recv_packet_raw().await?;
                    tracing::debug!("Received packet: {:?}", raw.id);
                    if matches!(
                        raw.id,
                        McpePacketName::PacketPlayStatus
                            | McpePacketName::PacketResourcePacksInfo
                            | McpePacketName::PacketDisconnect
                    ) {
                        let packet = raw.decode(&self.transport.session)?;
                        received_play_status =
                            observe_login_success_packet(packet, &mut early_resource_packs_info)?;
                    } else {
                        tracing::debug!("Ignoring packet ID during login handshake: {:?}", raw.id);
                    }
                }

                // Send ClientCacheStatus AFTER PlayStatus - tells server we're ready for ResourcePacksInfo
                tracing::debug!(
                    enabled = client_cache_enabled,
                    "Sending ClientCacheStatus..."
                );
                let cache_status = ClientCacheStatusPacket {
                    enabled: client_cache_enabled,
                };
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
                    .send_batch(&[McpePacket::from(ClientCacheStatusPacket {
                        enabled: client_cache_enabled,
                    })])
                    .await?;
            }
            McpePacketData::PacketDisconnect(disconnect) => {
                return Err(ProtocolError::UnexpectedHandshake(format!(
                    "Server disconnected during login: {:?}",
                    disconnect.reason
                ))
                .into());
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
    use bytes::{BufMut, Bytes, BytesMut};
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

    struct PendingTransport;

    impl Transport for PendingTransport {
        type Error = io::Error;

        const USES_BATCH_PREFIX: bool = true;

        fn poll_send(
            self: Pin<&mut Self>,
            _cx: &mut Context<'_>,
            _msg: TransportMessage,
        ) -> Poll<Result<(), Self::Error>> {
            Poll::Ready(Ok(()))
        }

        fn poll_recv(
            self: Pin<&mut Self>,
            _cx: &mut Context<'_>,
        ) -> Poll<Option<Result<TransportRecvMessage, Self::Error>>> {
            Poll::Pending
        }

        fn peer_addr(&self) -> SocketAddr {
            SocketAddr::new(IpAddr::V4(Ipv4Addr::LOCALHOST), 0)
        }
    }

    fn compressed_frame(packet: McpePacket) -> Bytes {
        encode_batch_multi(&[packet], true, 0, 0, true).expect("encode packet")
    }

    fn uncompressed_frame(packets: &[McpePacket]) -> Bytes {
        encode_batch_multi(packets, false, 0, 0, true).expect("encode packet batch")
    }

    fn malformed_uncompressed_frame(packet_id: crate::valentine::McpePacketName) -> Bytes {
        use valentine::bedrock::codec::BedrockCodec;
        use valentine::protocol::wire;

        let mut header = BytesMut::new();
        packet_id.encode(&mut header).expect("encode packet ID");

        let mut frame = BytesMut::new();
        frame.put_u8(crate::batch::BATCH_PACKET_ID);
        wire::write_var_u32(&mut frame, header.len() as u32);
        frame.extend_from_slice(&header);
        frame.freeze()
    }

    fn start_game_stream(
        inbound: Vec<Bytes>,
    ) -> BedrockStream<StartGame, Client, ScriptedTransport> {
        let sent = Arc::new(Mutex::new(Vec::new()));
        let mut transport = BedrockTransport::new(ScriptedTransport::new(inbound, sent));
        transport.set_max_decompressed_batch_size(Some(16 * 1024 * 1024));
        BedrockStream {
            transport,
            state: StartGame,
            _role: PhantomData,
        }
    }

    fn start_game_packet() -> McpePacket {
        McpePacket::from(StartGamePacket {
            runtime_entity_id: 42,
            ..Default::default()
        })
    }

    fn spawn_completion_packets() -> [McpePacket; 3] {
        [
            McpePacket::from(ItemRegistryPacket::default()),
            McpePacket::from(crate::valentine::ChunkRadiusUpdatePacket { chunk_radius: 16 }),
            McpePacket::from(crate::valentine::PlayStatusPacket {
                status: PlayStatusPacketStatus::PlayerSpawn,
            }),
        ]
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

    #[tokio::test]
    async fn unencrypted_login_success_can_advertise_an_installed_client_cache() {
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
            .await_handshake_with_client_cache(&SecretKey::random(&mut rand::thread_rng()), true)
            .await
            .expect("cache-enabled LoginSuccess should advance to resource packs");

        let sent = sent.lock().expect("sent lock");
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
            }] if status.enabled
        ));
    }

    #[tokio::test]
    async fn request_settings_rejects_an_unexpected_raw_id_without_decoding_its_body() {
        let sent = Arc::new(Mutex::new(Vec::new()));
        let inbound = vec![malformed_uncompressed_frame(
            crate::valentine::McpePacketName::PacketSetTitle,
        )];
        let transport = BedrockTransport::new(ScriptedTransport::new(inbound, sent));
        let stream = BedrockStream {
            transport,
            state: Handshake { config: None },
            _role: PhantomData,
        };

        let error = match stream.request_settings().await {
            Ok(_) => panic!("an unexpected settings packet ID must fail"),
            Err(error) => error,
        };
        assert!(matches!(
            error,
            JolyneError::Protocol(ProtocolError::UnexpectedHandshake(ref message))
                if message.contains("PacketSetTitle")
        ));
    }

    #[tokio::test]
    async fn optional_start_game_packets_are_fifo_deferred_compact_frames() {
        let mut packets = vec![
            start_game_packet(),
            McpePacket::from(crate::valentine::SetTimePacket { time: 11 }),
            McpePacket::from(crate::valentine::BiomeDefinitionListPacket::default()),
            McpePacket::from(crate::valentine::AvailableEntityIdentifiersPacket::default()),
            McpePacket::from(crate::valentine::CreativeContentPacket::default()),
            McpePacket::from(crate::valentine::SetTimePacket { time: 22 }),
        ];
        packets.extend(spawn_completion_packets());
        let frame = uncompressed_frame(&packets);
        let allocation_start = frame.as_ptr() as usize;
        let allocation_end = allocation_start + frame.len();

        let stream = start_game_stream(vec![frame.clone()]);
        let (mut play, game_data) = stream.await_start_game().await.expect("spawn sequence");
        assert!(game_data.biome_definitions.is_none());
        assert!(game_data.entity_identifiers.is_none());
        assert!(game_data.creative_content.is_none());

        let expected = [
            crate::valentine::McpePacketName::PacketSetTime,
            crate::valentine::McpePacketName::PacketBiomeDefinitionList,
            crate::valentine::McpePacketName::PacketAvailableEntityIdentifiers,
            crate::valentine::McpePacketName::PacketCreativeContent,
            crate::valentine::McpePacketName::PacketSetTime,
        ];
        for expected_id in expected {
            let raw = play
                .transport
                .recv_packet_raw()
                .await
                .expect("deferred packet");
            assert_eq!(raw.id, expected_id, "deferred FIFO order changed");
            let pointer = raw.inner_frame().as_ptr() as usize;
            assert!(
                pointer < allocation_start || pointer >= allocation_end,
                "deferred frame still retains the full incoming batch allocation"
            );
        }
    }

    #[tokio::test]
    async fn start_game_caps_aggregate_deferred_packet_count() {
        let deferred = McpePacket::from(crate::valentine::SetTimePacket { time: 1 });
        let mut first = vec![start_game_packet()];
        first.extend(std::iter::repeat_n(deferred.clone(), 800));
        let mut second = Vec::new();
        second.extend(std::iter::repeat_n(deferred, 801));
        second.extend(spawn_completion_packets());

        let stream = start_game_stream(vec![
            uncompressed_frame(&first),
            uncompressed_frame(&second),
        ]);
        let error = match stream.await_start_game().await {
            Ok(_) => panic!("more than 1,600 deferred packets must fail"),
            Err(error) => error,
        };
        assert!(matches!(
            error,
            JolyneError::Protocol(ProtocolError::TooManyPackets { max: 1_600 })
        ));
    }

    #[tokio::test]
    async fn start_game_caps_aggregate_deferred_frame_bytes() {
        const HALF_LIMIT: usize = 8 * 1024 * 1024;
        let level_chunk = || {
            McpePacket::from(crate::valentine::LevelChunkPacket {
                payload: vec![0; HALF_LIMIT],
                ..Default::default()
            })
        };
        let first = uncompressed_frame(&[start_game_packet(), level_chunk()]);
        let mut second = vec![level_chunk()];
        second.extend(spawn_completion_packets());

        let stream = start_game_stream(vec![first, uncompressed_frame(&second)]);
        let error = match stream.await_start_game().await {
            Ok(_) => panic!("more than 16 MiB of deferred frames must fail"),
            Err(error) => error,
        };
        assert!(matches!(
            error,
            JolyneError::Protocol(ProtocolError::BatchTooLarge {
                max: 16_777_216,
                ..
            })
        ));
    }

    #[tokio::test]
    async fn non_empty_resource_pack_stack_is_rejected() {
        let info = McpePacket::from(crate::valentine::ResourcePacksInfoPacket::default());
        let stack = McpePacket::from(crate::valentine::ResourcePackStackPacket {
            resource_packs: vec![crate::valentine::ResourcePackIdVersionsItem {
                uuid: "pack-id".into(),
                version: "1.0.0".into(),
                name: "test pack".into(),
            }],
            ..Default::default()
        });
        let sent = Arc::new(Mutex::new(Vec::new()));
        let transport = BedrockTransport::new(ScriptedTransport::new(
            vec![uncompressed_frame(&[info]), uncompressed_frame(&[stack])],
            sent,
        ));
        let stream = BedrockStream {
            transport,
            state: ResourcePacks { early_packet: None },
            _role: PhantomData,
        };

        let error = match stream.handle_packs().await {
            Ok(_) => panic!("a non-empty server pack stack must not be accepted"),
            Err(error) => error,
        };
        assert!(
            error
                .to_string()
                .contains("Resource pack downloads are not implemented")
        );
    }

    #[tokio::test]
    async fn pinned_gophertunnel_exempt_pack_stack_is_accepted() {
        let info = McpePacket::from(crate::valentine::ResourcePacksInfoPacket::default());
        let (uuid, version) = EXEMPTED_RESOURCE_PACKS[0];
        let stack = McpePacket::from(crate::valentine::ResourcePackStackPacket {
            resource_packs: vec![crate::valentine::ResourcePackIdVersionsItem {
                uuid: uuid.into(),
                version: version.into(),
                name: "client built-in".into(),
            }],
            ..Default::default()
        });
        let sent = Arc::new(Mutex::new(Vec::new()));
        let transport = BedrockTransport::new(ScriptedTransport::new(
            vec![uncompressed_frame(&[info]), uncompressed_frame(&[stack])],
            sent.clone(),
        ));
        let stream = BedrockStream {
            transport,
            state: ResourcePacks { early_packet: None },
            _role: PhantomData,
        };

        stream
            .handle_packs()
            .await
            .expect("client built-in packs do not require a download");
        assert_eq!(
            sent.lock().expect("sent lock").len(),
            2,
            "HaveAllPacks and Completed must both be sent"
        );
    }

    #[tokio::test]
    async fn join_deadline_bounds_pending_network_settings() {
        let transport = BedrockTransport::new(PendingTransport);
        let stream = BedrockStream {
            transport,
            state: Handshake { config: None },
            _role: PhantomData,
        };
        let config = ClientHandshakeConfig::random(
            SocketAddr::new(IpAddr::V4(Ipv4Addr::LOCALHOST), 0),
            "deadline-test",
        );

        let error = match stream
            .join_with_timeout(config, std::time::Duration::from_millis(10))
            .await
        {
            Ok(_) => panic!("pending settings must hit the login deadline"),
            Err(error) => error,
        };
        assert!(error.to_string().contains("login deadline"));
    }

    #[test]
    fn disconnect_while_waiting_for_login_success_is_an_error() {
        let mut early = None;
        let error = observe_login_success_packet(
            McpePacket::from(crate::valentine::DisconnectPacket::default()),
            &mut early,
        )
        .expect_err("Disconnect must stop login");

        assert!(error.to_string().contains("disconnected during login"));
        assert!(early.is_none());
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
            let raw = tokio::time::timeout(
                std::time::Duration::from_secs(30),
                self.transport.recv_packet_raw(),
            )
            .await
            .map_err(|_| {
                ProtocolError::UnexpectedHandshake("Timeout waiting for ResourcePacksInfo".into())
            })??;
            match raw.id {
                McpePacketName::PacketResourcePacksInfo => raw.decode(&self.transport.session)?,
                McpePacketName::PacketDisconnect => {
                    let packet = raw.decode(&self.transport.session)?;
                    let McpePacketData::PacketDisconnect(disconnect) = packet.data else {
                        unreachable!("packet ID and decoded variant must agree")
                    };
                    return Err(ProtocolError::UnexpectedHandshake(format!(
                        "Server disconnected during resource packs: {:?}",
                        disconnect.reason
                    ))
                    .into());
                }
                other => {
                    return Err(ProtocolError::UnexpectedHandshake(format!(
                        "Expected ResourcePacksInfo, got {other:?}"
                    ))
                    .into());
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

            if !info.texture_packs.is_empty() {
                return Err(ProtocolError::UnexpectedHandshake(
                    "Resource pack downloads are not implemented".into(),
                )
                .into());
            }
        } else {
            return Err(
                ProtocolError::UnexpectedHandshake("Expected ResourcePacksInfo".into()).into(),
            );
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
        let stack_raw = tokio::time::timeout(
            std::time::Duration::from_secs(30),
            self.transport.recv_packet_raw(),
        )
        .await
        .map_err(|_| {
            ProtocolError::UnexpectedHandshake("Timeout waiting for ResourcePackStack".into())
        })??;
        let stack_pkt = match stack_raw.id {
            McpePacketName::PacketResourcePackStack => stack_raw.decode(&self.transport.session)?,
            McpePacketName::PacketDisconnect => {
                let packet = stack_raw.decode(&self.transport.session)?;
                let McpePacketData::PacketDisconnect(disconnect) = packet.data else {
                    unreachable!("packet ID and decoded variant must agree")
                };
                return Err(ProtocolError::UnexpectedHandshake(format!(
                    "Server disconnected during resource packs: {:?}",
                    disconnect.reason
                ))
                .into());
            }
            other => {
                return Err(ProtocolError::UnexpectedHandshake(format!(
                    "Expected ResourcePackStack, got {other:?}"
                ))
                .into());
            }
        };

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
            if let Some(pack) = stack
                .resource_packs
                .iter()
                .find(|pack| !is_exempted_resource_pack(&pack.uuid, &pack.version))
            {
                return Err(ProtocolError::UnexpectedHandshake(format!(
                    "Resource pack downloads are not implemented for {}_{}",
                    pack.uuid, pack.version
                ))
                .into());
            }
        } else {
            return Err(ProtocolError::UnexpectedHandshake(format!(
                "Expected ResourcePackStack, got {:?}",
                stack_pkt.data.packet_id()
            ))
            .into());
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
        let mut received_chunk_radius = false;
        let mut received_player_spawn = false;
        let mut deferred_packets = DeferredPackets::default();

        // Captured game data
        let mut start_game: Option<StartGamePacket> = None;
        let mut item_registry: Option<ItemRegistryPacket> = None;

        tracing::debug!("Waiting for StartGame sequence...");

        let start_time = std::time::Instant::now();
        loop {
            if start_time.elapsed() > std::time::Duration::from_secs(120) {
                return Err(ProtocolError::UnexpectedHandshake(
                    "Timeout waiting for PlayerSpawn during StartGame".into(),
                )
                .into());
            }

            let raw = match tokio::time::timeout(
                std::time::Duration::from_secs(5),
                self.transport.recv_packet_raw(),
            )
            .await
            {
                Ok(Ok(raw)) => raw,
                Ok(Err(e)) => return Err(e),
                Err(_) => continue,
            };
            match raw.id {
                McpePacketName::PacketStartGame => {
                    let packet = raw.decode(&self.transport.session)?;
                    let McpePacketData::PacketStartGame(start) = packet.data else {
                        unreachable!("packet ID and decoded variant must agree")
                    };
                    tracing::debug!(runtime_id = %start.runtime_entity_id, "StartGame received");
                    if let Some(existing) = runtime_entity_id {
                        if existing != start.runtime_entity_id {
                            return Err(ProtocolError::UnexpectedHandshake(format!(
                                "conflicting StartGame runtime entity ID: first {existing}, then {}",
                                start.runtime_entity_id
                            ))
                            .into());
                        }
                    } else {
                        runtime_entity_id = Some(start.runtime_entity_id);
                        start_game = Some(*start);
                    }
                }
                McpePacketName::PacketItemRegistry => {
                    let packet = raw.decode(&self.transport.session)?;
                    let McpePacketData::PacketItemRegistry(registry) = packet.data else {
                        unreachable!("packet ID and decoded variant must agree")
                    };
                    tracing::debug!(items = %registry.itemstates.len(), "ItemRegistry received");
                    if let Some(shield) = registry
                        .itemstates
                        .iter()
                        .find(|item| item.name == "minecraft:shield")
                    {
                        self.transport.session.shield_item_id = i32::from(shield.runtime_id);
                    }
                    item_registry = Some(registry);
                }
                McpePacketName::PacketPlayStatus => {
                    let packet = raw.decode(&self.transport.session)?;
                    let McpePacketData::PacketPlayStatus(status) = packet.data else {
                        unreachable!("packet ID and decoded variant must agree")
                    };
                    tracing::debug!("PlayStatus received: {:?}", status.status);
                    if status.status == PlayStatusPacketStatus::PlayerSpawn {
                        received_player_spawn = true;
                    }
                }
                McpePacketName::PacketChunkRadiusUpdate => {
                    let packet = raw.clone().decode(&self.transport.session)?;
                    let McpePacketData::PacketChunkRadiusUpdate(update) = packet.data else {
                        unreachable!("packet ID and decoded variant must agree")
                    };
                    if update.chunk_radius < 1 {
                        return Err(ProtocolError::UnexpectedHandshake(format!(
                            "invalid updated chunk radius {}",
                            update.chunk_radius
                        ))
                        .into());
                    }
                    deferred_packets.push(raw)?;
                    received_chunk_radius = true;
                }
                McpePacketName::PacketDisconnect => {
                    let packet = raw.decode(&self.transport.session)?;
                    let McpePacketData::PacketDisconnect(dc) = packet.data else {
                        unreachable!("packet ID and decoded variant must agree")
                    };
                    tracing::warn!("Server disconnected: {:?}", dc.reason);
                    return Err(ProtocolError::UnexpectedHandshake(format!(
                        "Server disconnected during StartGame: {:?}",
                        dc.reason
                    ))
                    .into());
                }
                packet_id => {
                    tracing::debug!("StartGame: deferring packet {:?}", packet_id);
                    deferred_packets.push(raw)?;
                }
            }

            if !sent_chunk_radius && start_game.is_some() {
                self.transport
                    .send_batch(&[
                        McpePacket::from(ServerboundLoadingScreenPacket {
                            type_: 1,
                            loading_screen_id: None,
                        }),
                        McpePacket::from(RequestChunkRadiusPacket {
                            chunk_radius: 16,
                            max_radius: 16,
                        }),
                    ])
                    .await?;
                sent_chunk_radius = true;
            }

            if sent_chunk_radius
                && received_chunk_radius
                && received_player_spawn
                && item_registry.is_some()
            {
                break;
            }
        }

        let runtime_entity_id = runtime_entity_id.ok_or_else(|| {
            ProtocolError::UnexpectedHandshake("Never received StartGame runtime entity ID".into())
        })?;

        self.transport
            .send_batch(&[
                McpePacket::from(ServerboundLoadingScreenPacket {
                    type_: 2,
                    loading_screen_id: None,
                }),
                McpePacket::from(SetLocalPlayerAsInitializedPacket { runtime_entity_id }),
            ])
            .await?;

        // Build GameData from captured packets
        let game_data = GameData {
            start_game: start_game.ok_or_else(|| {
                ProtocolError::UnexpectedHandshake("Never received StartGame packet".into())
            })?,
            item_registry: item_registry.ok_or_else(|| {
                ProtocolError::UnexpectedHandshake("Never received ItemRegistry packet".into())
            })?,
            biome_definitions: None,
            entity_identifiers: None,
            creative_content: None,
        };

        self.transport
            .prepend_recv_queue(deferred_packets.into_packets());

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
    /// Receive the next packet with only its header decoded.
    #[instrument(skip_all, level = "trace")]
    pub async fn recv_packet_raw(&mut self) -> Result<RawPacket, JolyneError> {
        self.transport.recv_packet_raw().await
    }

    /// Materialize a raw packet using this stream's negotiated codec context.
    pub fn decode_raw_packet(&self, packet: RawPacket) -> Result<McpePacket, JolyneError> {
        packet.decode(&self.transport.session)
    }

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
