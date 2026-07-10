use thiserror::Error;
#[cfg(feature = "nethernet")]
use tokio_nethernet::error::NetherNetError;
#[cfg(feature = "raknet")]
use tokio_raknet::RaknetError;

#[derive(Debug, Error, Clone, PartialEq, Eq)]
pub enum AuthError {
    #[error("Empty login chain")]
    EmptyChain,
    #[error("Missing login chain")]
    MissingChain,
    #[error("Legacy/self-signed authentication is disabled on this server")]
    LegacyAuthDisabled,
    #[error("Missing authentication type")]
    MissingAuthType,
    #[error("Missing authentication token")]
    MissingToken,
    #[error("Missing authentication certificate")]
    MissingCertificate,
    #[error("Unsupported authentication type: {0}")]
    UnsupportedAuthType(u32),
    #[error("Login identity payload too large (max {0} bytes)")]
    PayloadTooLarge(usize),
    #[error("Invalid JWT header: {0}")]
    InvalidHeader(String),
    #[error("Unsupported JWT algorithm: {0}")]
    UnsupportedAlg(String),
    #[error("JWT validation failed: {0}")]
    BadSignature(String),
    #[error("JWT issuer not accepted: {0}")]
    InvalidIssuer(String),
    #[error("JWT audience not accepted: {0}")]
    InvalidAudience(String),
    #[error("Login chain has too many entries (max {0})")]
    ChainTooLong(usize),
    #[error("JWT token too large (max {0} bytes)")]
    TokenTooLarge(usize),
    #[error("Missing identityPublicKey")]
    MissingIdentityKey,
    #[error("Missing extraData")]
    MissingExtraData,
    #[error("Invalid UTF-8 in identity payload")]
    InvalidUtf8,
    #[error("Invalid JSON in identity payload")]
    InvalidJson,
    #[error("Token expired or not yet valid")]
    TemporalValidation,
}

#[derive(Debug, Error, Clone, PartialEq, Eq)]
pub enum ProtocolError {
    #[error("Invalid batch packet id: {0}")]
    InvalidBatchId(String),
    #[error("Decompression failed: {0}")]
    DecompressionFailed(String),
    #[error("Incompatible protocol version: client {client_protocol}, server {server_protocol}")]
    IncompatibleProtocol {
        client_protocol: i32,
        server_protocol: i32,
    },
    #[error("Unexpected packet during handshake: {0}")]
    UnexpectedHandshake(String),
    #[error("Missing expected Login packet after NetworkSettings")]
    MissingLoginPacket,
    #[error("Empty packet during initial handshake")]
    EmptyHandshakePacket,
}

#[derive(Debug, Error)]
pub enum JolyneError {
    #[cfg(feature = "raknet")]
    #[error("RakNet error: {0}")]
    Raknet(#[from] RaknetError),

    #[cfg(feature = "nethernet")]
    #[error("NetherNet error: {0}")]
    NetherNet(#[from] NetherNetError),

    #[error("Decode error: {0}")]
    Decode(#[from] valentine::bedrock::error::DecodeError),

    #[error("IO error: {0}")]
    Io(#[from] std::io::Error),

    #[error("Transport error: {0}")]
    Transport(String),

    #[error("Connection closed")]
    ConnectionClosed,

    #[error("Listener closed")]
    ListenerClosed,

    #[error("Authentication error: {0}")]
    Auth(#[from] AuthError),

    #[error("Protocol error: {0}")]
    Protocol(#[from] ProtocolError),
}
