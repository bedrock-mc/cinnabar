//! Runs admitted payload work without owning a scheduling pool.

use super::{
    DecodeCompletion, DecodeJob, PreparedWorldEvent, prepare_block_mutations, prepare_sub_chunks,
};
use std::time::Instant;
use world::{
    BlockEntityKey, ChunkKey, DecodedBlockEntities, DecodedLevelChunk, decode_column_tail,
};

impl DecodeJob {
    /// Decodes one immutable job and returns its completion to the caller's bounded lane.
    #[must_use]
    pub fn run(self, queued_at: Instant) -> DecodeCompletion {
        let started = Instant::now();
        let queue_wait = started.saturating_duration_since(queued_at);
        match self {
            DecodeJob::InlineLevelChunk {
                sequence,
                event,
                payload,
                slots,
                count,
                ids,
            } => {
                let chunk = ChunkKey::new(event.dimension, event.x, event.z);
                let decoded =
                    DecodedLevelChunk::decode_inline(chunk, slots, count, &payload, &ids, &ids);
                DecodeCompletion {
                    sequence,
                    queue_wait,
                    event: PreparedWorldEvent::InlineLevelChunk {
                        event,
                        decoded,
                        duration: started.elapsed(),
                    },
                }
            }
            DecodeJob::RequestLevelChunk {
                sequence,
                event,
                payload,
                slots,
                ids,
            } => {
                let chunk = ChunkKey::new(event.dimension, event.x, event.z);
                let decoded = decode_column_tail(chunk, slots, &payload, &ids);
                DecodeCompletion {
                    sequence,
                    queue_wait,
                    event: PreparedWorldEvent::RequestLevelChunk {
                        event,
                        decoded,
                        duration: started.elapsed(),
                    },
                }
            }
            DecodeJob::SubChunks {
                sequence,
                batch,
                ids,
            } => {
                let dimension = batch.dimension;
                let entries = prepare_sub_chunks(batch, &ids);
                DecodeCompletion {
                    sequence,
                    queue_wait,
                    event: PreparedWorldEvent::SubChunks {
                        dimension,
                        entries,
                        duration: started.elapsed(),
                    },
                }
            }
            DecodeJob::BlockUpdates {
                sequence,
                batches,
                ids,
            } => {
                let result = prepare_block_mutations(batches, &ids);
                DecodeCompletion {
                    sequence,
                    queue_wait,
                    event: PreparedWorldEvent::BlockUpdates {
                        result,
                        duration: started.elapsed(),
                    },
                }
            }
            DecodeJob::BlockEntityUpdate { sequence, event } => {
                let key = BlockEntityKey::new(
                    event.dimension,
                    event.position[0],
                    event.position[1],
                    event.position[2],
                );
                let decoded = DecodedBlockEntities::decode_live(key, &event.nbt);
                DecodeCompletion {
                    sequence,
                    queue_wait,
                    event: PreparedWorldEvent::BlockEntityUpdate {
                        key,
                        decoded,
                        duration: started.elapsed(),
                    },
                }
            }
        }
    }
}
