use crate::ItemPacketError;
use thiserror::Error;

#[derive(Debug, Clone, PartialEq, Eq, Error)]
pub enum ActorPacketError {
    #[error("painting direction {0} is not horizontal")]
    InvalidPaintingDirection(i32),
    #[error(transparent)]
    Item(#[from] ItemPacketError),

    #[error("actor identifier has {bytes} UTF-8 bytes, exceeding {max}")]
    IdentifierTooLong { bytes: usize, max: usize },
    #[error("actor spawn contains a non-finite {field}")]
    NonFiniteSpawnField { field: &'static str },
    #[error("actor collection {collection} has {count} entries, exceeding {max}")]
    TooManyEntries {
        collection: &'static str,
        count: usize,
        max: usize,
    },
    #[error("actor text field {field} has {bytes} UTF-8 bytes, exceeding {max}")]
    TextTooLong {
        field: &'static str,
        bytes: usize,
        max: usize,
    },
    #[error("actor field {field} is non-finite")]
    NonFiniteField { field: &'static str },
    #[error("absolute actor move has an invalid runtime ID varuint")]
    InvalidAbsoluteMoveRuntimeId,
    #[error(
        "absolute actor move has {actual} body bytes after its runtime ID; expected {expected}"
    )]
    InvalidAbsoluteMoveLength { actual: usize, expected: usize },
    #[error("actor update has negative tick {0}")]
    NegativeTick(i64),
}
