use std::sync::LazyLock;

use bedrock_protocol_1_26_51::packets::{self, PacketId};
use bedrock_protocol_1_26_51::wire;
use bytes::Bytes;
use thiserror::Error;

use crate::packet::GameHeader;
use crate::ui::UiPacketError;
use crate::world::WorldPacketError;
use crate::{Packet, PacketKind};

const MAX_BATCH_BYTES: usize = 16 * 1024 * 1024;
const MAX_BATCH_PACKETS: usize = 1_600;

/// Errors produced by raw Bedrock batch encoding and decoding.
#[derive(Debug, Error)]
pub enum ProtocolError {
    #[error("bridge connection failed: {0}")]
    Bridge(#[source] anyhow::Error),

    #[error("client blob cache failed: {0}")]
    BlobCache(#[from] crate::BlobCacheError),

    #[error("Bedrock session failed: {0}")]
    Session(#[source] SessionError),

    #[error("the server ended the current play session")]
    SessionBoundary,

    #[error("invalid raw batch header: expected 0xfe, got {actual:?}")]
    InvalidBatchHeader { actual: Option<u8> },

    #[error("raw batch is {actual} bytes, exceeding the {max}-byte limit")]
    BatchTooLarge { actual: usize, max: usize },

    #[error("raw batch contains more than {max} packets")]
    TooManyPackets { max: usize },

    #[error("declared packet length {declared} exceeds {available} available bytes")]
    TruncatedPacket { declared: usize, available: usize },

    #[error("decoded packet left {remaining} trailing bytes in its declared entry")]
    TrailingPacketBytes { remaining: usize },

    #[error("packet header ID {header:?} does not match payload ID {payload:?}")]
    HeaderIdMismatch {
        header: PacketKind,
        payload: PacketKind,
    },

    #[error("subclient IDs must be in 0..=3, got sender {sender} and target {target}")]
    InvalidSubclient { sender: u32, target: u32 },

    #[error("packet decode failed: {0}")]
    Decode(#[source] PacketDecodeError),

    #[error("UI packet validation failed: {0}")]
    Ui(#[from] UiPacketError),

    #[error("packet encode failed: {0}")]
    Encode(#[source] PacketEncodeError),

    #[error("world packet normalization failed: {0}")]
    World(#[from] WorldPacketError),
}

/// A failure in the play session or its startup; `Display` carries its detail.
#[derive(Debug, Error)]
#[error(transparent)]
pub struct SessionError(pub(crate) SessionFailure);

/// Why a session failed; packet failures name the packet and keep a bounded body preview.
#[derive(Debug, Error)]
pub(crate) enum SessionFailure {
    #[error(
        "decode packet {packet_id:?} (body_len={body_len}, body_preview={body_preview:02x?}): {source}"
    )]
    PacketDecode {
        packet_id: PacketId,
        body_len: usize,
        body_preview: Vec<u8>,
        #[source]
        source: wire::DecodeError,
    },
    #[error(
        "decode packet {packet_id:?} (body_len={body_len}) left {remaining} trailing body bytes"
    )]
    PacketTrailingBytes {
        packet_id: PacketId,
        body_len: usize,
        remaining: usize,
    },
    #[error("malformed packet entry: {0}")]
    MalformedEntry(#[source] wire::DecodeError),
    #[error("declared packet length {declared} exceeds available {available}")]
    TruncatedEntry { declared: usize, available: usize },
    #[error("unknown packet ID {0}")]
    UnknownPacketId(u32),
    #[error("batch payload exceeds max size ({actual} > {max})")]
    BatchTooLarge { actual: usize, max: usize },
    #[error("batch contains more than {max} packets")]
    TooManyPackets { max: usize },
    #[error("unexpected startup packet: {0}")]
    UnexpectedStartup(String),
    /// A Disconnect ended the join; the texts are the server's own, unbounded.
    #[error("server disconnected during {stage}: {reason}")]
    ServerDisconnect {
        stage: &'static str,
        reason: String,
        message: String,
        filtered_message: String,
    },
    /// A usable Transfer hands the join back to the reconnect owner.
    #[error("server transferred to {}:{}", .0.host, .0.port)]
    ServerTransfer(crate::ServerTransferEvent),
    #[error("session transport failed: {0}")]
    Transport(#[source] bridge::BridgeError),
    #[error("connection closed")]
    ConnectionClosed,
}

impl SessionFailure {
    /// Whether this is a malformed-input failure, which play counts, rather than a closed link.
    pub(crate) const fn is_decode(&self) -> bool {
        !matches!(self, Self::Transport(_) | Self::ConnectionClosed)
    }
}

impl From<SessionFailure> for SessionError {
    fn from(failure: SessionFailure) -> Self {
        Self(failure)
    }
}

/// A packet body the codec could not decode; `Display` carries its detail.
#[derive(Debug, Error)]
#[error(transparent)]
pub struct PacketDecodeError(pub(crate) wire::DecodeError);

/// A packet the codec could not encode; `Display` carries its detail.
#[derive(Debug, Error)]
#[error(transparent)]
pub struct PacketEncodeError(pub(crate) wire::EncodeError);

impl From<SessionError> for ProtocolError {
    fn from(error: SessionError) -> Self {
        Self::Session(error)
    }
}

impl From<SessionFailure> for ProtocolError {
    fn from(failure: SessionFailure) -> Self {
        Self::Session(SessionError(failure))
    }
}

impl From<wire::DecodeError> for ProtocolError {
    fn from(error: wire::DecodeError) -> Self {
        Self::Decode(PacketDecodeError(error))
    }
}

impl From<wire::EncodeError> for ProtocolError {
    fn from(error: wire::EncodeError) -> Self {
        Self::Encode(PacketEncodeError(error))
    }
}

/// Bounds for server packet bodies: no ceiling beyond the input except the UI budgets.
pub(crate) fn server_limits() -> &'static wire::Limits {
    static LIMITS: LazyLock<wire::Limits> = LazyLock::new(|| {
        crate::ui::validation::with_field_limits(wire::Limits::receiving_from(wire::Peer::Server))
    });
    &LIMITS
}

/// Decodes one entry body as `id` under [`server_limits`], sharing `body`'s allocation, and
/// rejects bytes the packet leaves unread.
pub(crate) fn decode_body(
    id: PacketId,
    body: &Bytes,
) -> Result<packets::Packet, wire::DecodeError> {
    let mut reader = wire::Reader::from_shared(body).with_limits(server_limits());
    let packet = packets::Packet::decode_body(id, &mut reader)?;
    reader.expect_consumed()?;
    Ok(packet)
}

/// Rejects a decoded packet whose UI or audio content the client must not admit.
pub(crate) fn validate_decoded(packet: &packets::Packet) -> Result<(), ProtocolError> {
    crate::ui::validation::validate_ui_packet(packet)?;
    crate::audio::validate_audio_packet(packet)?;
    Ok(())
}

/// Decodes an uncompressed gophertunnel raw batch into owned protocol packets.
pub fn decode_batch(bytes: Bytes) -> Result<Vec<Packet>, ProtocolError> {
    if bytes.len() > MAX_BATCH_BYTES {
        return Err(ProtocolError::BatchTooLarge {
            actual: bytes.len(),
            max: MAX_BATCH_BYTES,
        });
    }
    let frames =
        wire::Frames::from_batch(bytes, MAX_BATCH_PACKETS).map_err(|error| match error {
            wire::DecodeError::InvalidBatchHeader(actual) => {
                ProtocolError::InvalidBatchHeader { actual }
            }
            error => error.into(),
        })?;
    let mut packets = Vec::new();
    for frame in frames {
        let frame = frame.map_err(framing_error)?;
        let header = frame.header();
        let id =
            PacketId::from_raw(header.id).ok_or(wire::DecodeError::UnknownPacketId(header.id))?;
        let data = decode_body(id, &frame.body()).map_err(body_error)?;
        validate_decoded(&data)?;
        packets.push(Packet::from_parts(
            GameHeader {
                id,
                from_subclient: u32::from(header.sender_subclient),
                to_subclient: u32::from(header.target_subclient),
            },
            data,
        ));
    }
    Ok(packets)
}

/// Maps a batch framing failure onto the protocol's framing errors.
fn framing_error(error: wire::DecodeError) -> ProtocolError {
    match error {
        wire::DecodeError::TooManyFrames { limit } => ProtocolError::TooManyPackets { max: limit },
        wire::DecodeError::UnexpectedEof { needed, remaining } => ProtocolError::TruncatedPacket {
            declared: needed,
            available: remaining,
        },
        error => error.into(),
    }
}

/// Maps a body failure: unread bytes and UI budget rejections keep their own errors.
fn body_error(error: wire::DecodeError) -> ProtocolError {
    if let wire::DecodeError::TrailingBytes(remaining) = error {
        return ProtocolError::TrailingPacketBytes { remaining };
    }
    match crate::ui::validation::semantic_rejection(&error) {
        Some(rejection) => rejection.into(),
        None => error.into(),
    }
}

/// Reclassifies a play-stream decode failure with intact framing (a broken UI budget, an unknown
/// selector or a schema bound) as a skippable world rejection; every other failure stays fatal.
pub(crate) fn world_decode_error(error: SessionError) -> ProtocolError {
    if let SessionFailure::PacketDecode { source, .. } = &error.0
        && let Some(rejection) = crate::ui::validation::semantic_rejection(source)
    {
        return ProtocolError::World(WorldPacketError::Ui(rejection));
    }
    error.into()
}

/// Encodes one packet as an uncompressed gophertunnel raw batch.
pub fn encode(packet: &Packet) -> Result<Bytes, ProtocolError> {
    let mut bytes = vec![wire::BATCH_HEADER];
    encode_entry(packet, &mut bytes)?;
    Ok(Bytes::from(bytes))
}

/// Appends `packet` as one `[length][header][body]` batch entry; on error `out` is unchanged.
pub(crate) fn encode_entry(packet: &Packet, out: &mut Vec<u8>) -> Result<(), ProtocolError> {
    validate_packet(packet)?;
    let header = wire::PacketHeader {
        id: packet.header.id as u32,
        // `validate_packet` bounds both subclients to two bits.
        sender_subclient: packet.header.from_subclient as u8,
        target_subclient: packet.header.to_subclient as u8,
    };
    wire::encode_frame_with(out, header, usize::MAX, |writer| packet.data.encode(writer))?;
    Ok(())
}

pub(crate) fn validate_packet(packet: &Packet) -> Result<(), ProtocolError> {
    let payload_id = packet.data.id();
    if packet.header.id != payload_id {
        return Err(ProtocolError::HeaderIdMismatch {
            header: packet.kind(),
            payload: PacketKind(payload_id),
        });
    }
    if packet.header.from_subclient > 3 || packet.header.to_subclient > 3 {
        return Err(ProtocolError::InvalidSubclient {
            sender: packet.header.from_subclient,
            target: packet.header.to_subclient,
        });
    }
    Ok(())
}
