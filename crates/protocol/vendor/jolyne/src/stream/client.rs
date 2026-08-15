#![allow(clippy::items_after_test_module)]

use std::collections::{HashMap, HashSet};
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
    BedrockStream, Client, Handshake, Login, Play, ResourcePackArchive, ResourcePackHandoff,
    ResourcePacks, SecurePending, StartGame,
    resource_pack_handoff::{
        MAX_RESOURCE_PACK_BYTES, MAX_RESOURCE_PACK_CHUNK_BYTES, MAX_RESOURCE_PACK_CHUNKS,
        MAX_RESOURCE_PACK_TOTAL_BYTES, MAX_RESOURCE_PACKS, ResourcePackContentKey,
    },
    transport::{BedrockTransport, Transport},
};
use crate::valentine::BorrowedMcpePacketData;
use crate::valentine::{
    ActorRuntimeId, ClientCacheStatusPacket, ClientToServerHandshakePacket, ItemRegistryPacket,
    LoginPacket, PlayStatusPacketStatus, RequestChunkRadiusPacket, RequestNetworkSettingsPacket,
    ResourcePackChunkRequestPacket, ResourcePackClientResponsePacket,
    ResourcePackClientResponsePacketPayloadDownloading,
    ResourcePackClientResponsePacketPayloadDownloadingFinished,
    ResourcePackClientResponsePacketPayloadResourcePackStackFinished,
    ResourcePackClientResponsePacketResponse, ServerboundLoadingScreenPacket,
    ServerboundLoadingScreenPacketLoadingScreenPacketType, SetLocalPlayerAsInitializedPacket,
    StartGamePacket,
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

/// Frames the identity chain and client token into a Login connection request.
///
/// Each half is a little-endian `u32` byte length followed by the raw bytes,
/// which is what gophertunnel's `encodeRequest` writes
/// (`minecraft/protocol/login/request.go`) and what the 1.26.30 schema modelled
/// as two `LittleString`s. The 1.26.40 schema types the whole thing as one
/// opaque blob, so the framing lives here now.
///
/// The blob is deliberately not a `String`: those length prefixes are arbitrary
/// bytes, and any length whose low byte is >= 0x80 is invalid UTF-8.
fn encode_connection_request(chain: &str, client_token: &str) -> Vec<u8> {
    let chain = chain.as_bytes();
    let client_token = client_token.as_bytes();

    let mut out = Vec::with_capacity(8 + chain.len() + client_token.len());
    out.extend_from_slice(&(chain.len() as u32).to_le_bytes());
    out.extend_from_slice(chain);
    out.extend_from_slice(&(client_token.len() as u32).to_le_bytes());
    out.extend_from_slice(client_token);
    out
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
            client_network_version: crate::valentine::PROTOCOL_VERSION,
        };
        self.transport.send_raw(McpePacket::from(req)).await?;

        let settings_raw = self.transport.recv_packet_raw().await?;
        if settings_raw.id != McpePacketName::NetworkSettingsPacket {
            return Err(ProtocolError::UnexpectedHandshake(format!(
                "Expected NetworkSettings, got {:?}",
                settings_raw.id
            ))
            .into());
        }
        let settings_pkt = settings_raw.decode_borrowed()?;

        match settings_pkt.data {
            BorrowedMcpePacketData::NetworkSettingsPacket(settings) => {
                match settings.compression_algorithm {
                    NetworkSettingsPacketCompressionAlgorithm::ZLib => {
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
                    // 1.26.40 names the no-compression algorithm explicitly; older
                    // generated code only surfaced it as the 0xFFFF unknown value.
                    NetworkSettingsPacketCompressionAlgorithm::None => {
                        self.transport.set_compression_algorithm(
                            true,
                            BatchCompression::None,
                            0,
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
            client_network_version: crate::valentine::PROTOCOL_VERSION,
            connection_request: encode_connection_request(&chain, &client_token),
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
    if let McpePacketData::PlayStatusPacket(status) = &packet.data {
        if status.status != PlayStatusPacketStatus::LoginSuccess {
            return Err(ProtocolError::UnexpectedHandshake(format!(
                "Login failed: {:?}",
                status.status
            ))
            .into());
        }
        return Ok(true);
    }
    if let McpePacketData::DisconnectPacket(disconnect) = &packet.data {
        return Err(ProtocolError::UnexpectedHandshake(format!(
            "Server disconnected during login: {:?}",
            disconnect.reason
        ))
        .into());
    }
    if matches!(&packet.data, McpePacketData::ResourcePacksInfoPacket(_)) {
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
            McpePacketName::ServerToClientHandshakePacket
                | McpePacketName::PlayStatusPacket
                | McpePacketName::DisconnectPacket
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
            McpePacketData::ServerToClientHandshakePacket(hs) => {
                tracing::debug!("Processing ServerToClientHandshake");
                // 1. Decode Header to find Server Public Key (x5u)
                let header = decode_header(&hs.handshake_web_token).map_err(|e| {
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
                let mut parts = hs.handshake_web_token.split('.');
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
                        McpePacketName::PlayStatusPacket
                            | McpePacketName::ResourcePacksInfoPacket
                            | McpePacketName::DisconnectPacket
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
                    iscachesupported: client_cache_enabled,
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
            McpePacketData::PlayStatusPacket(status) => {
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
                        iscachesupported: client_cache_enabled,
                    })])
                    .await?;
            }
            McpePacketData::DisconnectPacket(disconnect) => {
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
            state: StartGame::with_resource_pack_handoff(ResourcePackHandoff::default()),
            _role: PhantomData,
        }
    }

    fn start_game_packet() -> McpePacket {
        McpePacket::from(StartGamePacket {
            runtime_id: ActorRuntimeId {
                actor_runtime_id: 42,
            },
            ..Default::default()
        })
    }

    fn spawn_completion_packets() -> [McpePacket; 3] {
        [
            McpePacket::from(ItemRegistryPacket::default()),
            McpePacket::from(crate::valentine::ChunkRadiusUpdatedPacket { chunk_radius: 16 }),
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
                data: McpePacketData::ClientCacheStatusPacket(status),
                ..
            }] if !status.iscachesupported
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
                data: McpePacketData::ClientCacheStatusPacket(status),
                ..
            }] if status.iscachesupported
        ));
    }

    #[tokio::test]
    async fn request_settings_rejects_an_unexpected_raw_id_without_decoding_its_body() {
        let sent = Arc::new(Mutex::new(Vec::new()));
        let inbound = vec![malformed_uncompressed_frame(
            crate::valentine::McpePacketName::SetTitlePacket,
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
                if message.contains("SetTitlePacket")
        ));
    }

    #[tokio::test]
    async fn optional_start_game_packets_are_fifo_deferred_compact_frames() {
        let mut packets = vec![
            start_game_packet(),
            McpePacket::from(crate::valentine::SetTimePacket { time: 11 }),
            McpePacket::from(crate::valentine::BiomeDefinitionListPacket::default()),
            McpePacket::from(crate::valentine::AvailableActorIdentifiersPacket::default()),
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
            crate::valentine::McpePacketName::SetTimePacket,
            crate::valentine::McpePacketName::BiomeDefinitionListPacket,
            crate::valentine::McpePacketName::AvailableActorIdentifiersPacket,
            crate::valentine::McpePacketName::CreativeContentPacket,
            crate::valentine::McpePacketName::SetTimePacket,
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
                // The chunk blob is a length-prefixed byte buffer; HALF_LIMIT
                // zero bytes keep the encoded length at exactly HALF_LIMIT.
                serialized_chunk_data: vec![0u8; HALF_LIMIT],
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
    async fn unadvertised_resource_pack_stack_is_rejected() {
        let info = McpePacket::from(crate::valentine::ResourcePacksInfoPacket::default());
        let stack = McpePacket::from(crate::valentine::ResourcePackStackPacket {
            texture_pack_list: vec![crate::valentine::PackInstanceId {
                pack_id: "pack-id".into(),
                version: "1.0.0".into(),
                sub_pack_name: "test pack".into(),
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
            Ok(_) => panic!("an unadvertised server pack stack must not be accepted"),
            Err(error) => error,
        };
        assert!(error.to_string().contains("resource-pack handoff failed"));
    }

    #[tokio::test]
    async fn pinned_gophertunnel_exempt_pack_stack_is_accepted() {
        let info = McpePacket::from(crate::valentine::ResourcePacksInfoPacket::default());
        let (uuid, version) = EXEMPTED_RESOURCE_PACKS[0];
        let stack = McpePacket::from(crate::valentine::ResourcePackStackPacket {
            texture_pack_list: vec![crate::valentine::PackInstanceId {
                pack_id: uuid.into(),
                version: version.into(),
                sub_pack_name: "client built-in".into(),
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

    fn test_pack_info(
        id: Uuid,
        data: &[u8],
        subpack: &str,
        key: &str,
    ) -> crate::valentine::PackInfoData {
        crate::valentine::PackInfoData {
            pack_id_version: crate::valentine::PackIdVersion {
                pack_uuid: id,
                pack_version: crate::valentine::SemVersion {
                    version: "1.0.0".into(),
                },
            },
            pack_size: data.len() as u64,
            content_key: key.into(),
            subpack_name: subpack.into(),
            ..Default::default()
        }
    }

    fn test_pack_packets(id: Uuid, data: &[u8], chunk_size: u32) -> Vec<McpePacket> {
        let name = format!("{id}_1.0.0");
        let count = (data.len() as u64).div_ceil(u64::from(chunk_size)) as u32;
        let mut packets = vec![McpePacket::from(
            crate::valentine::ResourcePackDataInfoPacket {
                resource_name: name.clone(),
                chunk_size,
                numberof_chunks: count,
                file_size: data.len() as u64,
                file_hash: Sha256::digest(data).to_vec(),
                ..Default::default()
            },
        )];
        for chunk in 0..count {
            let start = chunk as usize * chunk_size as usize;
            let end = (start + chunk_size as usize).min(data.len());
            packets.push(McpePacket::from(
                crate::valentine::ResourcePackChunkDataPacket {
                    resource_name: name.clone(),
                    chunk_id: chunk,
                    byte_offset: start as u64,
                    chunk_data: data[start..end].to_vec(),
                },
            ));
        }
        packets
    }

    fn resource_pack_stream(
        inbound: Vec<Bytes>,
    ) -> BedrockStream<ResourcePacks, Client, ScriptedTransport> {
        BedrockStream {
            transport: BedrockTransport::new(ScriptedTransport::new(
                inbound,
                Arc::new(Mutex::new(Vec::new())),
            )),
            state: ResourcePacks { early_packet: None },
            _role: PhantomData,
        }
    }

    #[tokio::test]
    async fn optional_pack_handoff_preserves_selected_subpack_archive_and_key() {
        let first_id = Uuid::new_v4();
        let selected_id = Uuid::new_v4();
        let first = b"unselected";
        let selected = b"selected archive split into chunks";
        let info = McpePacket::from(crate::valentine::ResourcePacksInfoPacket {
            resource_packs: vec![
                test_pack_info(first_id, first, "unused", "unused-key"),
                test_pack_info(selected_id, selected, "high", "memory-key"),
            ],
            ..Default::default()
        });
        let mut inbound = vec![uncompressed_frame(&[info])];
        for packet in test_pack_packets(first_id, first, 1024)
            .into_iter()
            .chain(test_pack_packets(selected_id, selected, 7))
        {
            inbound.push(uncompressed_frame(&[packet]));
        }
        inbound.push(uncompressed_frame(&[McpePacket::from(
            crate::valentine::ResourcePackStackPacket {
                texture_pack_list: vec![crate::valentine::PackInstanceId {
                    pack_id: selected_id.to_string(),
                    version: "1.0.0".into(),
                    sub_pack_name: "high".into(),
                }],
                ..Default::default()
            },
        )]));
        let mut start = resource_pack_stream(inbound)
            .handle_packs()
            .await
            .expect("valid optional handoff");
        let archives = start
            .state
            .resource_pack_handoff
            .take()
            .unwrap()
            .into_archives();
        assert_eq!(archives.len(), 1);
        assert_eq!(archives[0].pack_id, selected_id);
        assert_eq!(archives[0].sub_pack_name, "high");
        assert_eq!(archives[0].archive, selected);
        assert_eq!(archives[0].content_key.expose(), b"memory-key");
    }

    #[tokio::test]
    async fn pack_handoff_rejects_required_duplicate_and_malformed_inputs_secret_safely() {
        let id = Uuid::new_v4();
        let required = McpePacket::from(crate::valentine::ResourcePacksInfoPacket {
            resource_pack_required: true,
            resource_packs: vec![test_pack_info(id, b"archive", "", "hidden-key")],
            ..Default::default()
        });
        let mut required_inbound = vec![uncompressed_frame(&[required])];
        required_inbound.extend(
            test_pack_packets(id, b"archive", 1024)
                .into_iter()
                .map(|packet| uncompressed_frame(&[packet])),
        );
        required_inbound.push(uncompressed_frame(&[McpePacket::from(
            crate::valentine::ResourcePackStackPacket {
                texture_pack_list: vec![crate::valentine::PackInstanceId {
                    pack_id: id.to_string(),
                    version: "1.0.0".into(),
                    sub_pack_name: String::new(),
                }],
                ..Default::default()
            },
        )]));
        let error = resource_pack_stream(required_inbound)
            .handle_packs()
            .await
            .err()
            .expect("required offer must fail");
        assert!(error.to_string().contains("required resource-pack handoff"));

        let duplicate = McpePacket::from(crate::valentine::ResourcePacksInfoPacket {
            resource_packs: vec![
                test_pack_info(id, b"one", "", "first-key"),
                test_pack_info(id, b"two", "", "second-key"),
            ],
            ..Default::default()
        });
        let error = resource_pack_stream(vec![uncompressed_frame(&[duplicate])])
            .handle_packs()
            .await
            .err()
            .expect("duplicate offer must fail");
        assert!(error.to_string().contains("duplicate or ambiguous"));

        for fault in ["offset", "length", "digest", "identity"] {
            let secret = "never-in-errors";
            let data = b"bounded archive";
            let info = McpePacket::from(crate::valentine::ResourcePacksInfoPacket {
                resource_packs: vec![test_pack_info(id, data, "selected", secret)],
                ..Default::default()
            });
            let mut packets = test_pack_packets(id, data, 1024);
            match fault {
                "offset" => {
                    if let McpePacketData::ResourcePackChunkDataPacket(v) = &mut packets[1].data {
                        v.byte_offset = 1;
                    }
                }
                "length" => {
                    if let McpePacketData::ResourcePackChunkDataPacket(v) = &mut packets[1].data {
                        v.chunk_data.pop();
                    }
                }
                "digest" => {
                    if let McpePacketData::ResourcePackDataInfoPacket(v) = &mut packets[0].data {
                        v.file_hash.fill(0);
                    }
                }
                "identity" => {
                    if let McpePacketData::ResourcePackDataInfoPacket(v) = &mut packets[0].data {
                        v.resource_name = "unadvertised".into();
                    }
                }
                _ => unreachable!(),
            }
            let mut inbound = vec![uncompressed_frame(&[info])];
            inbound.extend(
                packets
                    .into_iter()
                    .map(|packet| uncompressed_frame(&[packet])),
            );
            let message = resource_pack_stream(inbound)
                .handle_packs()
                .await
                .err()
                .expect("malformed download must fail")
                .to_string();
            assert!(!message.contains(secret));
            assert!(!message.contains(&id.to_string()));
        }
    }

    #[tokio::test]
    async fn pack_handoff_enforces_archive_and_chunk_limits() {
        let id = Uuid::new_v4();
        let mut oversized = test_pack_info(id, b"", "", "bounded-key");
        oversized.pack_size = MAX_RESOURCE_PACK_BYTES + 1;
        let info = McpePacket::from(crate::valentine::ResourcePacksInfoPacket {
            resource_packs: vec![oversized],
            ..Default::default()
        });
        assert!(
            resource_pack_stream(vec![uncompressed_frame(&[info])])
                .handle_packs()
                .await
                .err()
                .expect("oversized offer must fail")
                .to_string()
                .contains("pack size exceeds limit")
        );

        for fault in ["chunk size", "chunk count"] {
            let data = b"archive";
            let info = McpePacket::from(crate::valentine::ResourcePacksInfoPacket {
                resource_packs: vec![test_pack_info(id, data, "", "bounded-key")],
                ..Default::default()
            });
            let mut packets = test_pack_packets(id, data, data.len() as u32);
            let McpePacketData::ResourcePackDataInfoPacket(metadata) = &mut packets[0].data else {
                unreachable!()
            };
            if fault == "chunk size" {
                metadata.chunk_size = MAX_RESOURCE_PACK_CHUNK_BYTES + 1;
            } else {
                metadata.numberof_chunks = MAX_RESOURCE_PACK_CHUNKS + 1;
            }
            let mut inbound = vec![uncompressed_frame(&[info])];
            inbound.extend(
                packets
                    .into_iter()
                    .map(|packet| uncompressed_frame(&[packet])),
            );
            assert!(
                resource_pack_stream(inbound)
                    .handle_packs()
                    .await
                    .err()
                    .expect("out-of-bounds metadata must fail")
                    .to_string()
                    .contains("invalid pack metadata")
            );
        }
    }

    #[tokio::test]
    async fn optional_offer_with_required_stack_is_rejected() {
        let id = Uuid::new_v4();
        let data = b"archive";
        let info = McpePacket::from(crate::valentine::ResourcePacksInfoPacket {
            resource_packs: vec![test_pack_info(id, data, "", "bounded-key")],
            ..Default::default()
        });
        let mut inbound = vec![uncompressed_frame(&[info])];
        inbound.extend(
            test_pack_packets(id, data, data.len() as u32)
                .into_iter()
                .map(|packet| uncompressed_frame(&[packet])),
        );
        inbound.push(uncompressed_frame(&[McpePacket::from(
            crate::valentine::ResourcePackStackPacket {
                texture_pack_required: true,
                texture_pack_list: vec![crate::valentine::PackInstanceId {
                    pack_id: id.to_string(),
                    version: "1.0.0".into(),
                    sub_pack_name: String::new(),
                }],
                ..Default::default()
            },
        )]));
        let error = resource_pack_stream(inbound)
            .handle_packs()
            .await
            .err()
            .expect("required stack must fail");
        assert!(error.to_string().contains("required resource-pack handoff"));
    }

    #[tokio::test]
    async fn required_offer_with_empty_selection_is_a_well_formed_noop() {
        let id = Uuid::new_v4();
        let data = b"unselected archive";
        let info = McpePacket::from(crate::valentine::ResourcePacksInfoPacket {
            resource_pack_required: true,
            resource_packs: vec![test_pack_info(id, data, "", "bounded-key")],
            ..Default::default()
        });
        let stack = McpePacket::from(crate::valentine::ResourcePackStackPacket {
            texture_pack_required: true,
            ..Default::default()
        });
        let mut inbound = vec![uncompressed_frame(&[info])];
        inbound.extend(
            test_pack_packets(id, data, 1024)
                .into_iter()
                .map(|packet| uncompressed_frame(&[packet])),
        );
        inbound.push(uncompressed_frame(&[stack]));
        let start = resource_pack_stream(inbound)
            .handle_packs()
            .await
            .expect("required flags without selected content are a no-op");
        assert!(start.state.resource_pack_handoff.unwrap().is_empty());
    }

    #[test]
    fn play_resource_pack_handoff_is_one_shot() {
        let archive = ResourcePackArchive {
            pack_id: Uuid::new_v4(),
            version: "1.0.0".into(),
            sub_pack_name: String::new(),
            archive: vec![1],
            content_key: ResourcePackContentKey::from_string("secret".into()),
        };
        let mut stream = BedrockStream {
            transport: BedrockTransport::new(ScriptedTransport::new(
                Vec::new(),
                Arc::new(Mutex::new(Vec::new())),
            )),
            state: Play {
                resource_pack_handoff: Some(ResourcePackHandoff::new(vec![archive])),
            },
            _role: PhantomData::<Client>,
        };
        assert_eq!(stream.take_resource_pack_handoff().len(), 1);
        assert!(stream.take_resource_pack_handoff().is_empty());
    }

    #[tokio::test]
    async fn cancelled_pack_download_releases_the_in_memory_handoff() {
        let info = McpePacket::from(crate::valentine::ResourcePacksInfoPacket {
            resource_packs: vec![test_pack_info(
                Uuid::new_v4(),
                b"archive",
                "",
                "cancelled-key",
            )],
            ..Default::default()
        });
        let stream = BedrockStream {
            transport: BedrockTransport::new(PendingTransport),
            state: ResourcePacks {
                early_packet: Some(info),
            },
            _role: PhantomData,
        };
        assert!(
            tokio::time::timeout(std::time::Duration::from_millis(10), stream.handle_packs())
                .await
                .is_err(),
            "pending download must be cancelled by its owner"
        );
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
                McpePacketName::ResourcePacksInfoPacket => raw.decode(&self.transport.session)?,
                McpePacketName::DisconnectPacket => {
                    let packet = raw.decode(&self.transport.session)?;
                    let McpePacketData::DisconnectPacket(disconnect) = packet.data else {
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

        let McpePacketData::ResourcePacksInfoPacket(mut info) = info_pkt.data else {
            return Err(
                ProtocolError::UnexpectedHandshake("Expected ResourcePacksInfo".into()).into(),
            );
        };
        let mut content_keys = info
            .resource_packs
            .iter_mut()
            .map(|pack| {
                Some(ResourcePackContentKey::from_string(std::mem::take(
                    &mut pack.content_key,
                )))
            })
            .collect::<Vec<_>>();
        let mut handoff = ResourcePackHandoff::default();
        let offer_required = info.resource_pack_required;
        if !info.resource_packs.is_empty() {
            tracing::debug!(
                "ResourcePacksInfo: must_accept={}, texture_packs={}",
                info.resource_pack_required,
                info.resource_packs.len()
            );
            handoff = self
                .download_optional_resource_packs(&mut info.resource_packs, &mut content_keys)
                .await?;
        }

        // 1.26.40 turns the response into a payload-carrying union whose body is
        // the lowercase enum name; gophertunnel writes the same string via
        // resourcePackResponseToString (packet/resource_pack_client_response.go),
        // where this response is PackResponseAllPacksDownloaded.
        tracing::debug!("Sending DownloadingFinished response...");
        let resp = ResourcePackClientResponsePacket {
            response: ResourcePackClientResponsePacketResponse::DownloadingFinished(
                ResourcePackClientResponsePacketPayloadDownloadingFinished {
                    response_type: "downloadingfinished".to_string(),
                },
            ),
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
            McpePacketName::ResourcePackStackPacket => stack_raw.decode(&self.transport.session)?,
            McpePacketName::DisconnectPacket => {
                let packet = stack_raw.decode(&self.transport.session)?;
                let McpePacketData::DisconnectPacket(disconnect) = packet.data else {
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

        if let McpePacketData::ResourcePackStackPacket(ref stack) = stack_pkt.data {
            tracing::debug!(
                "ResourcePackStack: must_accept={}, game_version={}, texture_pack_list={}",
                stack.texture_pack_required,
                stack.base_game_version,
                stack.texture_pack_list.len()
            );
            handoff = select_resource_pack_stack(handoff, &stack.texture_pack_list)?;
            if (offer_required || stack.texture_pack_required) && !handoff.is_empty() {
                return Err(pack_handoff_error(
                    "required resource-pack handoff is unavailable",
                ));
            }
        } else {
            return Err(ProtocolError::UnexpectedHandshake(format!(
                "Expected ResourcePackStack, got {:?}",
                stack_pkt.data.packet_id()
            ))
            .into());
        }

        // Send the stack-finished response to complete resource pack negotiation.
        // This is gophertunnel's PackResponseCompleted.
        tracing::debug!("Sending ResourcePackStackFinished (completed) response...");
        let complete = ResourcePackClientResponsePacket {
            response: ResourcePackClientResponsePacketResponse::ResourcePackStackFinished(
                ResourcePackClientResponsePacketPayloadResourcePackStackFinished {
                    response_type: "resourcepackstackfinished".to_string(),
                },
            ),
        };
        self.transport
            .send_batch(&[McpePacket::from(complete)])
            .await?;

        tracing::debug!("Resource packs negotiated successfully");

        Ok(BedrockStream {
            transport: self.transport,
            state: StartGame::with_resource_pack_handoff(handoff),
            _role: PhantomData,
        })
    }

    async fn download_optional_resource_packs(
        &mut self,
        offered: &mut [crate::valentine::PackInfoData],
        content_keys: &mut [Option<ResourcePackContentKey>],
    ) -> Result<ResourcePackHandoff, JolyneError> {
        if offered.len() > MAX_RESOURCE_PACKS {
            return Err(pack_handoff_error("pack count exceeds limit"));
        }
        let mut requested = Vec::with_capacity(offered.len());
        let mut seen = HashSet::with_capacity(offered.len());
        let mut total = 0u64;
        for pack in offered.iter() {
            if pack.pack_size > MAX_RESOURCE_PACK_BYTES {
                return Err(pack_handoff_error("pack size exceeds limit"));
            }
            total = total
                .checked_add(pack.pack_size)
                .filter(|&value| value <= MAX_RESOURCE_PACK_TOTAL_BYTES)
                .ok_or_else(|| pack_handoff_error("total pack size exceeds limit"))?;
            let name = format!(
                "{}_{}",
                pack.pack_id_version.pack_uuid, pack.pack_id_version.pack_version.version
            );
            if !seen.insert(name.clone()) {
                return Err(pack_handoff_error("duplicate or ambiguous pack identity"));
            }
            requested.push(name);
        }

        self.transport
            .send_batch(&[McpePacket::from(ResourcePackClientResponsePacket {
                response: ResourcePackClientResponsePacketResponse::Downloading(
                    ResourcePackClientResponsePacketPayloadDownloading {
                        response_type: "downloading".to_string(),
                        downloading_packs: requested.clone(),
                    },
                ),
            })])
            .await?;

        let mut archives = Vec::with_capacity(offered.len());
        for (index, pack) in offered.iter_mut().enumerate() {
            let raw = tokio::time::timeout(
                std::time::Duration::from_secs(30),
                self.transport.recv_packet_raw(),
            )
            .await
            .map_err(|_| pack_handoff_error("timed out waiting for pack metadata"))??;
            if raw.id != McpePacketName::ResourcePackDataInfoPacket {
                return Err(pack_handoff_error("unexpected packet during pack metadata"));
            }
            let packet = raw.decode(&self.transport.session)?;
            let McpePacketData::ResourcePackDataInfoPacket(data) = packet.data else {
                return Err(pack_handoff_error("invalid pack metadata packet"));
            };
            if data.resource_name != requested[index]
                || data.file_size != pack.pack_size
                || data.file_size > MAX_RESOURCE_PACK_BYTES
                || data.chunk_size == 0
                || data.chunk_size > MAX_RESOURCE_PACK_CHUNK_BYTES
                || data.numberof_chunks == 0
                || data.numberof_chunks > MAX_RESOURCE_PACK_CHUNKS
                || data.file_hash.len() != 32
            {
                return Err(pack_handoff_error("invalid pack metadata"));
            }
            let expected_chunks = data.file_size.div_ceil(u64::from(data.chunk_size));
            if expected_chunks != u64::from(data.numberof_chunks) {
                return Err(pack_handoff_error("invalid pack chunk count"));
            }
            let capacity = usize::try_from(data.file_size)
                .map_err(|_| pack_handoff_error("pack size is not representable"))?;
            let mut archive = Vec::with_capacity(capacity);
            for chunk in 0..data.numberof_chunks {
                self.transport
                    .send_batch(&[McpePacket::from(ResourcePackChunkRequestPacket {
                        resource_name: requested[index].clone(),
                        chunk: i32::try_from(chunk)
                            .map_err(|_| pack_handoff_error("invalid pack chunk index"))?,
                    })])
                    .await?;
                let raw = tokio::time::timeout(
                    std::time::Duration::from_secs(30),
                    self.transport.recv_packet_raw(),
                )
                .await
                .map_err(|_| pack_handoff_error("timed out waiting for pack chunk"))??;
                if raw.id != McpePacketName::ResourcePackChunkDataPacket {
                    return Err(pack_handoff_error("unexpected packet during pack download"));
                }
                let packet = raw.decode(&self.transport.session)?;
                let McpePacketData::ResourcePackChunkDataPacket(chunk_data) = packet.data else {
                    return Err(pack_handoff_error("invalid pack chunk packet"));
                };
                let offset = u64::from(chunk) * u64::from(data.chunk_size);
                let remaining = data.file_size - offset;
                let expected_len = remaining.min(u64::from(data.chunk_size));
                if chunk_data.resource_name != requested[index]
                    || chunk_data.chunk_id != chunk
                    || chunk_data.byte_offset != offset
                    || u64::try_from(chunk_data.chunk_data.len()).ok() != Some(expected_len)
                {
                    return Err(pack_handoff_error("invalid or reordered pack chunk"));
                }
                archive.extend_from_slice(&chunk_data.chunk_data);
            }
            if archive.len() != capacity || Sha256::digest(&archive).as_slice() != data.file_hash {
                return Err(pack_handoff_error("pack length or digest mismatch"));
            }
            archives.push(ResourcePackArchive {
                pack_id: pack.pack_id_version.pack_uuid,
                version: std::mem::take(&mut pack.pack_id_version.pack_version.version),
                sub_pack_name: std::mem::take(&mut pack.subpack_name),
                archive,
                content_key: content_keys[index]
                    .take()
                    .expect("content key retained for each bounded offer"),
            });
        }
        Ok(ResourcePackHandoff::new(archives))
    }
}

fn pack_handoff_error(reason: &'static str) -> JolyneError {
    ProtocolError::UnexpectedHandshake(format!("resource-pack handoff failed: {reason}")).into()
}

fn select_resource_pack_stack(
    handoff: ResourcePackHandoff,
    stack: &[crate::valentine::PackInstanceId],
) -> Result<ResourcePackHandoff, JolyneError> {
    let mut available = HashMap::with_capacity(handoff.len());
    for archive in handoff.into_archives() {
        if available
            .insert((archive.pack_id, archive.version.clone()), archive)
            .is_some()
        {
            return Err(pack_handoff_error("duplicate captured pack identity"));
        }
    }
    let mut selected = Vec::new();
    let mut seen = HashSet::new();
    for entry in stack {
        if is_exempted_resource_pack(&entry.pack_id, &entry.version) {
            continue;
        }
        let id = Uuid::parse_str(&entry.pack_id)
            .map_err(|_| pack_handoff_error("invalid selected pack identity"))?;
        let key = (id, entry.version.clone());
        if !seen.insert(key.clone()) {
            return Err(pack_handoff_error("duplicate selected pack identity"));
        }
        let mut archive = available
            .remove(&key)
            .ok_or_else(|| pack_handoff_error("unadvertised or missing selected pack"))?;
        if archive.sub_pack_name != entry.sub_pack_name {
            return Err(pack_handoff_error("selected sub-pack does not match offer"));
        }
        archive.sub_pack_name = entry.sub_pack_name.clone();
        selected.push(archive);
    }
    Ok(ResourcePackHandoff::new(selected))
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
        let mut runtime_entity_id: Option<u64> = None;
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
                McpePacketName::StartGamePacket => {
                    let packet = raw.decode(&self.transport.session)?;
                    let McpePacketData::StartGamePacket(start) = packet.data else {
                        unreachable!("packet ID and decoded variant must agree")
                    };
                    tracing::debug!(runtime_id = %start.runtime_id.actor_runtime_id, "StartGame received");
                    if let Some(existing) = runtime_entity_id {
                        if existing != start.runtime_id.actor_runtime_id {
                            return Err(ProtocolError::UnexpectedHandshake(format!(
                                "conflicting StartGame runtime entity ID: first {existing}, then {}",
                                start.runtime_id.actor_runtime_id
                            ))
                            .into());
                        }
                    } else {
                        runtime_entity_id = Some(start.runtime_id.actor_runtime_id);
                        start_game = Some(*start);
                    }
                }
                McpePacketName::ItemRegistryPacket => {
                    let packet = raw.decode(&self.transport.session)?;
                    let McpePacketData::ItemRegistryPacket(registry) = packet.data else {
                        unreachable!("packet ID and decoded variant must agree")
                    };
                    tracing::debug!(items = %registry.item_data.len(), "ItemRegistry received");
                    if let Some(shield) = registry
                        .item_data
                        .iter()
                        .find(|item| item.item_name == "minecraft:shield")
                    {
                        // `item_id` is the network runtime ID (gophertunnel's
                        // protocol.ItemEntry.RuntimeID). The 1.26.40 decoder no
                        // longer consumes this, but the session still tracks it.
                        self.transport.session.shield_item_id = i32::from(shield.item_id);
                    }
                    item_registry = Some(registry);
                }
                McpePacketName::PlayStatusPacket => {
                    let packet = raw.decode(&self.transport.session)?;
                    let McpePacketData::PlayStatusPacket(status) = packet.data else {
                        unreachable!("packet ID and decoded variant must agree")
                    };
                    tracing::debug!("PlayStatus received: {:?}", status.status);
                    if status.status == PlayStatusPacketStatus::PlayerSpawn {
                        received_player_spawn = true;
                    }
                }
                McpePacketName::ChunkRadiusUpdatedPacket => {
                    let packet = raw.clone().decode(&self.transport.session)?;
                    let McpePacketData::ChunkRadiusUpdatedPacket(update) = packet.data else {
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
                McpePacketName::DisconnectPacket => {
                    let packet = raw.decode(&self.transport.session)?;
                    let McpePacketData::DisconnectPacket(dc) = packet.data else {
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
                            loading_screen_packet_type:
                                ServerboundLoadingScreenPacketLoadingScreenPacketType::StartLoadingScreen,
                            loading_screen_id: None,
                        }),
                        McpePacket::from(RequestChunkRadiusPacket {
                            chunk_radius: 16,
                            max_chunk_radius: 16,
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
                    loading_screen_packet_type:
                        ServerboundLoadingScreenPacketLoadingScreenPacketType::EndLoadingScreen,
                    loading_screen_id: None,
                }),
                McpePacket::from(SetLocalPlayerAsInitializedPacket {
                    player_id: ActorRuntimeId {
                        actor_runtime_id: runtime_entity_id,
                    },
                }),
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
                state: Play {
                    resource_pack_handoff: self.state.resource_pack_handoff.take(),
                },
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
