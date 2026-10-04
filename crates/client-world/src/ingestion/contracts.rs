//! Prepared authoritative mutations shared with the synchronous chunk coordinator.

use std::{collections::BTreeSet, time::Duration};

use protocol::{LevelChunkEvent, WorldEvent};
use thiserror::Error;
use world::{
    BlockEntityError, BlockEntityKey, BlockEntityNbt, DecodedBiomeColumn, DecodedBlockEntities,
    DecodedLevelChunk, DecodedSubChunk, MutationError, PreparedSubChunkMutation, SubChunkKey,
};

#[derive(Debug)]
pub enum PreparedWorldEvent {
    InlineLevelChunk {
        event: LevelChunkEvent,
        decoded: DecodedLevelChunk,
        duration: Duration,
    },
    RequestLevelChunk {
        event: LevelChunkEvent,
        decoded: (DecodedBiomeColumn, DecodedBlockEntities),
        duration: Duration,
    },
    SubChunks {
        dimension: i32,
        entries: Vec<PreparedSubChunk>,
        duration: Duration,
    },
    BlockUpdates {
        result: Result<PreparedBlockMutations, MutationError>,
        duration: Duration,
    },
    BlockEntityUpdate {
        key: BlockEntityKey,
        decoded: Result<BlockEntityNbt, BlockEntityError>,
        duration: Duration,
    },
    Immediate(WorldEvent),
    CommitOnly,
    NormalizationFailure,
}

/// Packed replacements and their worker-computed light invalidation summary.
#[derive(Debug)]
pub struct PreparedBlockMutations {
    pub mutations: Vec<PreparedSubChunkMutation>,
    pub relight: BTreeSet<SubChunkKey>,
}

#[derive(Debug)]
pub struct PreparedSubChunk {
    pub position: [i32; 3],
    pub result: PreparedSubChunkResult,
    pub diagnostics: Option<protocol::SubChunkDiagnostic>,
}

#[derive(Debug)]
pub enum PreparedSubChunkResult {
    Decoded(DecodedSubChunk),
    AllAir,
    Unavailable(protocol::SubChunkUnavailable),
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Error)]
pub enum WorldStreamError {
    #[error("world sequence {sequence} is duplicate or older than next sequence {next}")]
    DuplicateOrPast { sequence: u64, next: u64 },
    #[error(
        "world admission is full at sequence {sequence} ({admitted}/{capacity} events, {heavy_admitted}/{heavy_capacity} heavy)"
    )]
    AdmissionFull {
        sequence: u64,
        admitted: usize,
        capacity: usize,
        heavy_admitted: usize,
        heavy_capacity: usize,
    },
    #[error("outbound SubChunkRequest FIFO is full at sequence {sequence} ({pending}/{capacity})")]
    OutboundFull {
        sequence: u64,
        pending: usize,
        capacity: usize,
    },
}
