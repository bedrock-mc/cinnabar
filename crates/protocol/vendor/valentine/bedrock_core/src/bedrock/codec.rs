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

    /// Opts an intentional zero-width codec into efficient repeated decoding.
    /// Other codecs use the default progress-checked element loop.
    fn decode_repeated<B: Buf>(
        _buf: &mut B,
        _len: usize,
        _args: Self::Args,
    ) -> Option<Result<Vec<Self>, DecodeError>> {
        None
    }
}

/// Computes the exact encoded wire size for a value without writing it.
pub trait BedrockSized {
    fn encoded_size(&self) -> usize;
}

/// Converts a signed wire length without allowing negative values or platform truncation.
pub fn checked_signed_len(value: i128) -> Result<usize, DecodeError> {
    if value < 0 {
        let value = i64::try_from(value).unwrap_or(i64::MIN);
        return Err(DecodeError::NegativeLength { value });
    }
    checked_unsigned_len(value as u128)
}

/// Converts an unsigned wire length without allowing platform truncation.
pub fn checked_unsigned_len(value: u128) -> Result<usize, DecodeError> {
    usize::try_from(value).map_err(|_| DecodeError::ArrayLengthExceeded {
        declared: usize::MAX,
        available: 0,
    })
}

fn allocation_failed(requested: usize) -> DecodeError {
    DecodeError::Io(std::io::Error::new(
        std::io::ErrorKind::OutOfMemory,
        format!("failed to reserve storage for {requested} decoded items"),
    ))
}

/// Creates storage for a decoded collection after applying any statically known
/// lower bound on the encoded size of each item.
pub fn prepare_decode_vec<T>(
    len: usize,
    remaining: usize,
    minimum_element_size: Option<usize>,
) -> Result<Vec<T>, DecodeError> {
    if let Some(minimum_element_size) = minimum_element_size.filter(|size| *size > 0) {
        let required =
            len.checked_mul(minimum_element_size)
                .ok_or(DecodeError::ArrayLengthExceeded {
                    declared: len,
                    available: remaining,
                })?;
        if required > remaining {
            return Err(DecodeError::ArrayLengthExceeded {
                declared: len,
                available: remaining,
            });
        }

        let mut values = Vec::new();
        values
            .try_reserve_exact(len)
            .map_err(|_| allocation_failed(len))?;
        return Ok(values);
    }

    // Unknown-width and zero-width elements must not turn an untrusted count
    // into an eager allocation. Capacity is grown fallibly as items decode.
    Ok(Vec::new())
}

/// Ensures one more decoded item can be pushed without an infallible allocator path.
pub fn reserve_decode_item<T>(values: &mut Vec<T>) -> Result<(), DecodeError> {
    if values.len() == values.capacity() {
        values
            .try_reserve(1)
            .map_err(|_| allocation_failed(values.len().saturating_add(1)))?;
    }
    Ok(())
}

/// Allocates a byte buffer through the fallible collection API.
pub fn allocate_decode_bytes(len: usize) -> Result<Vec<u8>, DecodeError> {
    let mut bytes = Vec::new();
    bytes
        .try_reserve_exact(len)
        .map_err(|_| allocation_failed(len))?;
    bytes.resize(len, 0);
    Ok(bytes)
}

pub fn decode_utf8_lossy_owned(bytes: Vec<u8>) -> String {
    match String::from_utf8(bytes) {
        Ok(s) => s,
        Err(err) => String::from_utf8_lossy(&err.into_bytes()).into_owned(),
    }
}

pub fn try_decode_utf8_lossy_owned(bytes: Vec<u8>) -> Result<String, DecodeError> {
    match String::from_utf8(bytes) {
        Ok(s) => Ok(s),
        Err(err) => {
            let bytes = err.into_bytes();
            let requested = (bytes.len() as u128).saturating_mul(3);
            let capacity = checked_unsigned_len(requested)?;
            let mut output = String::new();
            output
                .try_reserve_exact(capacity)
                .map_err(|_| allocation_failed(capacity))?;

            let mut remaining = bytes.as_slice();
            while !remaining.is_empty() {
                match std::str::from_utf8(remaining) {
                    Ok(valid) => {
                        output.push_str(valid);
                        break;
                    }
                    Err(error) => {
                        let valid_up_to = error.valid_up_to();
                        let valid = std::str::from_utf8(&remaining[..valid_up_to])
                            .expect("Utf8Error::valid_up_to must delimit valid UTF-8");
                        output.push_str(valid);
                        output.push(char::REPLACEMENT_CHARACTER);
                        match error.error_len() {
                            Some(invalid_len) => {
                                remaining = &remaining[valid_up_to + invalid_len..];
                            }
                            None => break,
                        }
                    }
                }
            }
            Ok(output)
        }
    }
}

pub fn decode_latin1_owned(bytes: Vec<u8>) -> Result<String, DecodeError> {
    let requested = (bytes.len() as u128).saturating_mul(2);
    let capacity = checked_unsigned_len(requested)?;
    let mut output = String::new();
    output
        .try_reserve_exact(capacity)
        .map_err(|_| allocation_failed(capacity))?;
    output.extend(bytes.into_iter().map(char::from));
    Ok(output)
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

// An explicitly corrected but still-untyped schema field is represented in the
// shared IR as `Void`/`()`. It consumes no bytes; the frontend warns and records
// the parity gap before reaching this fallback.
impl BedrockCodec for () {
    type Args = ();
    fn encode<B: BufMut>(&self, _buf: &mut B) -> Result<(), std::io::Error> {
        Ok(())
    }
    fn decode<B: Buf>(_buf: &mut B, _args: Self::Args) -> Result<Self, DecodeError> {
        Ok(())
    }

    fn decode_repeated<B: Buf>(
        _buf: &mut B,
        len: usize,
        _args: Self::Args,
    ) -> Option<Result<Vec<Self>, DecodeError>> {
        Some(Ok(vec![(); len]))
    }
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
        let len = checked_unsigned_len(crate::protocol::wire::read_var_u32(buf)? as u128)?;
        if buf.remaining() < len {
            return Err(DecodeError::StringLengthExceeded {
                declared: len,
                available: buf.remaining(),
            });
        }
        let mut v = allocate_decode_bytes(len)?;
        buf.copy_to_slice(&mut v);
        // Bedrock strings are effectively byte strings in the wild. Match gophertunnel's
        // tolerant decoding and avoid rejecting packets that carry non-UTF-8 payloads.
        try_decode_utf8_lossy_owned(v)
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
        let len = checked_unsigned_len(crate::protocol::wire::read_var_u32(buf)? as u128)?;
        if let Some(values) = T::decode_repeated(buf, len, args.clone()) {
            return values;
        }
        let mut v = prepare_decode_vec(len, buf.remaining(), None)?;
        for _ in 0..len {
            let remaining_before = buf.remaining();
            let value = T::decode(buf, args.clone())?;
            if buf.remaining() == remaining_before {
                return Err(DecodeError::ArrayLengthExceeded {
                    declared: len,
                    available: remaining_before,
                });
            }
            reserve_decode_item(&mut v)?;
            v.push(value);
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
        let (most_significant, least_significant) = self.as_u64_pair();
        buf.put_u64_le(most_significant);
        buf.put_u64_le(least_significant);
        Ok(())
    }

    fn decode<B: Buf>(buf: &mut B, _args: Self::Args) -> Result<Self, DecodeError> {
        if buf.remaining() < 16 {
            return Err(DecodeError::UnexpectedEof {
                needed: 16,
                available: buf.remaining(),
            });
        }
        let most_significant = buf.get_u64_le();
        let least_significant = buf.get_u64_le();
        Ok(uuid::Uuid::from_u64_pair(
            most_significant,
            least_significant,
        ))
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

#[derive(Debug, Clone, Copy, PartialEq)]
pub struct VarUInt(pub u32);

impl BedrockCodec for VarUInt {
    type Args = ();

    fn encode<B: BufMut>(&self, buf: &mut B) -> Result<(), std::io::Error> {
        let mut value = self.0;
        loop {
            let mut byte = (value & 0x7f) as u8;
            value >>= 7;
            if value != 0 {
                byte |= 0x80;
            }
            buf.put_u8(byte);
            if value == 0 {
                return Ok(());
            }
        }
    }

    fn decode<B: Buf>(buf: &mut B, _args: Self::Args) -> Result<Self, DecodeError> {
        let mut result = 0u32;
        for shift in (0..35).step_by(7) {
            if !buf.has_remaining() {
                return Err(DecodeError::UnexpectedEof {
                    needed: 1,
                    available: 0,
                });
            }
            let byte = buf.get_u8();
            if shift == 28 && byte & 0xf0 != 0 {
                return Err(DecodeError::VarIntTooLarge);
            }
            result |= u32::from(byte & 0x7f) << shift;
            if byte & 0x80 == 0 {
                if shift > 0 && byte & 0x7f == 0 {
                    return Err(DecodeError::VarIntTooLarge);
                }
                return Ok(Self(result));
            }
        }
        Err(DecodeError::VarIntTooLarge)
    }
}

impl BedrockSized for VarUInt {
    fn encoded_size(&self) -> usize {
        wire::var_u32_len(self.0)
    }
}

#[derive(Debug, Clone, Copy, PartialEq)]
pub struct VarULong(pub u64);

impl BedrockCodec for VarULong {
    type Args = ();

    fn encode<B: BufMut>(&self, buf: &mut B) -> Result<(), std::io::Error> {
        let mut value = self.0;
        loop {
            let mut byte = (value & 0x7f) as u8;
            value >>= 7;
            if value != 0 {
                byte |= 0x80;
            }
            buf.put_u8(byte);
            if value == 0 {
                return Ok(());
            }
        }
    }

    fn decode<B: Buf>(buf: &mut B, _args: Self::Args) -> Result<Self, DecodeError> {
        let mut result = 0u64;
        for shift in (0..70).step_by(7) {
            if !buf.has_remaining() {
                return Err(DecodeError::UnexpectedEof {
                    needed: 1,
                    available: 0,
                });
            }
            let byte = buf.get_u8();
            if shift == 63 && byte & 0xfe != 0 {
                return Err(DecodeError::VarLongTooLarge);
            }
            result |= u64::from(byte & 0x7f) << shift;
            if byte & 0x80 == 0 {
                if shift > 0 && byte & 0x7f == 0 {
                    return Err(DecodeError::VarLongTooLarge);
                }
                return Ok(Self(result));
            }
        }
        Err(DecodeError::VarLongTooLarge)
    }
}

impl BedrockSized for VarULong {
    fn encoded_size(&self) -> usize {
        wire::var_u64_len(self.0)
    }
}

/// Encodes a bounded bitset using seven payload bits per continuation byte.
pub fn encode_bitset<B: BufMut, const N: usize>(
    words: &[u64; N],
    bits: usize,
    buf: &mut B,
) -> Result<(), std::io::Error> {
    let capacity = N.checked_mul(64).ok_or_else(|| {
        std::io::Error::new(std::io::ErrorKind::InvalidInput, "bitset capacity overflow")
    })?;
    if bits == 0 || bits > capacity {
        return Err(std::io::Error::new(
            std::io::ErrorKind::InvalidInput,
            "bitset width exceeds its word storage",
        ));
    }
    if !bits.is_multiple_of(64) && words[N - 1] >> (bits % 64) != 0 {
        return Err(std::io::Error::new(
            std::io::ErrorKind::InvalidInput,
            "bitset contains set bits outside its declared width",
        ));
    }

    let Some(last) = (0..bits)
        .rev()
        .find(|index| words[index / 64] & (1 << (index % 64)) != 0)
    else {
        buf.put_u8(0);
        return Ok(());
    };
    let groups = last / 7 + 1;
    for group in 0..groups {
        let offset = group * 7;
        let width = (bits - offset).min(7);
        let mut value = 0u8;
        for bit in 0..width {
            let index = offset + bit;
            if words[index / 64] & (1 << (index % 64)) != 0 {
                value |= 1 << bit;
            }
        }
        if group + 1 < groups {
            value |= 0x80;
        }
        buf.put_u8(value);
    }
    Ok(())
}

/// Decodes a bounded seven-payload-bits-per-byte continuation bitset.
pub fn decode_bitset<B: Buf, const N: usize>(
    buf: &mut B,
    bits: usize,
) -> Result<[u64; N], DecodeError> {
    if bits == 0 || bits > N.saturating_mul(64) {
        return Err(DecodeError::InvalidBitset {
            bits,
            reason: "width exceeds word storage",
        });
    }
    let mut words = [0u64; N];
    let mut offset = 0usize;
    loop {
        if !buf.has_remaining() {
            return Err(DecodeError::UnexpectedEof {
                needed: 1,
                available: 0,
            });
        }
        let value = buf.get_u8();
        let width = (bits - offset).min(7);
        if width < 7 && usize::from(value & 0x7f) >= (1usize << width) {
            return Err(DecodeError::InvalidBitset {
                bits,
                reason: "payload has bits outside the declared width",
            });
        }
        for bit in 0..width {
            if value & (1 << bit) != 0 {
                let index = offset + bit;
                words[index / 64] |= 1 << (index % 64);
            }
        }
        if value & 0x80 == 0 {
            return Ok(words);
        }
        if offset + 7 >= bits {
            return Err(DecodeError::InvalidBitset {
                bits,
                reason: "continuation exceeds the declared width",
            });
        }
        offset += 7;
    }
}

pub fn bitset_encoded_size<const N: usize>(words: &[u64; N], bits: usize) -> usize {
    (0..bits)
        .rev()
        .find(|index| words[index / 64] & (1 << (index % 64)) != 0)
        .map_or(1, |last| last / 7 + 1)
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

impl Nbt {
    /// Decodes the fixed-width little-endian NBT variant used inside item
    /// stack extra data. Most other Bedrock network NBT uses variable-width
    /// network little endian and should continue to use [`BedrockCodec`].
    pub fn decode_little_endian<B: Buf>(buf: &mut B) -> Result<Self, DecodeError> {
        decode_nbt(buf, NbtEncoding::LittleEndian)
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
        decode_nbt(buf, NbtEncoding::NetworkLittleEndian)
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

#[derive(Debug, Clone, Copy)]
enum NbtEncoding {
    NetworkLittleEndian,
    LittleEndian,
}

const MAX_NBT_DEPTH: usize = 512;

fn decode_nbt<B: Buf>(buf: &mut B, encoding: NbtEncoding) -> Result<Nbt, DecodeError> {
    let mut cursor = Cursor::new(buf.chunk());
    let root_tag = read_u8(&mut cursor)?;

    // Root tags are named in both Bedrock little-endian variants. Network
    // little endian uses a VarUInt length; fixed little endian uses an i16.
    skip_string(&mut cursor, encoding)?;
    scan_payload(root_tag, &mut cursor, encoding, 0)?;

    let len = cursor.position() as usize;
    Ok(Nbt(buf.copy_to_bytes(len)))
}

// --- NBT scanner logic ---

fn scan_compound(
    cursor: &mut Cursor<&[u8]>,
    encoding: NbtEncoding,
    depth: usize,
) -> Result<(), DecodeError> {
    // A Compound is just a list of tags terminated by End (0x00)
    loop {
        let tag_id = read_u8(cursor)?;
        if tag_id == 0 {
            // Tag_End
            break;
        }

        // Tags in a compound are named.
        // Read Name (Short Length + Bytes)
        skip_string(cursor, encoding)?;

        // Skip the payload based on ID
        scan_payload(tag_id, cursor, encoding, depth)?;
    }
    Ok(())
}

fn scan_payload(
    tag_id: u8,
    cursor: &mut Cursor<&[u8]>,
    encoding: NbtEncoding,
    depth: usize,
) -> Result<(), DecodeError> {
    match tag_id {
        1 => skip(cursor, 1), // Byte
        2 => skip(cursor, 2), // Short
        3 => skip_i32(cursor, encoding),
        4 => skip_i64(cursor, encoding),
        5 => skip(cursor, 4), // Float
        6 => skip(cursor, 8), // Double
        7 => {
            let len = nonnegative_len(read_i32(cursor, encoding)?)?;
            skip(cursor, len as usize)
        }
        8 => skip_string(cursor, encoding), // String
        9 => {
            let depth = enter_nbt_container(depth)?;
            let inner_id = read_u8(cursor)?;
            let count = nonnegative_len(read_i32(cursor, encoding)?)?;
            for _ in 0..count {
                scan_payload(inner_id, cursor, encoding, depth)?;
            }
            Ok(())
        }
        10 => scan_compound(cursor, encoding, enter_nbt_container(depth)?),
        11 => {
            let len = nonnegative_len(read_i32(cursor, encoding)?)?;
            for _ in 0..len {
                skip_i32(cursor, encoding)?;
            }
            Ok(())
        }
        12 => {
            let len = nonnegative_len(read_i32(cursor, encoding)?)?;
            for _ in 0..len {
                skip_i64(cursor, encoding)?;
            }
            Ok(())
        }
        _ => Err(DecodeError::UnknownNbtTag { tag_id }),
    }
}

fn enter_nbt_container(depth: usize) -> Result<usize, DecodeError> {
    if depth >= MAX_NBT_DEPTH {
        Err(DecodeError::NbtDepthExceeded { max: MAX_NBT_DEPTH })
    } else {
        Ok(depth + 1)
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

fn skip_string(cursor: &mut Cursor<&[u8]>, encoding: NbtEncoding) -> Result<(), DecodeError> {
    let len = match encoding {
        NbtEncoding::NetworkLittleEndian => crate::protocol::wire::read_var_u32(cursor)? as usize,
        NbtEncoding::LittleEndian => {
            if cursor.remaining() < 2 {
                return Err(DecodeError::UnexpectedEof {
                    needed: 2,
                    available: cursor.remaining(),
                });
            }
            nonnegative_len(cursor.get_i16_le() as i32)? as usize
        }
    };
    skip(cursor, len)
}

fn read_i32(cursor: &mut Cursor<&[u8]>, encoding: NbtEncoding) -> Result<i32, DecodeError> {
    match encoding {
        NbtEncoding::NetworkLittleEndian => Ok(crate::protocol::wire::read_zigzag32(cursor)?),
        NbtEncoding::LittleEndian => {
            if cursor.remaining() < 4 {
                return Err(DecodeError::UnexpectedEof {
                    needed: 4,
                    available: cursor.remaining(),
                });
            }
            Ok(cursor.get_i32_le())
        }
    }
}

fn skip_i32(cursor: &mut Cursor<&[u8]>, encoding: NbtEncoding) -> Result<(), DecodeError> {
    match encoding {
        NbtEncoding::NetworkLittleEndian => {
            crate::protocol::wire::read_zigzag32(cursor)?;
            Ok(())
        }
        NbtEncoding::LittleEndian => skip(cursor, 4),
    }
}

fn skip_i64(cursor: &mut Cursor<&[u8]>, encoding: NbtEncoding) -> Result<(), DecodeError> {
    match encoding {
        NbtEncoding::NetworkLittleEndian => {
            crate::protocol::wire::read_zigzag64(cursor)?;
            Ok(())
        }
        NbtEncoding::LittleEndian => skip(cursor, 8),
    }
}

fn nonnegative_len(value: i32) -> Result<i32, DecodeError> {
    if value < 0 {
        Err(DecodeError::NegativeLength {
            value: value as i64,
        })
    } else {
        Ok(value)
    }
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
    use std::sync::atomic::{AtomicUsize, Ordering};

    #[derive(Debug, Clone, Copy, PartialEq, Eq)]
    struct ConsumingZst;

    impl BedrockCodec for ConsumingZst {
        type Args = ();

        fn encode<B: BufMut>(&self, buf: &mut B) -> Result<(), std::io::Error> {
            buf.put_u8(0);
            Ok(())
        }

        fn decode<B: Buf>(buf: &mut B, _args: Self::Args) -> Result<Self, DecodeError> {
            u8::decode(buf, ())?;
            Ok(Self)
        }
    }

    #[derive(Debug, Clone, Copy, PartialEq, Eq)]
    struct ZeroProgress;

    static ZERO_PROGRESS_DECODE_CALLS: AtomicUsize = AtomicUsize::new(0);

    impl BedrockCodec for ZeroProgress {
        type Args = ();

        fn encode<B: BufMut>(&self, _buf: &mut B) -> Result<(), std::io::Error> {
            Ok(())
        }

        fn decode<B: Buf>(_buf: &mut B, _args: Self::Args) -> Result<Self, DecodeError> {
            ZERO_PROGRESS_DECODE_CALLS.fetch_add(1, Ordering::Relaxed);
            Ok(Self)
        }
    }

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

    #[test]
    fn checked_lengths_reject_negative_and_overflowing_values() {
        assert!(matches!(
            checked_signed_len(-1),
            Err(DecodeError::NegativeLength { value: -1 })
        ));
        if usize::BITS < 128 {
            let value = (usize::MAX as u128) + 1;
            assert!(matches!(
                checked_unsigned_len(value),
                Err(DecodeError::ArrayLengthExceeded {
                    declared: usize::MAX,
                    available: 0,
                })
            ));
        }
    }

    #[test]
    fn collection_minimum_rejects_impossible_and_overflowing_counts() {
        assert!(matches!(
            prepare_decode_vec::<u32>(3, 11, Some(4)),
            Err(DecodeError::ArrayLengthExceeded {
                declared: 3,
                available: 11,
            })
        ));
        assert!(matches!(
            prepare_decode_vec::<u32>(usize::MAX, usize::MAX, Some(2)),
            Err(DecodeError::ArrayLengthExceeded { .. })
        ));
    }

    #[test]
    fn unknown_and_zero_width_collections_do_not_eagerly_reserve() {
        let unknown = prepare_decode_vec::<u8>(1_000_000, 0, None).expect("unknown width");
        let zero = prepare_decode_vec::<()>(1_000_000, 0, Some(0)).expect("zero width");
        assert_eq!(unknown.capacity(), 0);
        assert!(zero.is_empty());
        assert_eq!(std::mem::size_of_val(zero.as_slice()), 0);
    }

    #[test]
    fn allocation_failures_map_to_decode_errors() {
        assert!(matches!(
            allocate_decode_bytes(usize::MAX),
            Err(DecodeError::Io(error)) if error.kind() == std::io::ErrorKind::OutOfMemory
        ));
    }

    #[test]
    fn strings_and_vectors_larger_than_four_kib_roundtrip() {
        assert_codec_roundtrip("x".repeat(8_192), ());
        assert_codec_roundtrip(vec![7u16; 4_097], ());
        assert_codec_roundtrip(vec![ConsumingZst; 4_097], ());
        assert_codec_roundtrip(vec![(); 100_000], ());
    }

    #[test]
    fn unit_vectors_use_the_explicit_zero_width_fast_path() {
        let len = u32::MAX as usize;
        let mut encoded = BytesMut::new();
        crate::protocol::wire::write_var_u32(&mut encoded, len as u32);
        let mut encoded = encoded.freeze();

        let decoded = Vec::<()>::decode(&mut encoded, ()).expect("unit vector");
        assert_eq!(decoded.len(), len);
        assert!(encoded.is_empty());
    }

    #[test]
    fn unregistered_zero_progress_codec_fails_after_one_decode() {
        ZERO_PROGRESS_DECODE_CALLS.store(0, Ordering::Relaxed);
        let mut encoded = BytesMut::new();
        crate::protocol::wire::write_var_u32(&mut encoded, 1_000_000_000);
        let mut encoded = encoded.freeze();

        assert!(matches!(
            Vec::<ZeroProgress>::decode(&mut encoded, ()),
            Err(DecodeError::ArrayLengthExceeded {
                declared: 1_000_000_000,
                available: 0,
            })
        ));
        assert_eq!(ZERO_PROGRESS_DECODE_CALLS.load(Ordering::Relaxed), 1);
    }

    #[test]
    fn fallible_lossy_decoding_matches_standard_replacement_behavior() {
        let bytes = vec![b'a', 0xff, b'b'];
        assert_eq!(
            try_decode_utf8_lossy_owned(bytes.clone()).expect("decode"),
            String::from_utf8_lossy(&bytes)
        );
        assert_eq!(decode_latin1_owned(vec![0x41, 0xff]).unwrap(), "A\u{ff}");
    }

    #[test]
    fn decoded_collections_do_not_eagerly_trust_wire_counts() {
        let mut values: Vec<u64> = prepare_decode_vec(1_000_000, 0, None).expect("lazy storage");
        assert_eq!(values.capacity(), 0);

        reserve_decode_item(&mut values).expect("reserve first decoded item");
        values.push(42);
        assert_eq!(values, [42]);
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

    #[test]
    fn unsigned_varints_preserve_the_full_wire_range() {
        let mut u32_buf = BytesMut::new();
        VarUInt(u32::MAX).encode(&mut u32_buf).unwrap();
        assert_eq!(&u32_buf[..], &[0xff, 0xff, 0xff, 0xff, 0x0f]);
        assert_eq!(VarUInt(u32::MAX).encoded_size(), 5);
        let mut u32_input = u32_buf.freeze();
        assert_eq!(
            VarUInt::decode(&mut u32_input, ()).unwrap(),
            VarUInt(u32::MAX)
        );
        assert!(!u32_input.has_remaining());

        let mut u64_buf = BytesMut::new();
        VarULong(u64::MAX).encode(&mut u64_buf).unwrap();
        assert_eq!(
            &u64_buf[..],
            &[0xff, 0xff, 0xff, 0xff, 0xff, 0xff, 0xff, 0xff, 0xff, 0x01]
        );
        assert_eq!(VarULong(u64::MAX).encoded_size(), 10);
        let mut u64_input = u64_buf.freeze();
        assert_eq!(
            VarULong::decode(&mut u64_input, ()).unwrap(),
            VarULong(u64::MAX)
        );
        assert!(!u64_input.has_remaining());
    }

    #[test]
    fn unsigned_varints_reject_payload_bits_beyond_their_width() {
        let mut u32_input = &[0xff, 0xff, 0xff, 0xff, 0x7f][..];
        assert!(matches!(
            VarUInt::decode(&mut u32_input, ()),
            Err(DecodeError::VarIntTooLarge)
        ));

        let mut u64_input = &[0xff, 0xff, 0xff, 0xff, 0xff, 0xff, 0xff, 0xff, 0xff, 0x7f][..];
        assert!(matches!(
            VarULong::decode(&mut u64_input, ()),
            Err(DecodeError::VarLongTooLarge)
        ));
    }

    #[test]
    fn unsigned_varints_reject_noncanonical_overlong_encodings() {
        let mut u32_input = &[0x80, 0x00][..];
        assert!(matches!(
            VarUInt::decode(&mut u32_input, ()),
            Err(DecodeError::VarIntTooLarge)
        ));

        let mut u64_input = &[0x80, 0x00][..];
        assert!(matches!(
            VarULong::decode(&mut u64_input, ()),
            Err(DecodeError::VarLongTooLarge)
        ));
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

    #[test]
    fn uuid_uses_bedrock_little_endian_halves() {
        let uuid = uuid::Uuid::parse_str("00112233-4455-6677-8899-aabbccddeeff").unwrap();
        let mut buf = BytesMut::new();
        uuid.encode(&mut buf).unwrap();

        assert_eq!(
            buf.as_ref(),
            &[
                0x77, 0x66, 0x55, 0x44, 0x33, 0x22, 0x11, 0x00, 0xff, 0xee, 0xdd, 0xcc, 0xbb, 0xaa,
                0x99, 0x88,
            ]
        );
        assert_eq!(uuid::Uuid::decode(&mut buf.freeze(), ()).unwrap(), uuid);
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

    #[test]
    fn continuation_bitset_round_trips_declared_boundary_bits() {
        let words = [1u64, 0, 1u64 << 2];
        let mut encoded = BytesMut::new();
        encode_bitset(&words, 131, &mut encoded).expect("encode 131-bit set");
        assert_eq!(encoded.len(), 19);
        assert_eq!(encoded[0], 0x81);
        assert!(encoded[1..18].iter().all(|byte| *byte == 0x80));
        assert_eq!(encoded[18], 0x10);
        assert_eq!(bitset_encoded_size(&words, 131), encoded.len());

        let mut input = encoded.freeze();
        let decoded = decode_bitset::<_, 3>(&mut input, 131).expect("decode 131-bit set");
        assert_eq!(decoded, words);
        assert!(!input.has_remaining());
    }

    #[test]
    fn continuation_bitset_zero_is_one_byte_and_rejects_excess_bits() {
        let words = [0u64; 3];
        let mut encoded = BytesMut::new();
        encode_bitset(&words, 131, &mut encoded).expect("encode empty bitset");
        assert_eq!(&encoded[..], &[0]);
        assert_eq!(bitset_encoded_size(&words, 131), 1);

        let mut malformed = &[0x80; 19][..];
        let error = decode_bitset::<_, 3>(&mut malformed, 131).unwrap_err();
        assert!(matches!(
            error,
            DecodeError::InvalidBitset { bits: 131, .. }
        ));
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

    fn nested_compounds(depth: usize, encoding: NbtEncoding) -> bytes::Bytes {
        assert!(depth > 0);

        let name_length = match encoding {
            NbtEncoding::NetworkLittleEndian => &[0x00][..],
            NbtEncoding::LittleEndian => &[0x00, 0x00][..],
        };
        let mut data = BytesMut::new();
        data.put_u8(10);
        data.put_slice(name_length);
        for _ in 1..depth {
            data.put_u8(10);
            data.put_slice(name_length);
        }
        for _ in 0..depth {
            data.put_u8(0);
        }
        data.freeze()
    }

    fn nested_lists(depth: usize, encoding: NbtEncoding) -> bytes::Bytes {
        assert!(depth > 0);

        let name_length = match encoding {
            NbtEncoding::NetworkLittleEndian => &[0x00][..],
            NbtEncoding::LittleEndian => &[0x00, 0x00][..],
        };
        let mut data = BytesMut::new();
        data.put_u8(9);
        data.put_slice(name_length);
        for _ in 1..depth {
            data.put_u8(9);
            match encoding {
                NbtEncoding::NetworkLittleEndian => {
                    crate::protocol::wire::write_zigzag32(&mut data, 1)
                }
                NbtEncoding::LittleEndian => data.put_i32_le(1),
            }
        }
        data.put_u8(1);
        match encoding {
            NbtEncoding::NetworkLittleEndian => crate::protocol::wire::write_zigzag32(&mut data, 0),
            NbtEncoding::LittleEndian => data.put_i32_le(0),
        }
        data.freeze()
    }

    #[test]
    fn network_nbt_allows_512_nested_containers_and_rejects_513() {
        let mut boundary = nested_compounds(512, NbtEncoding::NetworkLittleEndian);
        Nbt::decode(&mut boundary, ()).expect("512 nested containers should be accepted");
        assert!(!boundary.has_remaining());

        let mut over_limit = nested_compounds(513, NbtEncoding::NetworkLittleEndian);
        let err = Nbt::decode(&mut over_limit, ()).unwrap_err();
        assert!(matches!(err, DecodeError::NbtDepthExceeded { max: 512 }));
    }

    #[test]
    fn little_endian_nbt_allows_512_nested_containers_and_rejects_513() {
        let mut boundary = nested_compounds(512, NbtEncoding::LittleEndian);
        Nbt::decode_little_endian(&mut boundary).expect("512 nested containers should be accepted");
        assert!(!boundary.has_remaining());

        let mut over_limit = nested_compounds(513, NbtEncoding::LittleEndian);
        let err = Nbt::decode_little_endian(&mut over_limit).unwrap_err();
        assert!(matches!(err, DecodeError::NbtDepthExceeded { max: 512 }));
    }

    #[test]
    fn nbt_depth_limit_counts_lists() {
        let mut boundary = nested_lists(512, NbtEncoding::NetworkLittleEndian);
        Nbt::decode(&mut boundary, ()).expect("512 nested lists should be accepted");
        assert!(!boundary.has_remaining());

        let mut over_limit = nested_lists(513, NbtEncoding::NetworkLittleEndian);
        let err = Nbt::decode(&mut over_limit, ()).unwrap_err();
        assert!(matches!(err, DecodeError::NbtDepthExceeded { max: 512 }));
    }
}
