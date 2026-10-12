// Code generated from canonical protocol manifest v2. DO NOT EDIT.

//! Wire runtime for Rust codec contract v1 (`docs/rust-codec-contract.md`).
//!
//! Decoding is slice-based: every declared length is checked against the
//! remaining input and the reader's [`Limits`] before anything is allocated,
//! and collection storage grows fallibly. Encoding is fallible and appends to a
//! caller-owned buffer.

use std::fmt;

/// Largest compound/list nesting depth accepted by the NBT scanners.
pub const MAX_NBT_DEPTH: u32 = 512;

/// Default element ceiling when receiving from a client, matching the Go
/// reader's `maxSliceLength`.
pub const CLIENT_COLLECTION_LIMIT: usize = 4096;

/// Largest number of bytes reserved up front for one decoded collection; the
/// rest grows fallibly as elements decode.
pub const MAX_EAGER_RESERVE_BYTES: usize = 1 << 20;

/// Widest encoding of a `var_u32`.
pub const MAX_VAR_U32_LEN: usize = 5;

/// Identifies one length-prefixed field site; the generated `fields` module
/// names every site this crate decodes.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub struct FieldId(pub u16);

impl FieldId {
    /// A length read outside any named site, such as inside NBT.
    pub const NONE: Self = Self(u16::MAX);
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum DecodeError {
    UnexpectedEof {
        needed: usize,
        remaining: usize,
    },
    /// A varint used more bytes than its canonical encoding requires.
    VarIntOverlong,
    /// A varint did not terminate within the width of its target type.
    VarIntTooLarge,
    NegativeLength(i64),
    /// A declared length exceeded the limit configured for its field.
    LengthLimitExceeded {
        field: FieldId,
        limit: usize,
        actual: u64,
    },
    /// A declared length could not be covered by the remaining bytes.
    LengthNotRepresentable {
        field: FieldId,
        needed: usize,
        remaining: usize,
    },
    /// The allocator refused storage for a decoded value.
    AllocationFailed {
        requested: usize,
    },
    SchemaConstraint(&'static str),
    UnknownVariant {
        type_name: &'static str,
        value: i64,
    },
    InvalidBitset(&'static str),
    NbtDepthExceeded,
    NbtMalformed(&'static str),
    UnknownPacketId(u32),
    /// A known packet id that the sending peer is not permitted to use.
    UnexpectedDirection(u32),
    /// Bytes remained inside a region that declared its own length.
    TrailingBytes(usize),
    InvalidBatchHeader(Option<u8>),
    TooManyFrames {
        limit: usize,
    },
}

impl fmt::Display for DecodeError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::UnexpectedEof { needed, remaining } => {
                write!(
                    f,
                    "unexpected end of input: needed {needed}, {remaining} remaining"
                )
            }
            Self::VarIntOverlong => write!(f, "varint is not canonically encoded"),
            Self::VarIntTooLarge => write!(f, "varint exceeds the width of its target type"),
            Self::NegativeLength(value) => write!(f, "negative length {value}"),
            Self::LengthLimitExceeded {
                field,
                limit,
                actual,
            } => {
                write!(
                    f,
                    "field {} declared length {actual} exceeds limit {limit}",
                    field.0
                )
            }
            Self::LengthNotRepresentable {
                field,
                needed,
                remaining,
            } => {
                write!(
                    f,
                    "field {} declared length needs {needed} bytes, {remaining} remaining",
                    field.0
                )
            }
            Self::AllocationFailed { requested } => {
                write!(
                    f,
                    "could not allocate {requested} bytes for a decoded value"
                )
            }
            Self::SchemaConstraint(reason) => write!(f, "schema constraint violated: {reason}"),
            Self::UnknownVariant { type_name, value } => {
                write!(f, "unknown {type_name} discriminant {value}")
            }
            Self::InvalidBitset(reason) => write!(f, "invalid bitset: {reason}"),
            Self::NbtDepthExceeded => write!(f, "NBT nesting exceeds {MAX_NBT_DEPTH}"),
            Self::NbtMalformed(reason) => write!(f, "malformed NBT: {reason}"),
            Self::UnknownPacketId(id) => write!(f, "unknown packet id {id}"),
            Self::UnexpectedDirection(id) => {
                write!(f, "packet id {id} cannot be sent by this peer")
            }
            Self::TrailingBytes(count) => write!(f, "{count} unread bytes in declared region"),
            Self::InvalidBatchHeader(actual) => {
                write!(f, "batch does not start with 0xfe: {actual:?}")
            }
            Self::TooManyFrames { limit } => write!(f, "batch holds more than {limit} packets"),
        }
    }
}

impl std::error::Error for DecodeError {}

pub type DecodeResult<T> = Result<T, DecodeError>;

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum EncodeError {
    /// The output would exceed the writer's configured limit.
    OutputLimitExceeded {
        limit: usize,
        needed: usize,
    },
    /// The allocator refused more output space.
    AllocationFailed {
        requested: usize,
    },
    /// A length does not fit its wire prefix.
    LengthOverflow(usize),
    SchemaConstraint(&'static str),
    /// The local peer may not send this packet id.
    UnexpectedDirection(u32),
    InvalidSubclient {
        sender: u8,
        target: u8,
    },
}

impl fmt::Display for EncodeError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::OutputLimitExceeded { limit, needed } => {
                write!(f, "encoding needs {needed} bytes, limit is {limit}")
            }
            Self::AllocationFailed { requested } => {
                write!(f, "could not allocate {requested} output bytes")
            }
            Self::LengthOverflow(len) => write!(f, "length {len} does not fit its wire prefix"),
            Self::SchemaConstraint(reason) => write!(f, "schema constraint violated: {reason}"),
            Self::UnexpectedDirection(id) => {
                write!(f, "packet id {id} cannot be sent by this peer")
            }
            Self::InvalidSubclient { sender, target } => {
                write!(
                    f,
                    "subclient ids must be 0..=3, got sender {sender} and target {target}"
                )
            }
        }
    }
}

impl std::error::Error for EncodeError {}

pub type EncodeResult<T> = Result<T, EncodeError>;

/// Decode bounds: a default element ceiling, a default byte ceiling for
/// strings and byte buffers, and per-field overrides of either.
///
/// Every declared length is also bounded by the input remaining, so the
/// defaults only need to be finite when a peer is untrusted.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Limits {
    elements: usize,
    bytes: usize,
    fields: Vec<Option<usize>>,
}

static UNBOUNDED: Limits = Limits::unbounded();

impl Limits {
    /// No ceiling beyond the remaining input.
    pub const fn unbounded() -> Self {
        Self {
            elements: usize::MAX,
            bytes: usize::MAX,
            fields: Vec::new(),
        }
    }

    /// The defaults for decoding packets sent by `sender`: unbounded from a
    /// server, and [`CLIENT_COLLECTION_LIMIT`] elements from a client.
    pub fn receiving_from(sender: Peer) -> Self {
        match sender {
            Peer::Server => Self::unbounded(),
            Peer::Client => Self {
                elements: CLIENT_COLLECTION_LIMIT,
                ..Self::unbounded()
            },
        }
    }

    pub fn with_elements(mut self, limit: usize) -> Self {
        self.elements = limit;
        self
    }

    pub fn with_bytes(mut self, limit: usize) -> Self {
        self.bytes = limit;
        self
    }

    /// Bounds one field site: an element count for collections and maps, a
    /// byte length for strings and byte buffers.
    pub fn with_field(mut self, field: FieldId, limit: usize) -> Self {
        self.set_field(field, limit);
        self
    }

    pub fn set_field(&mut self, field: FieldId, limit: usize) {
        if field == FieldId::NONE {
            return;
        }
        let index = usize::from(field.0);
        if self.fields.len() <= index {
            self.fields.resize(index + 1, None);
        }
        self.fields[index] = Some(limit);
    }

    pub fn field(&self, field: FieldId) -> Option<usize> {
        self.fields.get(usize::from(field.0)).copied().flatten()
    }

    fn elements_for(&self, field: FieldId) -> usize {
        self.field(field).unwrap_or(self.elements)
    }

    fn bytes_for(&self, field: FieldId) -> usize {
        self.field(field).unwrap_or(self.bytes)
    }
}

impl Default for Limits {
    fn default() -> Self {
        Self::unbounded()
    }
}

/// Cursor over a byte slice. Every read is bounds-checked; no read allocates
/// beyond the bytes it returns.
///
/// Constructing with [`Reader::from_shared`] lets strings, byte buffers and
/// NBT decode as refcounted slices of the source instead of copies.
#[derive(Clone, Debug)]
pub struct Reader<'a> {
    buf: &'a [u8],
    pos: usize,
    shared: Option<&'a bytes::Bytes>,
    limits: &'a Limits,
}

impl<'a> Reader<'a> {
    pub fn new(buf: &'a [u8]) -> Self {
        Self {
            buf,
            pos: 0,
            shared: None,
            limits: &UNBOUNDED,
        }
    }

    /// Reads from `source` and shares its allocation with decoded byte fields.
    pub fn from_shared(source: &'a bytes::Bytes) -> Self {
        Self {
            shared: Some(source),
            ..Self::new(source)
        }
    }

    pub fn with_limits(mut self, limits: &'a Limits) -> Self {
        self.limits = limits;
        self
    }

    pub fn limits(&self) -> &'a Limits {
        self.limits
    }

    /// Refcounts `slice` against the source buffer when this reader shares one,
    /// and copies otherwise.
    ///
    /// Private because `Bytes::slice_ref` panics on a slice from a different
    /// allocation. Callers use `take_shared`, which can only pass its own.
    fn share(&self, slice: &[u8]) -> bytes::Bytes {
        match self.shared {
            Some(source) => source.slice_ref(slice),
            None => bytes::Bytes::copy_from_slice(slice),
        }
    }

    /// Reads `count` bytes without copying when this reader shares its source.
    pub fn take_shared(&mut self, count: usize) -> DecodeResult<bytes::Bytes> {
        let slice = self.take(count)?;
        Ok(self.share(slice))
    }

    pub fn remaining(&self) -> usize {
        self.buf.len() - self.pos
    }

    pub fn position(&self) -> usize {
        self.pos
    }

    pub fn is_empty(&self) -> bool {
        self.remaining() == 0
    }

    /// Errors unless the input is fully consumed.
    pub fn expect_consumed(&self) -> DecodeResult<()> {
        match self.remaining() {
            0 => Ok(()),
            count => Err(DecodeError::TrailingBytes(count)),
        }
    }

    pub fn take(&mut self, count: usize) -> DecodeResult<&'a [u8]> {
        if count > self.remaining() {
            return Err(DecodeError::UnexpectedEof {
                needed: count,
                remaining: self.remaining(),
            });
        }
        let start = self.pos;
        self.pos += count;
        Ok(&self.buf[start..self.pos])
    }

    pub fn read_u8(&mut self) -> DecodeResult<u8> {
        Ok(self.take(1)?[0])
    }

    pub fn read_bytes<const N: usize>(&mut self) -> DecodeResult<[u8; N]> {
        let slice = self.take(N)?;
        let mut out = [0u8; N];
        out.copy_from_slice(slice);
        Ok(out)
    }

    /// Reads an unsigned LEB128 varint, rejecting both non-canonical encodings
    /// and encodings wider than `max_bits`.
    pub fn read_var_uint(&mut self, max_bits: u32) -> DecodeResult<u64> {
        let mut value = 0u64;
        let mut shift = 0u32;
        loop {
            let byte = self.read_u8()?;
            let payload = u64::from(byte & 0x7f);
            let remaining_bits = max_bits - shift;
            if remaining_bits < 7 && payload >= (1u64 << remaining_bits) {
                return Err(DecodeError::VarIntTooLarge);
            }
            value |= payload << shift;
            if byte & 0x80 == 0 {
                // A canonical varint never ends in a zero continuation group.
                if shift > 0 && byte == 0 {
                    return Err(DecodeError::VarIntOverlong);
                }
                return Ok(value);
            }
            shift += 7;
            if shift >= max_bits {
                return Err(DecodeError::VarIntTooLarge);
            }
        }
    }

    pub fn read_var_u32(&mut self) -> DecodeResult<u32> {
        Ok(self.read_var_uint(32)? as u32)
    }

    pub fn read_var_u64(&mut self) -> DecodeResult<u64> {
        self.read_var_uint(64)
    }

    pub fn read_zigzag_i32(&mut self) -> DecodeResult<i32> {
        let raw = self.read_var_uint(32)? as u32;
        Ok(((raw >> 1) as i32) ^ -((raw & 1) as i32))
    }

    pub fn read_zigzag_i64(&mut self) -> DecodeResult<i64> {
        let raw = self.read_var_uint(64)?;
        Ok(((raw >> 1) as i64) ^ -((raw & 1) as i64))
    }

    /// Default byte ceiling; the NBT scanner bounds its inner lengths by it.
    pub fn byte_buffer_limit(&self) -> usize {
        self.limits.bytes
    }

    /// Checks a declared count against `limit` and the remaining input, where
    /// `min_element_size` is the fewest bytes one element can occupy.
    pub fn checked_count_with(
        &self,
        declared: u64,
        min_element_size: usize,
        limit: usize,
    ) -> DecodeResult<usize> {
        self.checked(FieldId::NONE, declared, min_element_size, limit)
    }

    fn checked(
        &self,
        field: FieldId,
        declared: u64,
        min_element_size: usize,
        limit: usize,
    ) -> DecodeResult<usize> {
        self.checked_bounds(field, declared, min_element_size, 0, u64::MAX, limit)
    }

    /// Checks truncation first, then the schema bounds, then `limit`, so a
    /// short input is never reported as a recoverable bound violation.
    fn checked_bounds(
        &self,
        field: FieldId,
        declared: u64,
        min_element_size: usize,
        minimum: u64,
        maximum: u64,
        limit: usize,
    ) -> DecodeResult<usize> {
        let remaining = self.remaining();
        let needed = declared.saturating_mul(min_element_size.max(1) as u64);
        if needed > remaining as u64 {
            return Err(DecodeError::LengthNotRepresentable {
                field,
                needed: usize::try_from(needed).unwrap_or(usize::MAX),
                remaining,
            });
        }
        validate_length(declared, minimum, maximum)?;
        if declared > limit as u64 {
            return Err(DecodeError::LengthLimitExceeded {
                field,
                limit,
                actual: declared,
            });
        }
        Ok(declared as usize)
    }

    /// Validates the declared element count of `field` before allocation.
    pub fn checked_count(
        &self,
        field: FieldId,
        declared: u64,
        min_element_size: usize,
    ) -> DecodeResult<usize> {
        self.checked(
            field,
            declared,
            min_element_size,
            self.limits.elements_for(field),
        )
    }

    /// Applies schema-published count bounds in addition to the field limit.
    pub fn checked_count_limits(
        &self,
        field: FieldId,
        declared: u64,
        min_element_size: usize,
        minimum: u64,
        maximum: u64,
    ) -> DecodeResult<usize> {
        self.checked_bounds(
            field,
            declared,
            min_element_size,
            minimum,
            maximum,
            self.limits.elements_for(field),
        )
    }

    fn checked_byte_len(&self, field: FieldId, declared: u64) -> DecodeResult<usize> {
        self.checked(field, declared, 1, self.limits.bytes_for(field))
    }

    fn checked_byte_bounds(
        &self,
        field: FieldId,
        declared: u64,
        minimum: u64,
        maximum: u64,
    ) -> DecodeResult<usize> {
        self.checked_bounds(
            field,
            declared,
            1,
            minimum,
            maximum,
            self.limits.bytes_for(field),
        )
    }

    /// Reads the `var_u32`-prefixed string at `field`, keeping its exact bytes.
    pub fn read_str(&mut self, field: FieldId) -> DecodeResult<Str> {
        let declared = u64::from(self.read_var_u32()?);
        let count = self.checked_byte_len(field, declared)?;
        Ok(Str(self.take_shared(count)?))
    }

    /// Reads the `var_u32`-prefixed byte buffer at `field`.
    pub fn read_shared_bytes(&mut self, field: FieldId) -> DecodeResult<bytes::Bytes> {
        let declared = u64::from(self.read_var_u32()?);
        let count = self.checked_byte_len(field, declared)?;
        self.take_shared(count)
    }

    /// Borrows the `var_u32`-prefixed byte buffer at `field` from the input.
    pub fn read_byte_slice(&mut self, field: FieldId) -> DecodeResult<&'a [u8]> {
        let declared = u64::from(self.read_var_u32()?);
        let count = self.checked_byte_len(field, declared)?;
        self.take(count)
    }
}

/// Creates storage for `count` decoded elements, reserving at most
/// [`MAX_EAGER_RESERVE_BYTES`] up front.
pub fn alloc_vec<T>(count: usize) -> DecodeResult<Vec<T>> {
    let size = std::mem::size_of::<T>().max(1);
    let eager = count.min(MAX_EAGER_RESERVE_BYTES / size);
    let mut out = Vec::new();
    out.try_reserve_exact(eager)
        .map_err(|_| DecodeError::AllocationFailed {
            requested: eager.saturating_mul(size),
        })?;
    Ok(out)
}

/// Appends a decoded element, growing the storage fallibly.
#[inline]
pub fn push<T>(out: &mut Vec<T>, value: T) -> DecodeResult<()> {
    if out.len() == out.capacity() {
        grow(out)?;
    }
    out.push(value);
    Ok(())
}

#[cold]
#[inline(never)]
fn grow<T>(out: &mut Vec<T>) -> DecodeResult<()> {
    out.try_reserve(1)
        .map_err(|_| DecodeError::AllocationFailed {
            requested: out
                .len()
                .saturating_add(1)
                .saturating_mul(std::mem::size_of::<T>()),
        })
}

/// Appends encoded bytes to a caller-owned buffer. Every write is fallible:
/// it fails at the configured output limit or when the allocator refuses.
#[derive(Debug)]
pub struct Writer<'a> {
    buf: &'a mut Vec<u8>,
    start: usize,
    limit: usize,
}

impl<'a> Writer<'a> {
    /// Appends to `buf` after its current contents, without an output limit.
    pub fn new(buf: &'a mut Vec<u8>) -> Self {
        Self::with_limit(buf, usize::MAX)
    }

    /// Appends to `buf`, failing once this writer would add more than `limit`
    /// bytes.
    pub fn with_limit(buf: &'a mut Vec<u8>, limit: usize) -> Self {
        let start = buf.len();
        Self { buf, start, limit }
    }

    /// Bytes this writer has appended.
    pub fn written(&self) -> usize {
        self.buf.len() - self.start
    }

    pub fn as_written(&self) -> &[u8] {
        &self.buf[self.start..]
    }

    /// Ensures `additional` more bytes fit the limit and are allocated.
    pub fn reserve(&mut self, additional: usize) -> EncodeResult<()> {
        let needed = self.written().saturating_add(additional);
        if needed > self.limit {
            return Err(EncodeError::OutputLimitExceeded {
                limit: self.limit,
                needed,
            });
        }
        self.buf
            .try_reserve(additional)
            .map_err(|_| EncodeError::AllocationFailed {
                requested: additional,
            })
    }

    pub fn write_u8(&mut self, value: u8) -> EncodeResult<()> {
        self.reserve(1)?;
        self.buf.push(value);
        Ok(())
    }

    pub fn write_all(&mut self, bytes: &[u8]) -> EncodeResult<()> {
        self.reserve(bytes.len())?;
        self.buf.extend_from_slice(bytes);
        Ok(())
    }

    pub fn write_var_uint(&mut self, value: u64) -> EncodeResult<()> {
        let mut scratch = [0u8; 10];
        let len = put_var_uint(&mut scratch, value);
        self.write_all(&scratch[..len])
    }

    pub fn write_var_u32(&mut self, value: u32) -> EncodeResult<()> {
        self.write_var_uint(u64::from(value))
    }

    pub fn write_var_u64(&mut self, value: u64) -> EncodeResult<()> {
        self.write_var_uint(value)
    }

    pub fn write_zigzag_i32(&mut self, value: i32) -> EncodeResult<()> {
        self.write_var_uint(u64::from(((value << 1) ^ (value >> 31)) as u32))
    }

    pub fn write_zigzag_i64(&mut self, value: i64) -> EncodeResult<()> {
        self.write_var_uint(((value << 1) ^ (value >> 63)) as u64)
    }

    /// Writes a length as a `var_u32`, rejecting lengths above `u32::MAX`.
    pub fn write_len_var_u32(&mut self, len: usize) -> EncodeResult<()> {
        let value = u32::try_from(len).map_err(|_| EncodeError::LengthOverflow(len))?;
        self.write_var_u32(value)
    }

    /// Writes a length as a little-endian `u32`, rejecting lengths above `u32::MAX`.
    pub fn write_len_u32le(&mut self, len: usize) -> EncodeResult<()> {
        let value = u32::try_from(len).map_err(|_| EncodeError::LengthOverflow(len))?;
        self.write_all(&value.to_le_bytes())
    }

    pub fn write_str(&mut self, value: &[u8]) -> EncodeResult<()> {
        self.write_len_var_u32(value.len())?;
        self.write_all(value)
    }

    pub fn write_byte_slice(&mut self, value: &[u8]) -> EncodeResult<()> {
        self.write_len_var_u32(value.len())?;
        self.write_all(value)
    }
}

/// Writes `value` as LEB128 into `out` and returns the byte count.
fn put_var_uint(out: &mut [u8], mut value: u64) -> usize {
    let mut len = 0;
    while value >= 0x80 {
        out[len] = (value as u8) | 0x80;
        value >>= 7;
        len += 1;
    }
    out[len] = value as u8;
    len + 1
}

pub trait Encode {
    fn encode(&self, writer: &mut Writer<'_>) -> EncodeResult<()>;

    fn encode_to_vec(&self) -> EncodeResult<Vec<u8>> {
        let mut out = Vec::new();
        encode_into(self, &mut out)?;
        Ok(out)
    }
}

/// Appends `value` to `out`; on error `out` is restored to its prior length.
pub fn encode_into<T: Encode + ?Sized>(value: &T, out: &mut Vec<u8>) -> EncodeResult<()> {
    let start = out.len();
    let result = value.encode(&mut Writer::new(out));
    if result.is_err() {
        out.truncate(start);
    }
    result
}

pub trait Decode: Sized {
    fn decode(reader: &mut Reader<'_>) -> DecodeResult<Self>;

    /// Decodes from a complete slice and rejects trailing bytes.
    fn decode_exact(input: &[u8]) -> DecodeResult<Self> {
        let mut reader = Reader::new(input);
        let value = Self::decode(&mut reader)?;
        reader.expect_consumed()?;
        Ok(value)
    }

    /// Decodes all of `input` under `limits`, sharing its allocation with
    /// string, byte and NBT fields.
    fn decode_shared(input: &bytes::Bytes, limits: &Limits) -> DecodeResult<Self> {
        let mut reader = Reader::from_shared(input).with_limits(limits);
        let value = Self::decode(&mut reader)?;
        reader.expect_consumed()?;
        Ok(value)
    }
}

macro_rules! fixed_codec {
    ($name:ident, $inner:ty, $size:expr, $write:ident, $read:ident) => {
        #[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Hash, PartialOrd, Ord)]
        pub struct $name(pub $inner);

        impl Encode for $name {
            fn encode(&self, writer: &mut Writer<'_>) -> EncodeResult<()> {
                writer.write_all(&self.0.$write())
            }
        }

        impl Decode for $name {
            fn decode(reader: &mut Reader<'_>) -> DecodeResult<Self> {
                Ok(Self(<$inner>::$read(reader.read_bytes::<$size>()?)))
            }
        }
    };
}

macro_rules! fixed_float_codec {
    ($name:ident, $inner:ty, $size:expr, $write:ident, $read:ident) => {
        #[derive(Clone, Copy, Debug, Default, PartialEq, PartialOrd)]
        pub struct $name(pub $inner);

        impl Encode for $name {
            fn encode(&self, writer: &mut Writer<'_>) -> EncodeResult<()> {
                writer.write_all(&self.0.$write())
            }
        }

        impl Decode for $name {
            fn decode(reader: &mut Reader<'_>) -> DecodeResult<Self> {
                Ok(Self(<$inner>::$read(reader.read_bytes::<$size>()?)))
            }
        }
    };
}

fixed_codec!(I8, i8, 1, to_le_bytes, from_le_bytes);
fixed_codec!(U8, u8, 1, to_le_bytes, from_le_bytes);
fixed_codec!(I16LE, i16, 2, to_le_bytes, from_le_bytes);
fixed_codec!(I16BE, i16, 2, to_be_bytes, from_be_bytes);
fixed_codec!(U16LE, u16, 2, to_le_bytes, from_le_bytes);
fixed_codec!(U16BE, u16, 2, to_be_bytes, from_be_bytes);
fixed_codec!(I32LE, i32, 4, to_le_bytes, from_le_bytes);
fixed_codec!(I32BE, i32, 4, to_be_bytes, from_be_bytes);
fixed_codec!(U32LE, u32, 4, to_le_bytes, from_le_bytes);
fixed_codec!(U32BE, u32, 4, to_be_bytes, from_be_bytes);
fixed_codec!(I64LE, i64, 8, to_le_bytes, from_le_bytes);
fixed_codec!(I64BE, i64, 8, to_be_bytes, from_be_bytes);
fixed_codec!(U64LE, u64, 8, to_le_bytes, from_le_bytes);
fixed_codec!(U64BE, u64, 8, to_be_bytes, from_be_bytes);
fixed_float_codec!(F32LE, f32, 4, to_le_bytes, from_le_bytes);
fixed_float_codec!(F32BE, f32, 4, to_be_bytes, from_be_bytes);
fixed_float_codec!(F64LE, f64, 8, to_le_bytes, from_le_bytes);
fixed_float_codec!(F64BE, f64, 8, to_be_bytes, from_be_bytes);

macro_rules! var_codec {
    ($name:ident, $inner:ty, $write:ident, $read:ident) => {
        #[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Hash, PartialOrd, Ord)]
        pub struct $name(pub $inner);

        impl Encode for $name {
            fn encode(&self, writer: &mut Writer<'_>) -> EncodeResult<()> {
                writer.$write(self.0)
            }
        }

        impl Decode for $name {
            fn decode(reader: &mut Reader<'_>) -> DecodeResult<Self> {
                reader.$read().map(Self)
            }
        }
    };
}

var_codec!(VarUInt, u32, write_var_u32, read_var_u32);
var_codec!(VarULong, u64, write_var_u64, read_var_u64);
var_codec!(ZigZag32, i32, write_zigzag_i32, read_zigzag_i32);
var_codec!(ZigZag64, i64, write_zigzag_i64, read_zigzag_i64);

/// Signed non-zigzag varint: the two's-complement value zero-extended to the
/// target width.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub struct VarInt(pub i32);

impl Encode for VarInt {
    fn encode(&self, writer: &mut Writer<'_>) -> EncodeResult<()> {
        writer.write_var_u32(self.0 as u32)
    }
}

impl Decode for VarInt {
    fn decode(reader: &mut Reader<'_>) -> DecodeResult<Self> {
        reader.read_var_u32().map(|value| Self(value as i32))
    }
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub struct VarLong(pub i64);

impl Encode for VarLong {
    fn encode(&self, writer: &mut Writer<'_>) -> EncodeResult<()> {
        writer.write_var_u64(self.0 as u64)
    }
}

impl Decode for VarLong {
    fn decode(reader: &mut Reader<'_>) -> DecodeResult<Self> {
        reader.read_var_u64().map(|value| Self(value as i64))
    }
}

impl Encode for bool {
    fn encode(&self, writer: &mut Writer<'_>) -> EncodeResult<()> {
        writer.write_u8(u8::from(*self))
    }
}

impl Decode for bool {
    fn decode(reader: &mut Reader<'_>) -> DecodeResult<Self> {
        Ok(reader.read_u8()? != 0)
    }
}

/// A length-prefixed protocol string, kept as the exact bytes received.
///
/// Bedrock strings are byte strings on the wire. UTF-8 is checked when the
/// text is read, so a malformed string never fails the surrounding packet and
/// always re-encodes byte-identically.
#[derive(Clone, Default, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub struct Str(bytes::Bytes);

impl Str {
    pub const fn from_static(value: &'static str) -> Self {
        Self(bytes::Bytes::from_static(value.as_bytes()))
    }

    pub fn as_bytes(&self) -> &[u8] {
        &self.0
    }

    pub fn len(&self) -> usize {
        self.0.len()
    }

    pub fn is_empty(&self) -> bool {
        self.0.is_empty()
    }

    pub fn to_str(&self) -> Result<&str, std::str::Utf8Error> {
        std::str::from_utf8(&self.0)
    }

    pub fn to_string_lossy(&self) -> std::borrow::Cow<'_, str> {
        String::from_utf8_lossy(&self.0)
    }

    pub fn into_bytes(self) -> bytes::Bytes {
        self.0
    }
}

impl fmt::Debug for Str {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        fmt::Debug::fmt(&self.to_string_lossy(), f)
    }
}

impl From<bytes::Bytes> for Str {
    fn from(value: bytes::Bytes) -> Self {
        Self(value)
    }
}

impl From<String> for Str {
    fn from(value: String) -> Self {
        Self(bytes::Bytes::from(value))
    }
}

impl From<&str> for Str {
    fn from(value: &str) -> Self {
        Self(bytes::Bytes::copy_from_slice(value.as_bytes()))
    }
}

impl AsRef<[u8]> for Str {
    fn as_ref(&self) -> &[u8] {
        &self.0
    }
}

impl PartialEq<str> for Str {
    fn eq(&self, other: &str) -> bool {
        self.as_bytes() == other.as_bytes()
    }
}

impl PartialEq<&str> for Str {
    fn eq(&self, other: &&str) -> bool {
        self.as_bytes() == other.as_bytes()
    }
}

impl Encode for Str {
    fn encode(&self, writer: &mut Writer<'_>) -> EncodeResult<()> {
        writer.write_str(self.as_bytes())
    }
}

impl Decode for Str {
    fn decode(reader: &mut Reader<'_>) -> DecodeResult<Self> {
        reader.read_str(FieldId::NONE)
    }
}

impl<T: Encode> Encode for Option<T> {
    fn encode(&self, writer: &mut Writer<'_>) -> EncodeResult<()> {
        match self {
            Some(value) => {
                writer.write_u8(1)?;
                value.encode(writer)
            }
            None => writer.write_u8(0),
        }
    }
}

impl<T: Decode> Decode for Option<T> {
    fn decode(reader: &mut Reader<'_>) -> DecodeResult<Self> {
        match reader.read_u8()? {
            0 => Ok(None),
            _ => T::decode(reader).map(Some),
        }
    }
}

impl<T: Encode, const N: usize> Encode for [T; N] {
    fn encode(&self, writer: &mut Writer<'_>) -> EncodeResult<()> {
        for value in self {
            value.encode(writer)?;
        }
        Ok(())
    }
}

impl<T: Decode + Default + Copy, const N: usize> Decode for [T; N] {
    fn decode(reader: &mut Reader<'_>) -> DecodeResult<Self> {
        let mut out = [T::default(); N];
        for slot in out.iter_mut() {
            *slot = T::decode(reader)?;
        }
        Ok(out)
    }
}

/// Writes a `VarUInt`-prefixed collection.
pub fn encode_collection<T: Encode>(writer: &mut Writer<'_>, values: &[T]) -> EncodeResult<()> {
    writer.write_len_var_u32(values.len())?;
    for value in values {
        value.encode(writer)?;
    }
    Ok(())
}

pub fn encode_collection_limits<T: Encode>(
    writer: &mut Writer<'_>,
    values: &[T],
    minimum: u64,
    maximum: u64,
) -> EncodeResult<()> {
    check_length(values.len(), minimum, maximum)?;
    encode_collection(writer, values)
}

fn decode_elements<T: Decode>(reader: &mut Reader<'_>, count: usize) -> DecodeResult<Vec<T>> {
    let mut out = alloc_vec(count)?;
    for _ in 0..count {
        push(&mut out, T::decode(reader)?)?;
    }
    Ok(out)
}

/// Reads the `VarUInt`-prefixed collection at `field`, bounding the declared
/// count before reserving.
pub fn decode_collection<T: Decode>(
    reader: &mut Reader<'_>,
    field: FieldId,
    min_element_size: usize,
) -> DecodeResult<Vec<T>> {
    let declared = u64::from(reader.read_var_u32()?);
    let count = reader.checked_count(field, declared, min_element_size)?;
    decode_elements(reader, count)
}

pub fn decode_collection_limits<T: Decode>(
    reader: &mut Reader<'_>,
    field: FieldId,
    min_element_size: usize,
    minimum: u64,
    maximum: u64,
) -> DecodeResult<Vec<T>> {
    let declared = u64::from(reader.read_var_u32()?);
    let count = reader.checked_count_limits(field, declared, min_element_size, minimum, maximum)?;
    decode_elements(reader, count)
}

/// Rejects an encoded length outside the schema bounds.
pub fn check_length(actual: usize, minimum: u64, maximum: u64) -> EncodeResult<()> {
    validate_length(actual as u64, minimum, maximum)
        .map_err(|_| EncodeError::SchemaConstraint("length outside schema limits"))
}

fn validate_length(actual: u64, minimum: u64, maximum: u64) -> DecodeResult<()> {
    if actual < minimum {
        return Err(DecodeError::SchemaConstraint("length below minimum"));
    }
    if actual > maximum {
        return Err(DecodeError::SchemaConstraint("length above maximum"));
    }
    Ok(())
}

pub fn encode_string_limits(
    writer: &mut Writer<'_>,
    value: &Str,
    minimum: u64,
    maximum: u64,
) -> EncodeResult<()> {
    check_length(value.len(), minimum, maximum)?;
    writer.write_str(value.as_bytes())
}

pub fn decode_string_limits(
    reader: &mut Reader<'_>,
    field: FieldId,
    minimum: u64,
    maximum: u64,
) -> DecodeResult<Str> {
    let declared = u64::from(reader.read_var_u32()?);
    let count = reader.checked_byte_bounds(field, declared, minimum, maximum)?;
    Ok(Str(reader.take_shared(count)?))
}

pub fn encode_bytes_limits(
    writer: &mut Writer<'_>,
    value: &[u8],
    minimum: u64,
    maximum: u64,
) -> EncodeResult<()> {
    check_length(value.len(), minimum, maximum)?;
    writer.write_byte_slice(value)
}

pub fn decode_bytes_limits(
    reader: &mut Reader<'_>,
    field: FieldId,
    minimum: u64,
    maximum: u64,
) -> DecodeResult<bytes::Bytes> {
    let declared = u64::from(reader.read_var_u32()?);
    let count = reader.checked_byte_bounds(field, declared, minimum, maximum)?;
    reader.take_shared(count)
}

/// Rejects an encoded number outside the schema bounds.
pub fn check_number_limits<T: PartialOrd + Copy>(
    value: T,
    minimum: Option<T>,
    maximum: Option<T>,
) -> EncodeResult<()> {
    validate_number_limits(value, minimum, maximum)
        .map_err(|_| EncodeError::SchemaConstraint("number outside schema limits"))
}

pub fn validate_number_limits<T: PartialOrd + Copy>(
    value: T,
    minimum: Option<T>,
    maximum: Option<T>,
) -> DecodeResult<()> {
    if minimum.is_some_and(|minimum| value < minimum) {
        return Err(DecodeError::SchemaConstraint("number below minimum"));
    }
    if maximum.is_some_and(|maximum| value > maximum) {
        return Err(DecodeError::SchemaConstraint("number above maximum"));
    }
    Ok(())
}

// ---------------------------------------------------------------------------
// Collections, maps, and bitsets
// ---------------------------------------------------------------------------

/// Writes a `u32le`-prefixed collection.
pub fn encode_collection_u32le<T: Encode>(
    writer: &mut Writer<'_>,
    values: &[T],
) -> EncodeResult<()> {
    writer.write_len_u32le(values.len())?;
    for value in values {
        value.encode(writer)?;
    }
    Ok(())
}

/// Reads the `u32le`-prefixed collection at `field` under the same bounds as
/// [`decode_collection`].
pub fn decode_collection_u32le<T: Decode>(
    reader: &mut Reader<'_>,
    field: FieldId,
    min_element_size: usize,
) -> DecodeResult<Vec<T>> {
    let declared = u64::from(u32::from_le_bytes(reader.read_bytes::<4>()?));
    let count = reader.checked_count(field, declared, min_element_size)?;
    decode_elements(reader, count)
}

/// Writes a `VarUInt`-prefixed map as ordered key/value pairs.
pub fn encode_map<K: Encode, V: Encode>(
    writer: &mut Writer<'_>,
    entries: &[(K, V)],
) -> EncodeResult<()> {
    writer.write_len_var_u32(entries.len())?;
    for (key, value) in entries {
        key.encode(writer)?;
        value.encode(writer)?;
    }
    Ok(())
}

pub fn encode_map_limits<K: Encode, V: Encode>(
    writer: &mut Writer<'_>,
    entries: &[(K, V)],
    minimum: u64,
    maximum: u64,
) -> EncodeResult<()> {
    check_length(entries.len(), minimum, maximum)?;
    encode_map(writer, entries)
}

fn decode_entries<K: Decode, V: Decode>(
    reader: &mut Reader<'_>,
    count: usize,
) -> DecodeResult<Vec<(K, V)>> {
    let mut out = alloc_vec(count)?;
    for _ in 0..count {
        let key = K::decode(reader)?;
        let value = V::decode(reader)?;
        push(&mut out, (key, value))?;
    }
    Ok(out)
}

/// Reads the `VarUInt`-prefixed map at `field`, preserving wire order.
pub fn decode_map<K: Decode, V: Decode>(
    reader: &mut Reader<'_>,
    field: FieldId,
    min_entry_size: usize,
) -> DecodeResult<Vec<(K, V)>> {
    let declared = u64::from(reader.read_var_u32()?);
    let count = reader.checked_count(field, declared, min_entry_size)?;
    decode_entries(reader, count)
}

pub fn decode_map_limits<K: Decode, V: Decode>(
    reader: &mut Reader<'_>,
    field: FieldId,
    min_entry_size: usize,
    minimum: u64,
    maximum: u64,
) -> DecodeResult<Vec<(K, V)>> {
    let declared = u64::from(reader.read_var_u32()?);
    let count = reader.checked_count_limits(field, declared, min_entry_size, minimum, maximum)?;
    decode_entries(reader, count)
}

/// Writes `bits` bits using seven payload bits per continuation byte.
///
/// Trailing zero groups are omitted, so an all-zero bitset is a single `0`
/// byte. Set bits at or beyond `bits` are not written, as the client drops
/// them on read.
pub fn encode_bitset(writer: &mut Writer<'_>, words: &[u64], bits: u64) -> EncodeResult<()> {
    let masked = |index: usize| match bits.saturating_sub(index as u64 * 64) {
        0 => 0,
        width if width >= 64 => words[index],
        width => words[index] & ((1u64 << width) - 1),
    };
    let Some(last) = (0..words.len()).rev().find_map(|index| {
        let word = masked(index);
        if word == 0 {
            None
        } else {
            Some(index as u64 * 64 + u64::from(63 - word.leading_zeros()))
        }
    }) else {
        return writer.write_u8(0);
    };
    let groups = last / 7 + 1;
    for group in 0..groups {
        let offset = group * 7;
        let mut value = 0u8;
        for bit in 0..7 {
            let index = offset + bit;
            if index <= last && words[(index / 64) as usize] & (1u64 << (index % 64)) != 0 {
                value |= 1 << bit;
            }
        }
        if group + 1 < groups {
            value |= 0x80;
        }
        writer.write_u8(value)?;
    }
    Ok(())
}

/// Reads a seven-bits-per-byte bitset, dropping payload bits at or beyond
/// `bits` as the client does. Only an encoding still unterminated after
/// `ceil(bits / 7)` bytes is an error.
pub fn decode_bitset<const N: usize>(reader: &mut Reader<'_>, bits: u64) -> DecodeResult<[u64; N]> {
    let mut words = [0u64; N];
    let mut offset = 0u64;
    for _ in 0..bits.div_ceil(7).max(1) {
        let value = reader.read_u8()?;
        for bit in 0..7 {
            let index = offset + bit;
            if index >= bits || value & (1 << bit) == 0 {
                continue;
            }
            if let Some(word) = words.get_mut((index / 64) as usize) {
                *word |= 1u64 << (index % 64);
            }
        }
        if value & 0x80 == 0 {
            return Ok(words);
        }
        offset += 7;
    }
    Err(DecodeError::InvalidBitset(
        "unterminated after its widest encoding",
    ))
}

// ---------------------------------------------------------------------------
// Length-prefixed byte buffers
// ---------------------------------------------------------------------------

impl Encode for bytes::Bytes {
    fn encode(&self, writer: &mut Writer<'_>) -> EncodeResult<()> {
        writer.write_byte_slice(self)
    }
}

impl Decode for bytes::Bytes {
    fn decode(reader: &mut Reader<'_>) -> DecodeResult<Self> {
        reader.read_shared_bytes(FieldId::NONE)
    }
}
// ---------------------------------------------------------------------------
// Packet frames
// ---------------------------------------------------------------------------

/// Leading byte of an uncompressed game batch.
pub const BATCH_HEADER: u8 = 0xfe;

/// Which peer may send a packet.
#[derive(Clone, Copy, Debug, Eq, Hash, PartialEq)]
pub enum Direction {
    Clientbound,
    Serverbound,
    Bidirectional,
}

impl Direction {
    /// Reports whether a packet with this direction may be sent by the peer.
    pub const fn permits(self, sender: Peer) -> bool {
        matches!(
            (self, sender),
            (Self::Bidirectional, _)
                | (Self::Clientbound, Peer::Server)
                | (Self::Serverbound, Peer::Client)
        )
    }
}

/// The peer that produced a packet.
#[derive(Clone, Copy, Debug, Eq, Hash, PartialEq)]
pub enum Peer {
    Client,
    Server,
}

/// The header varint of a batch entry: a ten-bit packet id and two-bit sender
/// and target subclient ids. Higher bits are ignored on decode.
#[derive(Clone, Copy, Debug, Default, Eq, Hash, PartialEq)]
pub struct PacketHeader {
    pub id: u32,
    pub sender_subclient: u8,
    pub target_subclient: u8,
}

impl PacketHeader {
    pub const ID_MASK: u32 = 0x3ff;

    pub const fn new(id: u32) -> Self {
        Self {
            id,
            sender_subclient: 0,
            target_subclient: 0,
        }
    }

    pub const fn from_raw(raw: u32) -> Self {
        Self {
            id: raw & Self::ID_MASK,
            sender_subclient: ((raw >> 10) & 0x3) as u8,
            target_subclient: ((raw >> 12) & 0x3) as u8,
        }
    }

    pub const fn to_raw(self) -> u32 {
        (self.id & Self::ID_MASK)
            | ((self.sender_subclient as u32 & 0x3) << 10)
            | ((self.target_subclient as u32 & 0x3) << 12)
    }

    fn validate(self) -> EncodeResult<()> {
        if self.id > Self::ID_MASK || self.sender_subclient > 3 || self.target_subclient > 3 {
            return Err(EncodeError::InvalidSubclient {
                sender: self.sender_subclient,
                target: self.target_subclient,
            });
        }
        Ok(())
    }
}

/// One `[length][header][body]` batch entry with its body left undecoded.
///
/// The entry's length prefix fixes its boundary, so a body that fails to
/// decode, or belongs to an unknown packet, never desynchronizes the batch.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct RawFrame {
    header: PacketHeader,
    frame: bytes::Bytes,
    body_start: usize,
}

impl RawFrame {
    pub fn header(&self) -> PacketHeader {
        self.header
    }

    pub fn id(&self) -> u32 {
        self.header.id
    }

    /// Every byte after the header varint, up to the declared length.
    pub fn body(&self) -> bytes::Bytes {
        self.frame.slice(self.body_start..)
    }

    /// The complete entry, including its length prefix, for forwarding as is.
    pub fn frame(&self) -> &bytes::Bytes {
        &self.frame
    }

    /// Copies the entry into its own allocation so that retaining it does not
    /// keep the rest of the batch alive.
    pub fn compact(self) -> Self {
        Self {
            frame: bytes::Bytes::copy_from_slice(&self.frame),
            ..self
        }
    }
}

/// Iterates the entries of an uncompressed batch payload without decoding
/// packet bodies. A framing error is yielded once and ends iteration.
#[derive(Clone, Debug)]
pub struct Frames {
    rest: bytes::Bytes,
    limit: usize,
    yielded: usize,
    done: bool,
}

impl Frames {
    /// Reads entries from a payload without the batch header byte, failing
    /// after `max_frames` entries.
    pub fn new(payload: bytes::Bytes, max_frames: usize) -> Self {
        Self {
            rest: payload,
            limit: max_frames,
            yielded: 0,
            done: false,
        }
    }

    /// Checks and strips the leading [`BATCH_HEADER`] byte.
    pub fn from_batch(mut batch: bytes::Bytes, max_frames: usize) -> DecodeResult<Self> {
        match batch.first().copied() {
            Some(BATCH_HEADER) => Ok(Self::new(batch.split_off(1), max_frames)),
            actual => Err(DecodeError::InvalidBatchHeader(actual)),
        }
    }

    fn next_frame(&mut self) -> DecodeResult<RawFrame> {
        let mut reader = Reader::new(&self.rest);
        let declared = reader.read_var_u32()? as usize;
        let prefix = reader.position();
        if declared > reader.remaining() {
            return Err(DecodeError::UnexpectedEof {
                needed: declared,
                remaining: reader.remaining(),
            });
        }
        let frame = self.rest.split_to(prefix + declared);
        let mut header = Reader::new(&frame[prefix..]);
        let raw = header.read_var_u32()?;
        let body_start = prefix + header.position();
        Ok(RawFrame {
            header: PacketHeader::from_raw(raw),
            frame,
            body_start,
        })
    }
}

impl Iterator for Frames {
    type Item = DecodeResult<RawFrame>;

    fn next(&mut self) -> Option<Self::Item> {
        if self.done || self.rest.is_empty() {
            return None;
        }
        if self.yielded == self.limit {
            self.done = true;
            return Some(Err(DecodeError::TooManyFrames { limit: self.limit }));
        }
        let result = self.next_frame();
        match result {
            Ok(_) => self.yielded += 1,
            Err(_) => self.done = true,
        }
        Some(result)
    }
}

/// Appends one `[length][header][body]` entry whose body `body` writes,
/// failing if header and body exceed `limit` bytes. On error `out` is
/// restored to its prior length.
pub fn encode_frame_with(
    out: &mut Vec<u8>,
    header: PacketHeader,
    limit: usize,
    body: impl FnOnce(&mut Writer<'_>) -> EncodeResult<()>,
) -> EncodeResult<()> {
    let start = out.len();
    let result = write_frame(out, header, limit, body);
    if result.is_err() {
        out.truncate(start);
    }
    result
}

fn write_frame(
    out: &mut Vec<u8>,
    header: PacketHeader,
    limit: usize,
    body: impl FnOnce(&mut Writer<'_>) -> EncodeResult<()>,
) -> EncodeResult<()> {
    header.validate()?;
    let start = out.len();
    {
        // The length is unknown until the body is written, so reserve the
        // widest prefix and close the gap afterwards.
        let mut writer = Writer::with_limit(out, limit.saturating_add(MAX_VAR_U32_LEN));
        writer.write_all(&[0; MAX_VAR_U32_LEN])?;
        writer.write_var_u32(header.to_raw())?;
        body(&mut writer)?;
    }
    let declared = out.len() - start - MAX_VAR_U32_LEN;
    let length = u32::try_from(declared).map_err(|_| EncodeError::LengthOverflow(declared))?;
    let mut prefix = [0u8; MAX_VAR_U32_LEN];
    let prefix_len = put_var_uint(&mut prefix, u64::from(length));
    out.copy_within(start + MAX_VAR_U32_LEN.., start + prefix_len);
    out.truncate(start + prefix_len + declared);
    out[start..start + prefix_len].copy_from_slice(&prefix[..prefix_len]);
    Ok(())
}

// ---------------------------------------------------------------------------
// NBT
// ---------------------------------------------------------------------------

/// Integer and length encoding used by an NBT payload.
///
/// Bedrock uses both forms and they are not interchangeable: the network form
/// varint-encodes integers and lengths, the persistent form stores them as
/// fixed-width little endian. A payload read with the wrong variant desyncs the
/// surrounding packet rather than failing locally.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum NbtVariant {
    Network,
    Persistent,
}

const TAG_END: u8 = 0;
const TAG_BYTE: u8 = 1;
const TAG_SHORT: u8 = 2;
const TAG_INT: u8 = 3;
const TAG_LONG: u8 = 4;
const TAG_FLOAT: u8 = 5;
const TAG_DOUBLE: u8 = 6;
const TAG_BYTE_ARRAY: u8 = 7;
const TAG_STRING: u8 = 8;
const TAG_LIST: u8 = 9;
const TAG_COMPOUND: u8 = 10;
const TAG_INT_ARRAY: u8 = 11;
const TAG_LONG_ARRAY: u8 = 12;

impl<'a> Reader<'a> {
    fn nbt_len(&mut self, variant: NbtVariant) -> DecodeResult<usize> {
        let declared = match variant {
            NbtVariant::Network => i64::from(self.read_zigzag_i32()?),
            NbtVariant::Persistent => i64::from(i32::from_le_bytes(self.read_bytes::<4>()?)),
        };
        if declared < 0 {
            return Err(DecodeError::NegativeLength(declared));
        }
        self.checked_count_with(declared as u64, 1, self.byte_buffer_limit())
    }

    fn nbt_name(&mut self, variant: NbtVariant) -> DecodeResult<()> {
        let count = match variant {
            NbtVariant::Network => {
                let declared = u64::from(self.read_var_u32()?);
                self.checked_count_with(declared, 1, self.byte_buffer_limit())?
            }
            NbtVariant::Persistent => usize::from(u16::from_le_bytes(self.read_bytes::<2>()?)),
        };
        self.take(count)?;
        Ok(())
    }

    fn scan_nbt_payload(&mut self, tag: u8, variant: NbtVariant, depth: u32) -> DecodeResult<()> {
        if depth > MAX_NBT_DEPTH {
            return Err(DecodeError::NbtDepthExceeded);
        }
        match tag {
            TAG_BYTE => {
                self.take(1)?;
            }
            TAG_SHORT => {
                self.take(2)?;
            }
            TAG_INT => match variant {
                NbtVariant::Network => {
                    self.read_zigzag_i32()?;
                }
                NbtVariant::Persistent => {
                    self.take(4)?;
                }
            },
            TAG_LONG => match variant {
                NbtVariant::Network => {
                    self.read_zigzag_i64()?;
                }
                NbtVariant::Persistent => {
                    self.take(8)?;
                }
            },
            TAG_FLOAT => {
                self.take(4)?;
            }
            TAG_DOUBLE => {
                self.take(8)?;
            }
            TAG_BYTE_ARRAY => {
                let count = self.nbt_len(variant)?;
                self.take(count)?;
            }
            TAG_STRING => self.nbt_name(variant)?,
            TAG_LIST => {
                let element = self.read_u8()?;
                let count = self.nbt_len(variant)?;
                if element == TAG_END && count != 0 {
                    return Err(DecodeError::NbtMalformed("non-empty list of TAG_End"));
                }
                for _ in 0..count {
                    self.scan_nbt_payload(element, variant, depth + 1)?;
                }
            }
            TAG_COMPOUND => loop {
                let entry = self.read_u8()?;
                if entry == TAG_END {
                    break;
                }
                self.nbt_name(variant)?;
                self.scan_nbt_payload(entry, variant, depth + 1)?;
            },
            TAG_INT_ARRAY => {
                let count = self.nbt_len(variant)?;
                for _ in 0..count {
                    match variant {
                        NbtVariant::Network => {
                            self.read_zigzag_i32()?;
                        }
                        NbtVariant::Persistent => {
                            self.take(4)?;
                        }
                    }
                }
            }
            TAG_LONG_ARRAY => {
                let count = self.nbt_len(variant)?;
                for _ in 0..count {
                    match variant {
                        NbtVariant::Network => {
                            self.read_zigzag_i64()?;
                        }
                        NbtVariant::Persistent => {
                            self.take(8)?;
                        }
                    }
                }
            }
            _ => return Err(DecodeError::NbtMalformed("unknown tag")),
        }
        Ok(())
    }

    /// Advances past exactly one root NBT value and borrows its bytes.
    pub fn read_nbt(&mut self, variant: NbtVariant) -> DecodeResult<&'a [u8]> {
        let start = self.pos;
        let tag = self.read_u8()?;
        if tag != TAG_END {
            self.nbt_name(variant)?;
            self.scan_nbt_payload(tag, variant, 0)?;
        }
        Ok(&self.buf[start..self.pos])
    }
}

macro_rules! nbt_codec {
    ($name:ident, $variant:expr, $doc:expr) => {
        #[doc = $doc]
        #[derive(Clone, Debug, PartialEq, Eq)]
        pub struct $name(pub bytes::Bytes);

        impl Default for $name {
            /// A lone TAG_End, which is the empty payload the scanner accepts.
            fn default() -> Self {
                Self(bytes::Bytes::from_static(&[0u8]))
            }
        }

        impl $name {
            pub const VARIANT: NbtVariant = $variant;
        }

        impl Encode for $name {
            fn encode(&self, writer: &mut Writer<'_>) -> EncodeResult<()> {
                writer.write_all(&self.0)
            }
        }

        impl Decode for $name {
            fn decode(reader: &mut Reader<'_>) -> DecodeResult<Self> {
                let payload = reader.read_nbt($variant)?;
                Ok(Self(reader.share(payload)))
            }
        }
    };
}

nbt_codec!(
    NetworkNbt,
    NbtVariant::Network,
    "NBT with varint integers and lengths."
);
nbt_codec!(
    PersistentNbt,
    NbtVariant::Persistent,
    "NBT with fixed-width little-endian integers and lengths."
);

#[cfg(test)]
mod nbt_tests {
    use super::*;

    /// The same bytes mean different things under the two variants, so the
    /// scanner must consume a different number of them.
    #[test]
    fn network_and_persistent_integers_differ() {
        // TAG_Int("", 1): network varint-encodes the value, persistent uses i32le.
        let network = [0x03u8, 0x00, 0x02];
        let mut reader = Reader::new(&network);
        assert_eq!(reader.read_nbt(NbtVariant::Network), Ok(&network[..]));

        let persistent = [0x03u8, 0x00, 0x00, 0x01, 0x00, 0x00, 0x00];
        let mut reader = Reader::new(&persistent);
        assert_eq!(reader.read_nbt(NbtVariant::Persistent), Ok(&persistent[..]));

        // Reading the persistent payload as network leaves bytes unconsumed.
        let mut reader = Reader::new(&persistent);
        reader.read_nbt(NbtVariant::Network).expect("scan");
        assert_ne!(reader.remaining(), 0);
    }

    #[test]
    fn empty_root_is_a_single_end_tag() {
        let mut reader = Reader::new(&[0x00]);
        assert_eq!(reader.read_nbt(NbtVariant::Network), Ok(&[0x00][..]));
        assert_eq!(reader.remaining(), 0);
    }

    #[test]
    fn nesting_is_bounded() {
        // Root compound with an empty name, then a chain of compounds nested
        // deeper than the cap. Each entry is TAG_Compound plus a zero-length
        // name; the scanner errors before reaching the closing TAG_Ends.
        let depth = MAX_NBT_DEPTH + 8;
        let mut deep = vec![0x0a, 0x00];
        for _ in 0..depth {
            deep.extend_from_slice(&[0x0a, 0x00]);
        }
        deep.extend(std::iter::repeat_n(0x00, depth as usize + 1));
        let mut reader = Reader::new(&deep);
        assert_eq!(
            reader.read_nbt(NbtVariant::Network),
            Err(DecodeError::NbtDepthExceeded)
        );
    }

    #[test]
    fn truncated_payloads_fail_rather_than_over_read() {
        let mut reader = Reader::new(&[0x03, 0x00]);
        assert!(reader.read_nbt(NbtVariant::Persistent).is_err());
    }
}
// ---------------------------------------------------------------------------
// UUID
// ---------------------------------------------------------------------------

impl Encode for uuid::Uuid {
    fn encode(&self, writer: &mut Writer<'_>) -> EncodeResult<()> {
        // Bedrock transmits a UUID as two little-endian 64-bit halves, not as
        // the raw byte order returned by `Uuid::as_bytes`.
        let (high, low) = self.as_u64_pair();
        writer.write_all(&high.to_le_bytes())?;
        writer.write_all(&low.to_le_bytes())
    }
}

impl Decode for uuid::Uuid {
    fn decode(reader: &mut Reader<'_>) -> DecodeResult<Self> {
        let high = u64::from_le_bytes(reader.read_bytes::<8>()?);
        let low = u64::from_le_bytes(reader.read_bytes::<8>()?);
        Ok(uuid::Uuid::from_u64_pair(high, low))
    }
}
// ---------------------------------------------------------------------------
// Vectors
// ---------------------------------------------------------------------------

impl Encode for glam::Vec2 {
    fn encode(&self, writer: &mut Writer<'_>) -> EncodeResult<()> {
        writer.write_all(&self.x.to_le_bytes())?;
        writer.write_all(&self.y.to_le_bytes())
    }
}

impl Decode for glam::Vec2 {
    fn decode(reader: &mut Reader<'_>) -> DecodeResult<Self> {
        let x = f32::from_le_bytes(reader.read_bytes::<4>()?);
        let y = f32::from_le_bytes(reader.read_bytes::<4>()?);
        Ok(glam::Vec2::new(x, y))
    }
}

impl Encode for glam::Vec3 {
    fn encode(&self, writer: &mut Writer<'_>) -> EncodeResult<()> {
        writer.write_all(&self.x.to_le_bytes())?;
        writer.write_all(&self.y.to_le_bytes())?;
        writer.write_all(&self.z.to_le_bytes())
    }
}

impl Decode for glam::Vec3 {
    fn decode(reader: &mut Reader<'_>) -> DecodeResult<Self> {
        let x = f32::from_le_bytes(reader.read_bytes::<4>()?);
        let y = f32::from_le_bytes(reader.read_bytes::<4>()?);
        let z = f32::from_le_bytes(reader.read_bytes::<4>()?);
        Ok(glam::Vec3::new(x, y, z))
    }
}
// ---------------------------------------------------------------------------
// Schema string patterns
// ---------------------------------------------------------------------------

use std::collections::HashMap;
use std::sync::{LazyLock, Mutex};

static SCHEMA_PATTERNS: LazyLock<Mutex<HashMap<&'static str, regex::Regex>>> =
    LazyLock::new(|| Mutex::new(HashMap::new()));

fn matches_pattern(value: &Str, pattern: &'static str) -> bool {
    let Ok(text) = value.to_str() else {
        return false;
    };
    let mut patterns = SCHEMA_PATTERNS
        .lock()
        .expect("schema pattern cache poisoned");
    let compiled = patterns.entry(pattern).or_insert_with(|| {
        regex::Regex::new(pattern).expect("manifest contains an invalid schema pattern")
    });
    compiled.is_match(text)
}

/// Rejects an encoded string that is not UTF-8 or does not match `pattern`.
pub fn check_pattern(value: &Str, pattern: &'static str) -> EncodeResult<()> {
    if matches_pattern(value, pattern) {
        Ok(())
    } else {
        Err(EncodeError::SchemaConstraint(
            "string does not match pattern",
        ))
    }
}

/// Rejects a decoded string that is not UTF-8 or does not match `pattern`.
pub fn validate_pattern(value: &Str, pattern: &'static str) -> DecodeResult<()> {
    if matches_pattern(value, pattern) {
        Ok(())
    } else {
        Err(DecodeError::SchemaConstraint(
            "string does not match pattern",
        ))
    }
}

#[cfg(test)]
mod pattern_tests {
    use super::*;

    #[test]
    fn patterns_are_enforced() {
        assert_eq!(
            validate_pattern(&Str::from_static("abc"), "^[a-z]+$"),
            Ok(())
        );
        assert_eq!(
            validate_pattern(&Str::from_static("123"), "^[a-z]+$"),
            Err(DecodeError::SchemaConstraint(
                "string does not match pattern"
            ))
        );
        assert!(check_pattern(&Str::from(bytes::Bytes::from_static(&[0xff])), "^.*$").is_err());
    }
}
// ---------------------------------------------------------------------------
// Runtime tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod runtime_tests {
    use super::*;

    fn frame(id: u32, body: &[u8]) -> Vec<u8> {
        let mut out = Vec::new();
        encode_frame_with(&mut out, PacketHeader::new(id), usize::MAX, |writer| {
            writer.write_all(body)
        })
        .expect("frame encodes");
        out
    }

    #[test]
    fn varint_rejects_overlong_encodings() {
        assert_eq!(
            Reader::new(&[0x80, 0x00]).read_var_u32(),
            Err(DecodeError::VarIntOverlong)
        );
        assert_eq!(Reader::new(&[0x00]).read_var_u32(), Ok(0));
    }

    #[test]
    fn varint_round_trips_boundary_values() {
        for value in [0u64, 1, 127, 128, u32::MAX as u64, u64::MAX] {
            let mut out = Vec::new();
            Writer::new(&mut out).write_var_u64(value).unwrap();
            assert_eq!(
                Reader::new(&out).read_var_u64(),
                Ok(value),
                "varint {value}"
            );
        }
    }

    /// A declared count the input cannot hold fails before any allocation.
    #[test]
    fn impossible_counts_fail_before_allocating() {
        let mut input = Vec::new();
        Writer::new(&mut input).write_var_u32(u32::MAX).unwrap();
        let result = decode_collection::<U64LE>(&mut Reader::new(&input), FieldId(0), 8);
        assert!(matches!(
            result,
            Err(DecodeError::LengthNotRepresentable { .. })
        ));
    }

    /// A truncated count reports truncation, not a schema or limit violation.
    #[test]
    fn truncation_is_reported_before_bounds() {
        let mut input = Vec::new();
        Writer::new(&mut input).write_var_u32(1000).unwrap();
        let limits = Limits::unbounded().with_field(FieldId(0), 4);
        let truncated = decode_collection_limits::<U8>(
            &mut Reader::new(&input).with_limits(&limits),
            FieldId(0),
            1,
            0,
            4,
        );
        assert!(matches!(
            truncated,
            Err(DecodeError::LengthNotRepresentable { .. })
        ));
        let text = decode_string_limits(
            &mut Reader::new(&input).with_limits(&limits),
            FieldId(0),
            0,
            4,
        );
        assert!(matches!(
            text,
            Err(DecodeError::LengthNotRepresentable { .. })
        ));

        input.resize(input.len() + 1000, 0);
        let schema = decode_collection_limits::<U8>(
            &mut Reader::new(&input).with_limits(&limits),
            FieldId(0),
            1,
            0,
            4,
        );
        assert_eq!(
            schema,
            Err(DecodeError::SchemaConstraint("length above maximum"))
        );
        let limited = decode_collection_limits::<U8>(
            &mut Reader::new(&input).with_limits(&limits),
            FieldId(0),
            1,
            0,
            2000,
        );
        assert!(matches!(
            limited,
            Err(DecodeError::LengthLimitExceeded { .. })
        ));
    }

    /// Bits at or beyond the declared width are dropped in both directions.
    #[test]
    fn bitsets_mask_bits_beyond_their_width() {
        for words in [[1u64 << 10, 0], [0, 1], [0, 1 << 63], [u64::MAX, u64::MAX]] {
            let mut out = Vec::new();
            encode_bitset(&mut Writer::new(&mut out), &words, 10).unwrap();
            let masked = words[0] & ((1 << 10) - 1);
            assert_eq!(decode_bitset::<1>(&mut Reader::new(&out), 10), Ok([masked]));
        }
        let mut out = Vec::new();
        encode_bitset(&mut Writer::new(&mut out), &[1 << 9 | 1, 0], 10).unwrap();
        assert_eq!(out, vec![0x81, 0x04]);

        // 0x7c sets bits 9..=13 of a 10-bit set; only bit 9 survives.
        assert_eq!(
            decode_bitset::<1>(&mut Reader::new(&[0x81, 0x7c]), 10),
            Ok([1 << 9 | 1])
        );
        assert!(decode_bitset::<1>(&mut Reader::new(&[0x80, 0x80, 0x00]), 10).is_err());
        let mut widest = vec![0xff; 18];
        widest.push(0x7f);
        assert_eq!(
            decode_bitset::<3>(&mut Reader::new(&widest), 131),
            Ok([u64::MAX, u64::MAX, 7])
        );
        widest[18] = 0xff;
        widest.push(0x00);
        assert_eq!(
            decode_bitset::<3>(&mut Reader::new(&widest), 131),
            Err(DecodeError::InvalidBitset(
                "unterminated after its widest encoding"
            ))
        );
    }

    #[test]
    fn field_limits_override_the_default() {
        let mut input = Vec::new();
        Writer::new(&mut input).write_str(b"hello").unwrap();
        let limits = Limits::unbounded().with_field(FieldId(3), 4);
        let mut reader = Reader::new(&input).with_limits(&limits);
        assert_eq!(
            reader.read_str(FieldId(3)),
            Err(DecodeError::LengthLimitExceeded {
                field: FieldId(3),
                limit: 4,
                actual: 5
            })
        );
        let mut reader = Reader::new(&input).with_limits(&limits);
        assert_eq!(reader.read_str(FieldId(4)).unwrap(), "hello");
    }

    #[test]
    fn client_defaults_bound_collections_and_server_defaults_do_not() {
        let mut input = Vec::new();
        Writer::new(&mut input).write_var_u32(5000).unwrap();
        input.resize(input.len() + 5000, 0);
        let client = Limits::receiving_from(Peer::Client);
        let result =
            decode_collection::<U8>(&mut Reader::new(&input).with_limits(&client), FieldId(0), 1);
        assert!(matches!(
            result,
            Err(DecodeError::LengthLimitExceeded { .. })
        ));
        let server = Limits::receiving_from(Peer::Server);
        let values =
            decode_collection::<U8>(&mut Reader::new(&input).with_limits(&server), FieldId(0), 1);
        assert_eq!(values.map(|values| values.len()), Ok(5000));
    }

    /// Strings keep their exact bytes, so non-UTF-8 text survives a round trip.
    #[test]
    fn strings_preserve_invalid_utf8() {
        let raw = bytes::Bytes::from_static(&[0x02, 0xff, 0xfe]);
        let value = Str::decode_shared(&raw, &Limits::unbounded()).unwrap();
        assert!(value.to_str().is_err());
        assert_eq!(value.encode_to_vec().unwrap(), raw.as_ref());
    }

    #[test]
    fn shared_strings_do_not_copy() {
        let raw = bytes::Bytes::from(vec![0x03, b'a', b'b', b'c']);
        let value = Str::decode_shared(&raw, &Limits::unbounded()).unwrap();
        assert_eq!(value.as_bytes().as_ptr(), raw[1..].as_ptr());
    }

    struct FailsMidway;

    impl Encode for FailsMidway {
        fn encode(&self, writer: &mut Writer<'_>) -> EncodeResult<()> {
            writer.write_all(&[1, 2])?;
            Err(EncodeError::SchemaConstraint("fails midway"))
        }
    }

    #[test]
    fn writer_limit_fails_and_encode_into_restores_the_buffer() {
        let mut out = vec![0xaa];
        let mut writer = Writer::with_limit(&mut out, 2);
        assert!(writer.write_all(&[1, 2, 3]).is_err());

        let mut out = vec![9];
        assert!(encode_into(&FailsMidway, &mut out).is_err());
        assert_eq!(out, vec![9]);
    }

    #[test]
    fn frames_keep_exact_boundaries() {
        let mut batch = vec![BATCH_HEADER];
        batch.extend(frame(1000, &[1, 2, 3]));
        batch.extend(frame(9, &[]));
        let frames: Vec<_> = Frames::from_batch(batch.into(), 8)
            .unwrap()
            .collect::<Result<_, _>>()
            .unwrap();
        assert_eq!(frames.len(), 2);
        assert_eq!(frames[0].id(), 1000);
        assert_eq!(frames[0].body().as_ref(), &[1, 2, 3]);
        assert_eq!(frames[1].id(), 9);
        assert!(frames[1].body().is_empty());
    }

    #[test]
    fn frame_encoding_moves_short_prefixes() {
        let body = vec![7u8; 300];
        let encoded = frame(1, &body);
        // 301 bytes of header and body need a two-byte prefix.
        assert_eq!(&encoded[..3], &[0xad, 0x02, 0x01]);
        assert_eq!(encoded.len(), 2 + 1 + 300);
    }

    #[test]
    fn truncated_frames_and_frame_floods_fail() {
        let truncated = bytes::Bytes::from_static(&[0x05, 0x01]);
        let mut frames = Frames::new(truncated, 8);
        assert!(frames.next().unwrap().is_err());
        assert!(frames.next().is_none());

        let mut flood = Vec::new();
        for _ in 0..3 {
            flood.extend(frame(1, &[]));
        }
        let results: Vec<_> = Frames::new(flood.into(), 2).collect();
        assert_eq!(results.len(), 3);
        assert_eq!(results[2], Err(DecodeError::TooManyFrames { limit: 2 }));
    }

    #[test]
    fn subclients_out_of_range_are_rejected() {
        let header = PacketHeader {
            id: 1,
            sender_subclient: 4,
            target_subclient: 0,
        };
        let mut out = vec![9];
        assert!(encode_frame_with(&mut out, header, usize::MAX, |_| Ok(())).is_err());
        assert_eq!(out, vec![9]);
    }
}
