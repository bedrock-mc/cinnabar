use thiserror::Error;

/// Errors produced while decoding Bedrock chunk data.
#[derive(Debug, Clone, PartialEq, Eq, Error)]
pub enum DecodeError {
    #[error(
        "unexpected end of input while reading {context}: need {needed} bytes, have {remaining}"
    )]
    UnexpectedEof {
        context: &'static str,
        needed: usize,
        remaining: usize,
    },

    #[error("unsupported sub-chunk version {0}")]
    UnsupportedVersion(u8),

    #[error("sub-chunk has {count} storages, exceeding the client limit of {max}")]
    TooManyStorages { count: usize, max: usize },

    #[error("network sub-chunk contains a disk palette header {header:#04x}")]
    DiskPaletteInNetworkData { header: u8 },

    #[error("unsupported palette width {0} bits per index")]
    UnsupportedBitsPerIndex(u8),

    #[error("palette length {count} is invalid for this storage (maximum {max})")]
    InvalidPaletteLength { count: i32, max: usize },

    #[error("{context} VarInt does not terminate within five bytes")]
    VarIntTooLong { context: &'static str },

    #[error("{context} VarInt overflows 32 bits")]
    VarIntOverflow { context: &'static str },

    #[error(
        "block {block_index} references palette index {palette_index}, but the palette has {palette_len} entries"
    )]
    PaletteIndexOutOfBounds {
        block_index: usize,
        palette_index: usize,
        palette_len: usize,
    },

    #[error("standalone sub-chunk has {remaining} trailing bytes")]
    TrailingBytes { remaining: usize },

    #[error("sub-chunk Y index mismatch: expected {expected}, got {actual}")]
    SubChunkIndexMismatch { expected: i32, actual: i32 },

    #[error("level chunk has {count} sub-chunks, exceeding the client limit of {max}")]
    TooManySubChunks { count: usize, max: usize },

    #[error("sub-chunk Y index overflow for first index {first} and offset {offset}")]
    SubChunkYOverflow { first: i32, offset: usize },
}
