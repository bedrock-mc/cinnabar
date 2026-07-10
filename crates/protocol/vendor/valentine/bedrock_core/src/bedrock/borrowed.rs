use bytes::{Buf, Bytes};
use std::borrow::Cow;

use crate::bedrock::codec::{BedrockCodec, BedrockSized, U32LE, VarInt};
use crate::bedrock::error::DecodeError;
use crate::protocol::wire;

pub const GAME_PACKET_ID: u8 = 0xfe;

/// Zero-copy decode for `Bytes`-backed protocol views.
pub trait BedrockBorrowDecode: Sized {
    type Args;

    fn borrow_decode(buf: &mut Bytes, args: Self::Args) -> Result<Self, DecodeError>;
}

/// Borrowed UTF-8-ish string storage backed by `Bytes`.
#[derive(Debug, Clone, PartialEq, Eq, Hash, Default)]
pub struct BorrowedStr(Bytes);

impl BorrowedStr {
    pub fn as_bytes(&self) -> &[u8] {
        &self.0
    }

    pub fn as_str(&self) -> Result<&str, std::str::Utf8Error> {
        std::str::from_utf8(&self.0)
    }

    pub fn to_string_lossy(&self) -> Cow<'_, str> {
        String::from_utf8_lossy(&self.0)
    }

    pub fn into_bytes(self) -> Bytes {
        self.0
    }
}

impl From<Bytes> for BorrowedStr {
    fn from(value: Bytes) -> Self {
        Self(value)
    }
}

impl AsRef<[u8]> for BorrowedStr {
    fn as_ref(&self) -> &[u8] {
        self.as_bytes()
    }
}

impl BedrockSized for BorrowedStr {
    fn encoded_size(&self) -> usize {
        self.0.len()
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct RawMcpeHeader {
    pub id_raw: u32,
    pub from_subclient: u32,
    pub to_subclient: u32,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RawMcpeFrame {
    pub header: RawMcpeHeader,
    pub payload: Bytes,
}

impl RawMcpeFrame {
    /// Decodes `[0xFE] [Length] [Header] [Body]` directly from a `Bytes` buffer.
    pub fn decode(buf: &mut Bytes) -> Result<Self, DecodeError> {
        if !buf.has_remaining() {
            return Err(DecodeError::UnexpectedEof {
                needed: 1,
                available: 0,
            });
        }

        let leading = buf.get_u8();
        if leading != GAME_PACKET_ID {
            return Err(DecodeError::InvalidMagicByte {
                expected: GAME_PACKET_ID,
                actual: leading,
            });
        }

        let declared_len = wire::read_var_u32(buf)? as usize;
        let mut frame = take_exact_packet_bytes(buf, declared_len)?;
        let header_raw = wire::read_var_u32(&mut frame)?;
        let payload = frame.split_to(frame.remaining());

        Ok(Self {
            header: RawMcpeHeader {
                id_raw: header_raw & 0x3ff,
                from_subclient: (header_raw >> 10) & 0x3,
                to_subclient: (header_raw >> 12) & 0x3,
            },
            payload,
        })
    }
}

pub fn take_var_u32_prefixed_bytes(buf: &mut Bytes) -> Result<Bytes, DecodeError> {
    let len = wire::read_var_u32(buf)? as usize;
    take_exact_array_bytes(buf, len)
}

pub fn take_varint_prefixed_bytes(buf: &mut Bytes) -> Result<Bytes, DecodeError> {
    let len_raw = VarInt::decode(buf, ())?.0 as i64;
    if len_raw < 0 {
        return Err(DecodeError::NegativeLength { value: len_raw });
    }
    take_exact_array_bytes(buf, len_raw as usize)
}

pub fn take_u32le_prefixed_bytes(buf: &mut Bytes) -> Result<Bytes, DecodeError> {
    let len = U32LE::decode(buf, ())?.0 as usize;
    take_exact_string_bytes(buf, len)
}

pub fn take_var_u32_prefixed_string(buf: &mut Bytes) -> Result<BorrowedStr, DecodeError> {
    Ok(BorrowedStr(take_var_u32_prefixed_bytes(buf)?))
}

pub fn take_varint_prefixed_string(buf: &mut Bytes) -> Result<BorrowedStr, DecodeError> {
    Ok(BorrowedStr(take_varint_prefixed_bytes(buf)?))
}

pub fn take_u32le_prefixed_string(buf: &mut Bytes) -> Result<BorrowedStr, DecodeError> {
    Ok(BorrowedStr(take_u32le_prefixed_bytes(buf)?))
}

fn take_exact_array_bytes(buf: &mut Bytes, len: usize) -> Result<Bytes, DecodeError> {
    if buf.remaining() < len {
        return Err(DecodeError::ArrayLengthExceeded {
            declared: len,
            available: buf.remaining(),
        });
    }
    Ok(buf.split_to(len))
}

fn take_exact_packet_bytes(buf: &mut Bytes, len: usize) -> Result<Bytes, DecodeError> {
    if buf.remaining() < len {
        return Err(DecodeError::PacketLengthExceeded {
            declared: len,
            available: buf.remaining(),
        });
    }
    Ok(buf.split_to(len))
}

fn take_exact_string_bytes(buf: &mut Bytes, len: usize) -> Result<Bytes, DecodeError> {
    if buf.remaining() < len {
        return Err(DecodeError::StringLengthExceeded {
            declared: len,
            available: buf.remaining(),
        });
    }
    Ok(buf.split_to(len))
}
