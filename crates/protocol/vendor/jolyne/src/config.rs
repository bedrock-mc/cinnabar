#[derive(Debug, Clone, Copy)]
pub struct BedrockListenerConfig {
    /// Whether to enforce Xbox Live authentication (online mode).
    ///
    /// If `true`, the server will verify the JWT chain in the LoginPacket against the Mojang public key.
    /// If `false`, the server will accept any validly structured self-signed JWT chain (offline mode).
    ///
    /// Default: `true`
    pub online_mode: bool,

    /// Minimum uncompressed payload size (in bytes) before compression is applied to batches.
    /// Matches Bedrock behavior where very small packets stay uncompressed.
    ///
    /// Default: `512`
    pub compression_threshold: u16,

    /// The compression level to use for batch packets.
    /// Default: 7
    pub compression_level: u32,

    /// Whether to enable the Bedrock encryption handshake (ServerToClientHandshake / ClientToServerHandshake).
    /// When enabled, clients are expected to complete the handshake;
    ///
    /// Default: `true`
    pub encryption_enabled: bool,

    /// Whether to allow legacy/self-signed authentication chains (guest/self-signed).
    ///
    /// Default: `true`
    pub allow_legacy_auth: bool,

    /// Whether clients must accept resource packs. We currently send an empty pack list; if this is true and
    /// the client refuses, the connection is terminated.
    ///
    /// Default: `false`
    pub require_resource_packs: bool,

    /// Whether to handle (accept/ignore) ClientCacheStatus packets. Default: true (accept and ignore payload).
    pub handle_client_cache_status: bool,

    /// Reserved flag for future explicit block-palette/update packets.
    ///
    /// Current generated StartGame data already carries block properties; the server start
    /// sequence does not send additional palette/update packets until Jolyne has a data source
    /// for non-empty runtime block data.
    ///
    /// Default: `false`
    pub send_block_palette: bool,

    /// Number of bytes to skip at the beginning of the packet during encryption/decryption.
    ///
    /// Bedrock RakNet packets usually start with a Packet ID (0xFE for GamePackets) which is
    /// kept in cleartext, while the rest of the payload is encrypted.
    ///
    /// Default: `1` (Preserves 0xFE)
    pub encryption_header_len: usize,

    /// Optional guard to cap decompressed batch payloads.
    ///
    /// If set, any batch whose decompressed size exceeds this limit will be rejected.
    /// This helps avoid zip bombs or malformed packets that expand excessively.
    ///
    /// Default: `Some(4 MiB)`
    pub max_decompressed_batch_size: Option<usize>,
}

impl Default for BedrockListenerConfig {
    fn default() -> Self {
        Self {
            online_mode: true,
            compression_threshold: 512,
            compression_level: 7,
            encryption_enabled: true,
            allow_legacy_auth: true,
            require_resource_packs: false,
            handle_client_cache_status: true,
            send_block_palette: false,
            encryption_header_len: 1,
            max_decompressed_batch_size: Some(1024 * 1024 * 4),
        }
    }
}
