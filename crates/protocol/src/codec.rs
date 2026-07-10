use bytes::{Buf, BufMut, Bytes, BytesMut};
use thiserror::Error;
use valentine::bedrock::context::BedrockSession;
use valentine::bedrock::version::v1_26_30::{McpePacketData, McpePacketName};
use valentine::protocol::wire;

use crate::Packet;
use crate::world::WorldPacketError;

const BATCH_HEADER: u8 = 0xfe;
const MAX_BATCH_BYTES: usize = 16 * 1024 * 1024;
const MAX_BATCH_PACKETS: usize = 1_600;

/// Errors produced by raw Bedrock batch encoding and decoding.
#[derive(Debug, Error)]
pub enum ProtocolError {
    #[error("bridge connection failed: {0}")]
    Bridge(#[source] anyhow::Error),

    #[error("Bedrock session failed: {0}")]
    Session(#[from] jolyne::error::JolyneError),

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
        header: McpePacketName,
        payload: McpePacketName,
    },

    #[error("subclient IDs must be in 0..=3, got sender {sender} and target {target}")]
    InvalidSubclient { sender: u32, target: u32 },

    #[error("packet decode failed: {0}")]
    Decode(#[from] valentine::bedrock::error::DecodeError),

    #[error("packet encode failed: {0}")]
    Encode(#[from] std::io::Error),

    #[error("world packet normalization failed: {0}")]
    World(#[from] WorldPacketError),
}

/// Decodes an uncompressed gophertunnel raw batch into owned protocol packets.
pub fn decode_batch(
    mut bytes: Bytes,
    session: &BedrockSession,
) -> Result<Vec<Packet>, ProtocolError> {
    if bytes.len() > MAX_BATCH_BYTES {
        return Err(ProtocolError::BatchTooLarge {
            actual: bytes.len(),
            max: MAX_BATCH_BYTES,
        });
    }
    if bytes.first().copied() != Some(BATCH_HEADER) {
        return Err(ProtocolError::InvalidBatchHeader {
            actual: bytes.first().copied(),
        });
    }
    bytes.advance(1);

    let mut packets = Vec::new();
    while bytes.has_remaining() {
        if packets.len() == MAX_BATCH_PACKETS {
            return Err(ProtocolError::TooManyPackets {
                max: MAX_BATCH_PACKETS,
            });
        }

        let frame_start = bytes.clone();
        let before_length = bytes.remaining();
        let declared = wire::read_var_u32(&mut bytes)? as usize;
        let length_prefix = before_length - bytes.remaining();
        if bytes.remaining() < declared {
            return Err(ProtocolError::TruncatedPacket {
                declared,
                available: bytes.remaining(),
            });
        }

        let mut frame = frame_start.slice(..length_prefix + declared);
        bytes.advance(declared);
        let (header, data) = McpePacketData::decode_inner(&mut frame, session.into())?;
        if frame.has_remaining() {
            return Err(ProtocolError::TrailingPacketBytes {
                remaining: frame.remaining(),
            });
        }
        packets.push(Packet::new(header, data));
    }
    Ok(packets)
}

/// Encodes one packet as an uncompressed gophertunnel raw batch.
pub fn encode(packet: &Packet, _session: &BedrockSession) -> Result<Bytes, ProtocolError> {
    validate_packet(packet)?;

    let mut bytes = BytesMut::new();
    bytes.put_u8(BATCH_HEADER);
    packet.data.encode_inner_bytes_mut(
        &mut bytes,
        packet.header.from_subclient,
        packet.header.to_subclient,
    )?;
    Ok(bytes.freeze())
}

pub(crate) fn validate_packet(packet: &Packet) -> Result<(), ProtocolError> {
    let payload_id = packet.data.packet_id();
    if packet.header.id != payload_id {
        return Err(ProtocolError::HeaderIdMismatch {
            header: packet.header.id,
            payload: payload_id,
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
