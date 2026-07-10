use bytes::{Buf, BufMut, Bytes};
use std::io::Cursor;
use std::mem;

use crate::bedrock::context::BedrockSession;
use crate::bedrock::error::DecodeError;
use crate::protocol::wire;

/// Bedrock binary codec for encode/decode on the wire.
pub trait BedrockCodec: Sized {
    type Args;

    fn encode<B: BufMut>(&self, buf: &mut B) -> Result<(), std::io::Error>;
    fn decode<B: Buf>(buf: &mut B, args: Self::Args) -> Result<Self, DecodeError>;
}

/// Computes the exact encoded wire size for a value without writing it.
pub trait BedrockSized {
    fn encoded_size(&self) -> usize;
}

pub fn decode_utf8_lossy_owned(bytes: Vec<u8>) -> String {
    match String::from_utf8(bytes) {
        Ok(s) => s,
        Err(err) => String::from_utf8_lossy(&err.into_bytes()).into_owned(),
    }
}

#[derive(Clone)]
pub struct ProtocolArgs<'a> {
    pub shield_id: i32,
    pub session: &'a BedrockSession,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct ZigZag32(pub i32);

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct ZigZag64(pub i64);

macro_rules! le_int_newtype {
    ($name:ident, $inner:ty, $put:ident, $get:ident) => {
        #[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
        pub struct $name(pub $inner);

        impl BedrockCodec for $name {
            type Args = ();
            fn encode<B: BufMut>(&self, buf: &mut B) -> Result<(), std::io::Error> {
                buf.$put(self.0);
                Ok(())
            }
            fn decode<B: Buf>(buf: &mut B, _args: Self::Args) -> Result<Self, DecodeError> {
                if buf.remaining() < mem::size_of::<$inner>() {
                    return Err(DecodeError::UnexpectedEof {
                        needed: mem::size_of::<$inner>(),
                        available: buf.remaining(),
                    });
                }
                Ok(Self(buf.$get()))
            }
        }

        impl BedrockSized for $name {
            fn encoded_size(&self) -> usize {
                mem::size_of::<$inner>()
            }
        }
    };
}

macro_rules! le_float_newtype {
    ($name:ident, $inner:ty, $put:ident, $get:ident) => {
        #[derive(Debug, Clone, Copy, PartialEq)]
        pub struct $name(pub $inner);

        impl BedrockCodec for $name {
            type Args = ();
            fn encode<B: BufMut>(&self, buf: &mut B) -> Result<(), std::io::Error> {
                buf.$put(self.0);
                Ok(())
            }
            fn decode<B: Buf>(buf: &mut B, _args: Self::Args) -> Result<Self, DecodeError> {
                if buf.remaining() < mem::size_of::<$inner>() {
                    return Err(DecodeError::UnexpectedEof {
                        needed: mem::size_of::<$inner>(),
                        available: buf.remaining(),
                    });
                }
                Ok(Self(buf.$get()))
            }
        }

        impl BedrockSized for $name {
            fn encoded_size(&self) -> usize {
                mem::size_of::<$inner>()
            }
        }
    };
}

le_int_newtype!(U16LE, u16, put_u16_le, get_u16_le);
le_int_newtype!(I16LE, i16, put_i16_le, get_i16_le);
le_int_newtype!(U32LE, u32, put_u32_le, get_u32_le);
le_int_newtype!(I32LE, i32, put_i32_le, get_i32_le);
le_int_newtype!(U64LE, u64, put_u64_le, get_u64_le);
le_int_newtype!(I64LE, i64, put_i64_le, get_i64_le);
le_float_newtype!(F32LE, f32, put_f32_le, get_f32_le);
le_float_newtype!(F64LE, f64, put_f64_le, get_f64_le);

impl BedrockCodec for ZigZag32 {
    type Args = ();
    fn encode<B: BufMut>(&self, buf: &mut B) -> Result<(), std::io::Error> {
        wire::write_zigzag32(buf, self.0);
        Ok(())
    }
    fn decode<B: Buf>(buf: &mut B, _args: Self::Args) -> Result<Self, DecodeError> {
        Ok(ZigZag32(wire::read_zigzag32(buf)?))
    }
}

impl BedrockSized for ZigZag32 {
    fn encoded_size(&self) -> usize {
        wire::var_u32_len(wire::zigzag32_encode(self.0))
    }
}

impl BedrockCodec for ZigZag64 {
    type Args = ();
    fn encode<B: BufMut>(&self, buf: &mut B) -> Result<(), std::io::Error> {
        wire::write_zigzag64(buf, self.0);
        Ok(())
    }
    fn decode<B: Buf>(buf: &mut B, _args: Self::Args) -> Result<Self, DecodeError> {
        Ok(ZigZag64(wire::read_zigzag64(buf)?))
    }
}

impl BedrockSized for ZigZag64 {
    fn encoded_size(&self) -> usize {
        wire::var_u64_len(wire::zigzag64_encode(self.0))
    }
}

macro_rules! fixed_size_codec {
    ($ty:ty) => {
        impl BedrockSized for $ty {
            fn encoded_size(&self) -> usize {
                mem::size_of::<$ty>()
            }
        }
    };
}

impl BedrockCodec for bool {
    type Args = ();
    fn encode<B: BufMut>(&self, buf: &mut B) -> Result<(), std::io::Error> {
        buf.put_u8(u8::from(*self));
        Ok(())
    }
    fn decode<B: Buf>(buf: &mut B, _args: Self::Args) -> Result<Self, DecodeError> {
        if !buf.has_remaining() {
            return Err(DecodeError::UnexpectedEof {
                needed: 1,
                available: 0,
            });
        }
        Ok(buf.get_u8() != 0)
    }
}
fixed_size_codec!(bool);

impl BedrockCodec for u8 {
    type Args = ();
    fn encode<B: BufMut>(&self, buf: &mut B) -> Result<(), std::io::Error> {
        buf.put_u8(*self);
        Ok(())
    }
    fn decode<B: Buf>(buf: &mut B, _args: Self::Args) -> Result<Self, DecodeError> {
        if !buf.has_remaining() {
            Err(DecodeError::UnexpectedEof {
                needed: 1,
                available: 0,
            })
        } else {
            Ok(buf.get_u8())
        }
    }
}
fixed_size_codec!(u8);
impl BedrockCodec for i8 {
    type Args = ();
    fn encode<B: BufMut>(&self, buf: &mut B) -> Result<(), std::io::Error> {
        buf.put_i8(*self);
        Ok(())
    }
    fn decode<B: Buf>(buf: &mut B, _args: Self::Args) -> Result<Self, DecodeError> {
        if !buf.has_remaining() {
            Err(DecodeError::UnexpectedEof {
                needed: 1,
                available: 0,
            })
        } else {
            Ok(buf.get_i8())
        }
    }
}
fixed_size_codec!(i8);
impl BedrockCodec for u16 {
    type Args = ();
    fn encode<B: BufMut>(&self, buf: &mut B) -> Result<(), std::io::Error> {
        buf.put_u16(*self);
        Ok(())
    }
    fn decode<B: Buf>(buf: &mut B, _args: Self::Args) -> Result<Self, DecodeError> {
        if buf.remaining() < 2 {
            Err(DecodeError::UnexpectedEof {
                needed: 2,
                available: buf.remaining(),
            })
        } else {
            Ok(buf.get_u16())
        }
    }
}
fixed_size_codec!(u16);
impl BedrockCodec for i16 {
    type Args = ();
    fn encode<B: BufMut>(&self, buf: &mut B) -> Result<(), std::io::Error> {
        buf.put_i16(*self);
        Ok(())
    }
    fn decode<B: Buf>(buf: &mut B, _args: Self::Args) -> Result<Self, DecodeError> {
        if buf.remaining() < 2 {
            Err(DecodeError::UnexpectedEof {
                needed: 2,
                available: buf.remaining(),
            })
        } else {
            Ok(buf.get_i16())
        }
    }
}
fixed_size_codec!(i16);
impl BedrockCodec for u32 {
    type Args = ();
    fn encode<B: BufMut>(&self, buf: &mut B) -> Result<(), std::io::Error> {
        buf.put_u32(*self);
        Ok(())
    }
    fn decode<B: Buf>(buf: &mut B, _args: Self::Args) -> Result<Self, DecodeError> {
        if buf.remaining() < 4 {
            Err(DecodeError::UnexpectedEof {
                needed: 4,
                available: buf.remaining(),
            })
        } else {
            Ok(buf.get_u32())
        }
    }
}
fixed_size_codec!(u32);
impl BedrockCodec for i32 {
    type Args = ();
    fn encode<B: BufMut>(&self, buf: &mut B) -> Result<(), std::io::Error> {
        buf.put_i32(*self);
        Ok(())
    }
    fn decode<B: Buf>(buf: &mut B, _args: Self::Args) -> Result<Self, DecodeError> {
        if buf.remaining() < 4 {
            Err(DecodeError::UnexpectedEof {
                needed: 4,
                available: buf.remaining(),
            })
        } else {
            Ok(buf.get_i32())
        }
    }
}
fixed_size_codec!(i32);
impl BedrockCodec for u64 {
    type Args = ();
    fn encode<B: BufMut>(&self, buf: &mut B) -> Result<(), std::io::Error> {
        buf.put_u64(*self);
        Ok(())
    }
    fn decode<B: Buf>(buf: &mut B, _args: Self::Args) -> Result<Self, DecodeError> {
        if buf.remaining() < 8 {
            Err(DecodeError::UnexpectedEof {
                needed: 8,
                available: buf.remaining(),
            })
        } else {
            Ok(buf.get_u64())
        }
    }
}
fixed_size_codec!(u64);
impl BedrockCodec for i64 {
    type Args = ();
    fn encode<B: BufMut>(&self, buf: &mut B) -> Result<(), std::io::Error> {
        buf.put_i64(*self);
        Ok(())
    }
    fn decode<B: Buf>(buf: &mut B, _args: Self::Args) -> Result<Self, DecodeError> {
        if buf.remaining() < 8 {
            Err(DecodeError::UnexpectedEof {
                needed: 8,
                available: buf.remaining(),
            })
        } else {
            Ok(buf.get_i64())
        }
    }
}
fixed_size_codec!(i64);

impl BedrockCodec for f32 {
    type Args = ();
    fn encode<B: BufMut>(&self, buf: &mut B) -> Result<(), std::io::Error> {
        buf.put_f32(*self);
        Ok(())
    }
    fn decode<B: Buf>(buf: &mut B, _args: Self::Args) -> Result<Self, DecodeError> {
        if buf.remaining() < 4 {
            Err(DecodeError::UnexpectedEof {
                needed: 4,
                available: buf.remaining(),
            })
        } else {
            Ok(buf.get_f32())
        }
    }
}
fixed_size_codec!(f32);

impl BedrockCodec for f64 {
    type Args = ();
    fn encode<B: BufMut>(&self, buf: &mut B) -> Result<(), std::io::Error> {
        buf.put_f64(*self);
        Ok(())
    }
    fn decode<B: Buf>(buf: &mut B, _args: Self::Args) -> Result<Self, DecodeError> {
        if buf.remaining() < 8 {
            Err(DecodeError::UnexpectedEof {
                needed: 8,
                available: buf.remaining(),
            })
        } else {
            Ok(buf.get_f64())
        }
    }
}
fixed_size_codec!(f64);

impl BedrockCodec for String {
    type Args = ();
    fn encode<B: BufMut>(&self, buf: &mut B) -> Result<(), std::io::Error> {
        let bytes = self.as_bytes();
        crate::protocol::wire::write_var_u32(buf, bytes.len() as u32);
        buf.put_slice(bytes);
        Ok(())
    }
    fn decode<B: Buf>(buf: &mut B, _args: Self::Args) -> Result<Self, DecodeError> {
        let len = crate::protocol::wire::read_var_u32(buf)? as usize;
        if buf.remaining() < len {
            return Err(DecodeError::StringLengthExceeded {
                declared: len,
                available: buf.remaining(),
            });
        }
        let mut v = vec![0u8; len];
        buf.copy_to_slice(&mut v);
        // Bedrock strings are effectively byte strings in the wild. Match gophertunnel's
        // tolerant decoding and avoid rejecting packets that carry non-UTF-8 payloads.
        Ok(decode_utf8_lossy_owned(v))
    }
}

impl BedrockSized for String {
    fn encoded_size(&self) -> usize {
        wire::var_u32_len(self.len() as u32) + self.len()
    }
}

impl<T: BedrockCodec> BedrockCodec for Box<T> {
    type Args = T::Args;
    fn encode<B: BufMut>(&self, buf: &mut B) -> Result<(), std::io::Error> {
        (**self).encode(buf)
    }
    fn decode<B: Buf>(buf: &mut B, args: Self::Args) -> Result<Self, DecodeError> {
        Ok(Box::new(T::decode(buf, args)?))
    }
}

impl<T: BedrockSized> BedrockSized for Box<T> {
    fn encoded_size(&self) -> usize {
        (**self).encoded_size()
    }
}

impl<T: BedrockCodec> BedrockCodec for Vec<T>
where
    T::Args: Clone,
{
    type Args = T::Args;
    fn encode<B: BufMut>(&self, buf: &mut B) -> Result<(), std::io::Error> {
        crate::protocol::wire::write_var_u32(buf, self.len() as u32);
        for item in self {
            item.encode(buf)?;
        }
        Ok(())
    }
    fn decode<B: Buf>(buf: &mut B, args: Self::Args) -> Result<Self, DecodeError> {
        let len = crate::protocol::wire::read_var_u32(buf)? as usize;
        let mut v = Vec::with_capacity(len);
        for _ in 0..len {
            v.push(T::decode(buf, args.clone())?);
        }
        Ok(v)
    }
}

impl<T: BedrockSized> BedrockSized for Vec<T> {
    fn encoded_size(&self) -> usize {
        wire::var_u32_len(self.len() as u32)
            + self.iter().map(BedrockSized::encoded_size).sum::<usize>()
    }
}

impl<T: BedrockCodec> BedrockCodec for Option<T> {
    type Args = T::Args;
    fn encode<B: BufMut>(&self, buf: &mut B) -> Result<(), std::io::Error> {
        match self {
            Some(v) => {
                buf.put_u8(1);
                v.encode(buf)?;
            }
            None => {
                buf.put_u8(0);
            }
        }
        Ok(())
    }
    fn decode<B: Buf>(buf: &mut B, args: Self::Args) -> Result<Self, DecodeError> {
        let present = u8::decode(buf, ())?;
        if present != 0 {
            Ok(Some(T::decode(buf, args)?))
        } else {
            Ok(None)
        }
    }
}

impl<T: BedrockSized> BedrockSized for Option<T> {
    fn encoded_size(&self) -> usize {
        1 + self.as_ref().map_or(0, BedrockSized::encoded_size)
    }
}

impl BedrockCodec for uuid::Uuid {
    type Args = ();
    fn encode<B: BufMut>(&self, buf: &mut B) -> Result<(), std::io::Error> {
        buf.put_slice(self.as_bytes());
        Ok(())
    }

    fn decode<B: Buf>(buf: &mut B, _args: Self::Args) -> Result<Self, DecodeError> {
        if buf.remaining() < 16 {
            return Err(DecodeError::UnexpectedEof {
                needed: 16,
                available: buf.remaining(),
            });
        }
        let mut bytes = [0u8; 16];
        buf.copy_to_slice(&mut bytes);
        Ok(uuid::Uuid::from_bytes(bytes))
    }
}

impl BedrockSized for uuid::Uuid {
    fn encoded_size(&self) -> usize {
        16
    }
}

#[derive(Debug, Clone, Copy, PartialEq)]
pub struct VarInt(pub i32);

impl BedrockCodec for VarInt {
    type Args = ();
    fn encode<B: BufMut>(&self, buf: &mut B) -> Result<(), std::io::Error> {
        let mut x = self.0 as u32;
        loop {
            let mut temp = (x & 0x7F) as u8;
            x >>= 7;
            if x != 0 {
                temp |= 0x80;
                buf.put_u8(temp);
            } else {
                buf.put_u8(temp);
                break;
            }
        }
        Ok(())
    }

    fn decode<B: Buf>(buf: &mut B, _args: Self::Args) -> Result<Self, DecodeError> {
        let mut result = 0;
        let mut shift = 0;
        loop {
            if !buf.has_remaining() {
                return Err(DecodeError::UnexpectedEof {
                    needed: 1,
                    available: 0,
                });
            }
            let byte = buf.get_u8();
            result |= ((byte & 0x7F) as i32) << shift;
            if (byte & 0x80) == 0 {
                return Ok(VarInt(result));
            }
            shift += 7;
            if shift >= 35 {
                return Err(DecodeError::VarIntTooLarge);
            }
        }
    }
}

impl BedrockSized for VarInt {
    fn encoded_size(&self) -> usize {
        wire::var_u32_len(self.0 as u32)
    }
}

// --- VarLong Wrapper ---
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct VarLong(pub i64);

impl BedrockCodec for VarLong {
    type Args = ();
    fn encode<B: BufMut>(&self, buf: &mut B) -> Result<(), std::io::Error> {
        let mut x = self.0 as u64;
        loop {
            let mut temp = (x & 0x7F) as u8;
            x >>= 7;
            if x != 0 {
                temp |= 0x80;
                buf.put_u8(temp);
            } else {
                buf.put_u8(temp);
                break;
            }
        }
        Ok(())
    }

    fn decode<B: Buf>(buf: &mut B, _args: Self::Args) -> Result<Self, DecodeError> {
        let mut result = 0;
        let mut shift = 0;
        loop {
            if !buf.has_remaining() {
                return Err(DecodeError::UnexpectedEof {
                    needed: 1,
                    available: 0,
                });
            }
            let byte = buf.get_u8();
            result |= ((byte & 0x7F) as i64) << shift;
            if (byte & 0x80) == 0 {
                return Ok(VarLong(result));
            }
            shift += 7;
            if shift >= 70 {
                return Err(DecodeError::VarLongTooLarge);
            }
        }
    }
}

impl BedrockSized for VarLong {
    fn encoded_size(&self) -> usize {
        wire::var_u64_len(self.0 as u64)
    }
}

pub trait GamePacket: BedrockCodec {
    type PacketId;
    const PACKET_ID: Self::PacketId;
}

#[derive(Debug, Clone, PartialEq)]
pub struct Nbt(pub Bytes);

impl Default for Nbt {
    fn default() -> Self {
        // NetworkLittleEndian empty compound:
        // 0x0a (Tag Compound)
        // 0x00 (Name Length = 0, VarInt)
        // 0x00 (Tag End)
        Self(vec![0x0a, 0x00, 0x00].into())
    }
}

impl super::codec::BedrockCodec for Nbt {
    type Args = ();

    fn encode<B: BufMut>(&self, buf: &mut B) -> Result<(), std::io::Error> {
        // Just write the blob.
        buf.put_slice(&self.0);
        Ok(())
    }

    fn decode<B: Buf>(buf: &mut B, _args: Self::Args) -> Result<Self, DecodeError> {
        let chunk = buf.chunk();

        let mut cursor = Cursor::new(chunk);

        let root_tag = read_u8(&mut cursor)?;

        // 2. Read the Root Name
        // Even if empty, the root tag has a name field (2 bytes for length 0)
        skip_string(&mut cursor)?;

        // 3. Scan ONLY the payload of the root tag
        // If root is Compound (10), this calls scan_compound recursively
        // to handle the inner list, which is correct.
        scan_payload(root_tag, &mut cursor)?;
        // --- FIXED LOGIC END ---

        let len = cursor.position() as usize;
        let data = buf.copy_to_bytes(len);
        Ok(Nbt(data))
    }
}

impl BedrockSized for Nbt {
    fn encoded_size(&self) -> usize {
        self.0.len()
    }
}

impl BedrockSized for Bytes {
    fn encoded_size(&self) -> usize {
        self.len()
    }
}

impl BedrockSized for () {
    fn encoded_size(&self) -> usize {
        0
    }
}

impl<T: BedrockSized, const N: usize> BedrockSized for [T; N] {
    fn encoded_size(&self) -> usize {
        self.iter().map(BedrockSized::encoded_size).sum()
    }
}

// --- The Scanner Logic (Little Endian) ---

fn scan_compound(cursor: &mut Cursor<&[u8]>) -> Result<(), DecodeError> {
    // A Compound is just a list of tags terminated by End (0x00)
    loop {
        let tag_id = read_u8(cursor)?;
        if tag_id == 0 {
            // Tag_End
            break;
        }

        // Tags in a compound are named.
        // Read Name (Short Length + Bytes)
        skip_string(cursor)?;

        // Skip the payload based on ID
        scan_payload(tag_id, cursor)?;
    }
    Ok(())
}

fn scan_payload(tag_id: u8, cursor: &mut Cursor<&[u8]>) -> Result<(), DecodeError> {
    use crate::protocol::wire;
    match tag_id {
        1 => skip(cursor, 1), // Byte
        2 => skip(cursor, 2), // Short
        3 => {
            // Int (ZigZag32)
            wire::read_zigzag32(cursor)?;
            Ok(())
        }
        4 => {
            // Long (ZigZag64)
            wire::read_zigzag64(cursor)?;
            Ok(())
        }
        5 => skip(cursor, 4), // Float
        6 => skip(cursor, 8), // Double
        7 => {
            // Byte Array (ZigZag32 Length + Bytes)
            let len = wire::read_zigzag32(cursor)?;
            skip(cursor, len as usize)
        }
        8 => skip_string(cursor), // String
        9 => {
            // List (TagId + ZigZag32 Length + Payloads)
            let inner_id = read_u8(cursor)?;
            let count = wire::read_zigzag32(cursor)?;
            if count > 0 {
                for _ in 0..count {
                    scan_payload(inner_id, cursor)?;
                }
            }
            Ok(())
        }
        10 => scan_compound(cursor), // Compound (Recursion)
        11 => {
            // Int Array (ZigZag32 Length + ZigZag32s)
            let len = wire::read_zigzag32(cursor)?;
            for _ in 0..len {
                wire::read_zigzag32(cursor)?;
            }
            Ok(())
        }
        12 => {
            // Long Array (ZigZag32 Length + ZigZag64s)
            let len = wire::read_zigzag32(cursor)?;
            for _ in 0..len {
                wire::read_zigzag64(cursor)?;
            }
            Ok(())
        }
        _ => Err(DecodeError::UnknownNbtTag { tag_id }),
    }
}

// --- Low Level Helpers ---

fn read_u8(cursor: &mut Cursor<&[u8]>) -> Result<u8, DecodeError> {
    if !cursor.has_remaining() {
        return Err(DecodeError::UnexpectedEof {
            needed: 1,
            available: 0,
        });
    }
    Ok(cursor.get_u8())
}

fn skip_string(cursor: &mut Cursor<&[u8]>) -> Result<(), DecodeError> {
    let len = crate::protocol::wire::read_var_u32(cursor)? as usize;
    skip(cursor, len)
}

fn skip(cursor: &mut Cursor<&[u8]>, n: usize) -> Result<(), DecodeError> {
    if cursor.remaining() < n {
        return Err(DecodeError::UnexpectedEof {
            needed: n,
            available: cursor.remaining(),
        });
    }
    cursor.advance(n);
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use bytes::BytesMut;

    /// Helper to assert roundtrip encoding/decoding for BedrockCodec types
    fn assert_codec_roundtrip<T>(value: T, args: T::Args)
    where
        T: BedrockCodec + PartialEq + std::fmt::Debug,
        T::Args: Clone,
    {
        let mut buf = BytesMut::new();
        value.encode(&mut buf).expect("encode should succeed");
        let mut reader = buf.freeze();
        let decoded = T::decode(&mut reader, args).expect("decode should succeed");
        assert_eq!(value, decoded);
        assert!(!reader.has_remaining(), "should consume all bytes");
    }

    // ========== Primitive Tests ==========

    #[test]
    fn bool_roundtrip() {
        assert_codec_roundtrip(true, ());
        assert_codec_roundtrip(false, ());
    }

    #[test]
    fn bool_encoding() {
        let mut buf = BytesMut::new();
        true.encode(&mut buf).unwrap();
        assert_eq!(buf.as_ref(), &[0x01]);

        buf.clear();
        false.encode(&mut buf).unwrap();
        assert_eq!(buf.as_ref(), &[0x00]);
    }

    #[test]
    fn u8_roundtrip() {
        for value in [0u8, 1, 127, 128, 255] {
            assert_codec_roundtrip(value, ());
        }
    }

    #[test]
    fn i8_roundtrip() {
        for value in [0i8, 1, -1, 127, -128] {
            assert_codec_roundtrip(value, ());
        }
    }

    #[test]
    fn u16_roundtrip() {
        for value in [0u16, 1, 255, 256, u16::MAX] {
            assert_codec_roundtrip(value, ());
        }
    }

    #[test]
    fn i16_roundtrip() {
        for value in [0i16, 1, -1, i16::MAX, i16::MIN] {
            assert_codec_roundtrip(value, ());
        }
    }

    #[test]
    fn u32_roundtrip() {
        for value in [0u32, 1, 255, 65535, u32::MAX] {
            assert_codec_roundtrip(value, ());
        }
    }

    #[test]
    fn i32_roundtrip() {
        for value in [0i32, 1, -1, i32::MAX, i32::MIN] {
            assert_codec_roundtrip(value, ());
        }
    }

    #[test]
    fn u64_roundtrip() {
        for value in [0u64, 1, u32::MAX as u64, u64::MAX] {
            assert_codec_roundtrip(value, ());
        }
    }

    #[test]
    fn i64_roundtrip() {
        for value in [0i64, 1, -1, i64::MAX, i64::MIN] {
            assert_codec_roundtrip(value, ());
        }
    }

    #[test]
    fn f32_roundtrip() {
        for value in [0.0f32, 1.0, -1.0, f32::MIN, f32::MAX, std::f32::consts::PI] {
            assert_codec_roundtrip(value, ());
        }
    }

    #[test]
    fn f64_roundtrip() {
        for value in [0.0f64, 1.0, -1.0, f64::MIN, f64::MAX, std::f64::consts::PI] {
            assert_codec_roundtrip(value, ());
        }
    }

    // ========== Little-Endian Newtype Tests ==========

    #[test]
    fn u16le_roundtrip() {
        for value in [0u16, 1, 255, 256, u16::MAX] {
            assert_codec_roundtrip(U16LE(value), ());
        }
    }

    #[test]
    fn u16le_encoding_is_little_endian() {
        let mut buf = BytesMut::new();
        U16LE(0x0102).encode(&mut buf).unwrap();
        assert_eq!(buf.as_ref(), &[0x02, 0x01]); // Little-endian
    }

    #[test]
    fn i16le_roundtrip() {
        for value in [0i16, 1, -1, i16::MAX, i16::MIN] {
            assert_codec_roundtrip(I16LE(value), ());
        }
    }

    #[test]
    fn u32le_roundtrip() {
        for value in [0u32, 1, 255, 65535, u32::MAX] {
            assert_codec_roundtrip(U32LE(value), ());
        }
    }

    #[test]
    fn u32le_encoding_is_little_endian() {
        let mut buf = BytesMut::new();
        U32LE(0x01020304).encode(&mut buf).unwrap();
        assert_eq!(buf.as_ref(), &[0x04, 0x03, 0x02, 0x01]);
    }

    #[test]
    fn i32le_roundtrip() {
        for value in [0i32, 1, -1, i32::MAX, i32::MIN] {
            assert_codec_roundtrip(I32LE(value), ());
        }
    }

    #[test]
    fn u64le_roundtrip() {
        for value in [0u64, 1, u32::MAX as u64, u64::MAX] {
            assert_codec_roundtrip(U64LE(value), ());
        }
    }

    #[test]
    fn i64le_roundtrip() {
        for value in [0i64, 1, -1, i64::MAX, i64::MIN] {
            assert_codec_roundtrip(I64LE(value), ());
        }
    }

    #[test]
    fn f32le_roundtrip() {
        for value in [0.0f32, 1.0, -1.0, std::f32::consts::PI] {
            assert_codec_roundtrip(F32LE(value), ());
        }
    }

    #[test]
    fn f64le_roundtrip() {
        for value in [0.0f64, 1.0, -1.0, std::f64::consts::PI] {
            assert_codec_roundtrip(F64LE(value), ());
        }
    }

    // ========== ZigZag Wrapper Tests ==========

    #[test]
    fn zigzag32_roundtrip() {
        for value in [0, 1, -1, 127, -128, i32::MAX, i32::MIN] {
            assert_codec_roundtrip(ZigZag32(value), ());
        }
    }

    #[test]
    fn zigzag32_encoding() {
        let mut buf = BytesMut::new();
        ZigZag32(1).encode(&mut buf).unwrap();
        // ZigZag(1) = 2, VarInt(2) = [0x02]
        assert_eq!(buf.as_ref(), &[0x02]);

        buf.clear();
        ZigZag32(-1).encode(&mut buf).unwrap();
        // ZigZag(-1) = 1, VarInt(1) = [0x01]
        assert_eq!(buf.as_ref(), &[0x01]);
    }

    #[test]
    fn zigzag64_roundtrip() {
        for value in [0, 1, -1, i64::MAX, i64::MIN] {
            assert_codec_roundtrip(ZigZag64(value), ());
        }
    }

    // ========== VarInt/VarLong Tests ==========

    #[test]
    fn varint_roundtrip() {
        for value in [0, 1, 127, 128, i32::MAX] {
            assert_codec_roundtrip(VarInt(value), ());
        }
    }

    #[test]
    fn varint_encoding() {
        let mut buf = BytesMut::new();
        VarInt(0).encode(&mut buf).unwrap();
        assert_eq!(buf.as_ref(), &[0x00]);

        buf.clear();
        VarInt(127).encode(&mut buf).unwrap();
        assert_eq!(buf.as_ref(), &[0x7F]);

        buf.clear();
        VarInt(128).encode(&mut buf).unwrap();
        assert_eq!(buf.as_ref(), &[0x80, 0x01]);
    }

    #[test]
    fn varlong_roundtrip() {
        for value in [0, 1, 127, 128, i64::MAX] {
            assert_codec_roundtrip(VarLong(value), ());
        }
    }

    // ========== String Tests ==========

    #[test]
    fn string_roundtrip() {
        for value in ["", "hello", "hello world", "こんにちは"] {
            assert_codec_roundtrip(value.to_string(), ());
        }
    }

    #[test]
    fn string_encoding() {
        let mut buf = BytesMut::new();
        "hi".to_string().encode(&mut buf).unwrap();
        // Length (VarInt 2) + "hi"
        assert_eq!(buf.as_ref(), &[0x02, b'h', b'i']);
    }

    #[test]
    fn string_empty() {
        let mut buf = BytesMut::new();
        String::new().encode(&mut buf).unwrap();
        assert_eq!(buf.as_ref(), &[0x00]); // Just length 0
    }

    // ========== UUID Tests ==========

    #[test]
    fn uuid_roundtrip() {
        let uuid = uuid::Uuid::new_v4();
        assert_codec_roundtrip(uuid, ());
    }

    #[test]
    fn uuid_nil() {
        let uuid = uuid::Uuid::nil();
        assert_codec_roundtrip(uuid, ());
    }

    // ========== Option Tests ==========

    #[test]
    fn option_some_roundtrip() {
        assert_codec_roundtrip(Some(42i32), ());
        assert_codec_roundtrip(Some("hello".to_string()), ());
    }

    #[test]
    fn option_none_roundtrip() {
        assert_codec_roundtrip(Option::<i32>::None, ());
        assert_codec_roundtrip(Option::<String>::None, ());
    }

    #[test]
    fn option_encoding() {
        let mut buf = BytesMut::new();
        Some(1u8).encode(&mut buf).unwrap();
        assert_eq!(buf.as_ref(), &[0x01, 0x01]); // Present flag + value

        buf.clear();
        Option::<u8>::None.encode(&mut buf).unwrap();
        assert_eq!(buf.as_ref(), &[0x00]); // Just absent flag
    }

    // ========== Vec Tests ==========

    #[test]
    fn vec_roundtrip() {
        assert_codec_roundtrip(vec![1u8, 2, 3], ());
        assert_codec_roundtrip(vec![1i32, 2, 3], ());
        assert_codec_roundtrip(vec!["a".to_string(), "b".to_string()], ());
    }

    #[test]
    fn vec_empty_roundtrip() {
        assert_codec_roundtrip(Vec::<u8>::new(), ());
        assert_codec_roundtrip(Vec::<String>::new(), ());
    }

    #[test]
    fn vec_encoding() {
        let mut buf = BytesMut::new();
        vec![1u8, 2, 3].encode(&mut buf).unwrap();
        // Length (VarInt 3) + elements
        assert_eq!(buf.as_ref(), &[0x03, 0x01, 0x02, 0x03]);
    }

    // ========== Box Tests ==========

    #[test]
    fn box_roundtrip() {
        assert_codec_roundtrip(Box::new(42i32), ());
        assert_codec_roundtrip(Box::new("hello".to_string()), ());
    }

    // ========== Error Tests ==========

    #[test]
    fn u8_decode_empty_buffer() {
        let mut reader = &[][..];
        let err = u8::decode(&mut reader, ()).unwrap_err();
        assert!(matches!(
            err,
            DecodeError::UnexpectedEof {
                needed: 1,
                available: 0
            }
        ));
    }

    #[test]
    fn u32_decode_insufficient_buffer() {
        let mut reader = &[0x01, 0x02][..]; // Only 2 bytes, need 4
        let err = u32::decode(&mut reader, ()).unwrap_err();
        assert!(matches!(
            err,
            DecodeError::UnexpectedEof {
                needed: 4,
                available: 2
            }
        ));
    }

    #[test]
    fn string_decode_insufficient_buffer() {
        let mut reader = &[0x05, b'h', b'i'][..]; // Claims length 5, only 2 bytes
        let err = String::decode(&mut reader, ()).unwrap_err();
        assert!(matches!(
            err,
            DecodeError::StringLengthExceeded {
                declared: 5,
                available: 2
            }
        ));
    }

    #[test]
    fn varint_too_long() {
        // VarInt with 6 continuation bytes
        let mut reader = &[0x80, 0x80, 0x80, 0x80, 0x80, 0x01][..];
        let err = VarInt::decode(&mut reader, ()).unwrap_err();
        assert!(matches!(err, DecodeError::VarIntTooLarge));
    }

    #[test]
    fn varlong_too_long() {
        // VarLong with 11 continuation bytes
        let data = [0x80; 11];
        let mut reader = &data[..];
        let err = VarLong::decode(&mut reader, ()).unwrap_err();
        assert!(matches!(err, DecodeError::VarLongTooLarge));
    }

    // ========== NBT Tests ==========

    #[test]
    fn nbt_default_is_empty_compound() {
        let nbt = Nbt::default();
        // NetworkLittleEndian empty compound: 0x0a (Compound), 0x00 (name len), 0x00 (End)
        assert_eq!(nbt.0.as_ref(), &[0x0a, 0x00, 0x00]);
    }

    #[test]
    fn nbt_default_roundtrip() {
        let nbt = Nbt::default();
        let mut buf = BytesMut::new();
        nbt.encode(&mut buf).unwrap();

        let mut reader = buf.freeze();
        let decoded = Nbt::decode(&mut reader, ()).unwrap();
        assert_eq!(nbt.0, decoded.0);
    }
}
