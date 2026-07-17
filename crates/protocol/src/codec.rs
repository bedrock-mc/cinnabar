use bytes::{Buf, BufMut, Bytes, BytesMut};
use thiserror::Error;
use valentine::bedrock::borrowed::{BorrowedStr, take_varint_prefixed_string};
use valentine::bedrock::codec::{BedrockCodec, I32LE, VarInt, ZigZag64};
use valentine::bedrock::context::BedrockSession;
use valentine::bedrock::version::v1_26_30::{BorrowedMcpePacket, McpePacketData, McpePacketName};
use valentine::protocol::wire;

use crate::Packet;
use crate::ui::{
    MAX_CHAT_AUTOCOMPLETE, MAX_CHAT_AUTOCOMPLETE_BYTES, MAX_CHAT_PARAMETERS,
    MAX_SCORE_ENTRIES_PER_PACKET, MAX_UI_TEXT_BYTES, UiPacketError, validate_borrowed_ui_packet,
};
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

    #[error("UI packet validation failed: {0}")]
    Ui(#[from] UiPacketError),

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
        validate_raw_ui_frame(&frame)?;
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

fn validate_raw_ui_frame(frame: &Bytes) -> Result<(), ProtocolError> {
    let mut probe = frame.clone();
    let _declared = wire::read_var_u32(&mut probe)?;
    let header = wire::read_var_u32(&mut probe)?;
    let packet_id = header & 0x3ff;
    if packet_id == McpePacketName::PacketUpdateSoftEnum as u32 {
        return validate_raw_soft_enum_packet(probe);
    }
    if !matches!(packet_id, 9 | 74 | 88 | 100 | 106 | 107 | 108 | 186) {
        return Ok(());
    }

    if packet_id == McpePacketName::PacketSetScore as u32 {
        return validate_raw_score_packet(probe);
    }
    if packet_id == McpePacketName::PacketText as u32 {
        return validate_raw_text_packet(probe);
    }

    let mut borrowed_frame = frame.clone();
    let packet = BorrowedMcpePacket::decode_inner(&mut borrowed_frame)?;
    if borrowed_frame.has_remaining() {
        return Err(ProtocolError::TrailingPacketBytes {
            remaining: borrowed_frame.remaining(),
        });
    }
    validate_borrowed_ui_packet(&packet.data)?;
    Ok(())
}

fn validate_raw_soft_enum_packet(mut payload: Bytes) -> Result<(), ProtocolError> {
    let enum_name = take_raw_ui_text(&mut payload, "soft_enum.name")?;
    let count_raw = VarInt::decode(&mut payload, ())?.0 as i64;
    if count_raw < 0 {
        return Err(
            valentine::bedrock::error::DecodeError::NegativeLength { value: count_raw }.into(),
        );
    }
    let count = count_raw as usize;
    if count > MAX_CHAT_AUTOCOMPLETE {
        return Err(UiPacketError::TooManyAutocompleteSuggestions {
            count,
            max: MAX_CHAT_AUTOCOMPLETE,
        }
        .into());
    }
    let mut retained_bytes = enum_name.as_bytes().len();
    for _ in 0..count {
        let option = take_raw_ui_text(&mut payload, "soft_enum.option")?;
        retained_bytes = retained_bytes.checked_add(option.as_bytes().len()).ok_or(
            UiPacketError::AutocompleteTooLarge {
                bytes: usize::MAX,
                max: MAX_CHAT_AUTOCOMPLETE_BYTES,
            },
        )?;
        if retained_bytes > MAX_CHAT_AUTOCOMPLETE_BYTES {
            return Err(UiPacketError::AutocompleteTooLarge {
                bytes: retained_bytes,
                max: MAX_CHAT_AUTOCOMPLETE_BYTES,
            }
            .into());
        }
    }
    let action = u8::decode(&mut payload, ())?;
    if action > 2 {
        return Err(UiPacketError::UnknownEnum {
            kind: "soft enum action",
            value: i64::from(action),
        }
        .into());
    }
    Ok(())
}

fn validate_raw_score_packet(mut payload: Bytes) -> Result<(), ProtocolError> {
    let action = u8::decode(&mut payload, ())?;
    if action > 1 {
        return Err(UiPacketError::UnknownEnum {
            kind: "score action",
            value: i64::from(action),
        }
        .into());
    }
    let count_raw = VarInt::decode(&mut payload, ())?.0 as i64;
    if count_raw < 0 {
        return Err(
            valentine::bedrock::error::DecodeError::NegativeLength { value: count_raw }.into(),
        );
    }
    let count = count_raw as usize;
    if count > MAX_SCORE_ENTRIES_PER_PACKET {
        return Err(UiPacketError::TooManyScores {
            count,
            max: MAX_SCORE_ENTRIES_PER_PACKET,
        }
        .into());
    }
    for _ in 0..count {
        let _scoreboard_id = ZigZag64::decode(&mut payload, ())?;
        let objective_name = take_varint_prefixed_string(&mut payload)?;
        validate_raw_utf8(&objective_name, "score.objective_name")?;
        let _score = I32LE::decode(&mut payload, ())?;
        if action == 0 {
            let identity = i8::decode(&mut payload, ())?;
            match identity {
                1 | 2 => {
                    let _entity_id = ZigZag64::decode(&mut payload, ())?;
                }
                3 => {
                    let custom_name = take_varint_prefixed_string(&mut payload)?;
                    validate_raw_utf8(&custom_name, "score.custom_name")?;
                }
                value => {
                    return Err(UiPacketError::UnknownEnum {
                        kind: "score identity",
                        value: i64::from(value),
                    }
                    .into());
                }
            }
        }
    }
    Ok(())
}

fn validate_raw_text_packet(mut payload: Bytes) -> Result<(), ProtocolError> {
    let _needs_translation = bool::decode(&mut payload, ())?;
    let category = u8::decode(&mut payload, ())?;
    if category > 2 {
        return Err(UiPacketError::UnknownEnum {
            kind: "text category",
            value: i64::from(category),
        }
        .into());
    }
    let kind = u8::decode(&mut payload, ())?;
    match kind {
        1 | 7 | 8 => {
            let _source = take_raw_ui_text(&mut payload, "text.source_name")?;
            let _message = take_raw_ui_text(&mut payload, "text.message")?;
        }
        0 | 5 | 6 | 9 | 10 | 11 => {
            let _message = take_raw_ui_text(&mut payload, "text.message")?;
        }
        2..=4 => {
            let _message = take_raw_ui_text(&mut payload, "text.message")?;
            let count_raw = VarInt::decode(&mut payload, ())?.0 as i64;
            if count_raw < 0 {
                return Err(valentine::bedrock::error::DecodeError::NegativeLength {
                    value: count_raw,
                }
                .into());
            }
            let count = count_raw as usize;
            if count > MAX_CHAT_PARAMETERS {
                return Err(UiPacketError::TooManyChatParameters {
                    count,
                    max: MAX_CHAT_PARAMETERS,
                }
                .into());
            }
            for _ in 0..count {
                let _parameter = take_raw_ui_text(&mut payload, "text.parameter")?;
            }
        }
        value => {
            return Err(UiPacketError::UnknownEnum {
                kind: "text type",
                value: i64::from(value),
            }
            .into());
        }
    }
    let _xuid = take_raw_ui_text(&mut payload, "text.xuid")?;
    let _platform_chat_id = take_raw_ui_text(&mut payload, "text.platform_chat_id")?;
    if bool::decode(&mut payload, ())? {
        let _filtered_message = take_raw_ui_text(&mut payload, "text.filtered_message")?;
    }
    Ok(())
}

fn take_raw_ui_text(
    payload: &mut Bytes,
    field: &'static str,
) -> Result<BorrowedStr, ProtocolError> {
    let value = take_varint_prefixed_string(payload)?;
    if value.as_bytes().len() > MAX_UI_TEXT_BYTES {
        return Err(UiPacketError::TextTooLong {
            bytes: value.as_bytes().len(),
            max: MAX_UI_TEXT_BYTES,
        }
        .into());
    }
    validate_raw_utf8(&value, field)?;
    Ok(value)
}

fn validate_raw_utf8(value: &BorrowedStr, field: &'static str) -> Result<(), ProtocolError> {
    value
        .as_str()
        .map(|_| ())
        .map_err(|_| UiPacketError::InvalidUtf8 { field }.into())
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
