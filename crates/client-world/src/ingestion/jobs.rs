//! Immutable decode work admitted by the ordered world owner.

use bytes::Bytes;
use protocol::{BlockEntityUpdateEvent, LevelChunkEvent, SubChunkBatchEvent};
use std::{
    sync::Arc,
    time::{Duration, Instant},
};
use world::{BlockUpdate, DimensionSlots, SubChunk, SubChunkKey};

use super::{DecodeIds, PreparedWorldEvent};

#[derive(Debug)]
pub struct DecodeCompletion {
    pub sequence: u64,
    pub event: PreparedWorldEvent,
    pub queue_wait: Duration,
}

#[derive(Debug)]
pub struct QueuedDecodeJob {
    pub queued_at: Instant,
    pub job: DecodeJob,
}

#[derive(Debug)]
pub enum DecodeJob {
    InlineLevelChunk {
        sequence: u64,
        event: LevelChunkEvent,
        payload: Bytes,
        slots: DimensionSlots,
        count: usize,
        ids: DecodeIds,
    },
    RequestLevelChunk {
        sequence: u64,
        event: LevelChunkEvent,
        payload: Bytes,
        slots: DimensionSlots,
        ids: DecodeIds,
    },
    SubChunks {
        sequence: u64,
        batch: SubChunkBatchEvent,
        ids: DecodeIds,
    },
    BlockUpdates {
        sequence: u64,
        batches: Vec<BlockMutationBatch>,
        ids: DecodeIds,
    },
    BlockEntityUpdate {
        sequence: u64,
        event: BlockEntityUpdateEvent,
    },
}

#[derive(Debug)]
pub struct BlockMutationBatch {
    pub key: SubChunkKey,
    pub previous: Option<Arc<SubChunk>>,
    pub updates: Vec<BlockUpdate>,
}
