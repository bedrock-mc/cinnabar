//! Client-side Bedrock world model.
//!
//! Chunk block data remains palette + packed indices at runtime. The decoder
//! intentionally never creates flat per-block arrays.

mod chunk;
mod error;
mod palette;
mod store;
mod sub_chunk;

pub use chunk::{Chunk, ChunkKey, SubChunkKey};
pub use error::DecodeError;
pub use palette::{BLOCKS_PER_SUB_CHUNK, Palette, PalettedStorage};
pub use store::{ApplyLevelChunk, ChunkStore, MAX_LEVEL_SUBCHUNKS};
pub use sub_chunk::{MAX_PALETTE_ENTRIES, MAX_STORAGE_COUNT, SubChunk};
