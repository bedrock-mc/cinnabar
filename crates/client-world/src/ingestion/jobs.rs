//! Immutable decode work admitted by the ordered world owner.

use bytes::Bytes;
use protocol::{
    BlockEntityUpdateEvent, LevelChunkEvent, SubChunkBatchEvent, SyncedBlockUpdateEvent,
};
use std::{collections::BTreeMap, sync::Arc, time::Duration};
use web_time::Instant;
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
        /// The column's resident sections when the job was admitted, for comparing a resend.
        previous: Option<Arc<BTreeMap<i32, Arc<SubChunk>>>>,
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
    SyncedBlockUpdates {
        sequence: u64,
        batches: Vec<BlockMutationBatch>,
        events: Vec<SyncedBlockUpdateEvent>,
        /// Earlier packet batches only for sections with active cracks.
        crack_batches: Vec<BlockMutationBatch>,
        ids: DecodeIds,
    },
}

#[derive(Debug)]
pub struct BlockMutationBatch {
    pub key: SubChunkKey,
    pub previous: Option<Arc<SubChunk>>,
    pub updates: Vec<BlockUpdate>,
}
