use thiserror::Error;

/// Errors that can occur when decoding Bedrock protocol data.
#[derive(Debug, Error)]
pub enum DecodeError {
    #[error("unexpected end of buffer: needed {needed} bytes, had {available}")]
    UnexpectedEof { needed: usize, available: usize },

    #[error("negative length not allowed: got {value}")]
    NegativeLength { value: i64 },

    #[error("string length {declared} exceeds remaining buffer {available}")]
    StringLengthExceeded { declared: usize, available: usize },

    #[error("array length {declared} exceeds remaining buffer {available}")]
    ArrayLengthExceeded { declared: usize, available: usize },

    #[error("invalid enum value {value} for {enum_name}")]
    InvalidEnumValue { enum_name: &'static str, value: i64 },

    #[error("invalid packet id: {id}")]
    InvalidPacketId { id: u32 },

    #[error("expected magic byte 0x{expected:02x}, got 0x{actual:02x}")]
    InvalidMagicByte { expected: u8, actual: u8 },

    #[error("packet length {declared} exceeds available {available}")]
    PacketLengthExceeded { declared: usize, available: usize },

    #[error("varint too large")]
    VarIntTooLarge,

    #[error("varlong too large")]
    VarLongTooLarge,

    #[error("unknown NBT tag: {tag_id}")]
    UnknownNbtTag { tag_id: u8 },

    #[error("utf8 decode error: {0}")]
    Utf8Error(#[from] std::string::FromUtf8Error),

    #[error("io error: {0}")]
    Io(#[from] std::io::Error),
}

impl From<DecodeError> for std::io::Error {
    fn from(err: DecodeError) -> Self {
        use std::io::ErrorKind;
        match &err {
            DecodeError::UnexpectedEof { .. } => std::io::Error::new(ErrorKind::UnexpectedEof, err),
            _ => std::io::Error::new(ErrorKind::InvalidData, err),
        }
    }
}
