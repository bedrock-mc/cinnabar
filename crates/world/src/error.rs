use thiserror::Error;

use crate::{BlockEntityError, BlockEntityNbtError};

/// The process-wide collision revision identity space has been exhausted.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Error)]
pub enum CollisionRevisionError {
    #[error("collision revision identity space is exhausted")]
    Exhausted,
}

/// Errors produced while decoding Bedrock chunk data.
#[derive(Debug, Clone, PartialEq, Eq, Error)]
pub enum DecodeError {
    #[error(transparent)]
    CollisionRevision(#[from] CollisionRevisionError),

    #[error(transparent)]
    BlockEntity(#[from] BlockEntityError),

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

    #[error("biome storage {index} copies a previous storage, but none exists")]
    BiomeCopyWithoutPrevious { index: usize },

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

    #[error("biome column has {count} storages, exceeding the client limit of {max}")]
    TooManyBiomeStorages { count: usize, max: usize },

    #[error("sub-chunk Y index overflow for first index {first} and offset {offset}")]
    SubChunkYOverflow { first: i32, offset: usize },
}

impl DecodeError {
    /// Returns a stable malformed-wire reason, excluding bounded policy and
    /// semantically unusable but structurally complete data.
    #[must_use]
    pub const fn wire_error_reason(&self) -> Option<&'static str> {
        match self {
            Self::CollisionRevision(_)
            | Self::TooManyStorages { .. }
            | Self::TooManySubChunks { .. }
            | Self::TooManyBiomeStorages { .. }
            | Self::SubChunkIndexMismatch { .. }
            | Self::SubChunkYOverflow { .. } => None,
            Self::BlockEntity(error) => match error {
                BlockEntityError::MissingReservedEntryCount
                | BlockEntityError::TrailingBytes { .. }
                | BlockEntityError::Nbt(
                    BlockEntityNbtError::UnknownTag { .. }
                    | BlockEntityNbtError::UnexpectedEof { .. }
                    | BlockEntityNbtError::VarIntTooLong
                    | BlockEntityNbtError::VarIntOverflow
                    | BlockEntityNbtError::VarLongTooLong
                    | BlockEntityNbtError::VarLongOverflow
                    | BlockEntityNbtError::NegativeLength { .. }
                    | BlockEntityNbtError::InvalidUtf8
                    | BlockEntityNbtError::NonEmptyEndList,
                ) => Some("malformed block-entity wire"),
                _ => None,
            },
            Self::UnexpectedEof { .. } => Some("truncated chunk payload"),
            Self::UnsupportedVersion(_)
            | Self::DiskPaletteInNetworkData { .. }
            | Self::UnsupportedBitsPerIndex(_) => Some("invalid chunk encoding"),
            Self::BiomeCopyWithoutPrevious { .. }
            | Self::InvalidPaletteLength { .. }
            | Self::PaletteIndexOutOfBounds { .. } => Some("invalid chunk palette"),
            Self::VarIntTooLong { .. } | Self::VarIntOverflow { .. } => {
                Some("malformed chunk VarInt")
            }
            Self::TrailingBytes { .. } => Some("trailing chunk bytes"),
        }
    }
}

/// Errors produced before mutating packed block storage.
///
/// All updates in a batch are validated before the store is changed, so these
/// errors never leave a partially-applied sub-chunk behind.
#[derive(Debug, Clone, PartialEq, Eq, Error)]
pub enum MutationError {
    #[error(transparent)]
    CollisionRevision(#[from] CollisionRevisionError),

    #[error("local block coordinates ({x}, {y}, {z}) are outside a 16x16x16 sub-chunk")]
    LocalCoordinatesOutOfBounds { x: u8, y: u8, z: u8 },

    #[error("block storage layer {layer} exceeds the client limit of {max}")]
    LayerOutOfBounds { layer: u32, max: usize },
}
