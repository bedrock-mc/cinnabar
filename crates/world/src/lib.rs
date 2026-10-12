//! Client-side Bedrock world model.
//!
//! Chunk block data remains palette + packed indices at runtime. The decoder
//! intentionally never creates flat per-block arrays.

mod biome;
mod block_entity;
mod block_highlights;
mod chunk;
mod chunk_grid;
mod collision_revision;
mod dimension;
mod error;
mod light;
mod light_solver;
mod mesh_neighbourhood;
mod mutation;
mod nbt_tree;
mod palette;
mod section_snapshot;
mod store;
mod sub_chunk;
pub mod weather;

pub use biome::{BiomeIds, BiomeStorage, DecodedBiomeColumn, RawBiomeIds};
pub use block_entity::{
    BlockEntityError, BlockEntityKey, BlockEntityNbt, BlockEntityNbtError, DecodedBlockEntities,
    DecodedSubChunk, MAX_BLOCK_ENTITIES_PER_CHUNK, MAX_BLOCK_ENTITIES_PER_SUB_CHUNK,
    MAX_BLOCK_ENTITY_BYTES_PER_CHUNK, MAX_BLOCK_ENTITY_NBT_BYTES, MAX_BLOCK_ENTITY_TAIL_BYTES,
    MAX_NBT_COLLECTION_LENGTH, MAX_NBT_DEPTH, MAX_NBT_STRING_BYTES, MAX_NBT_TAGS,
    RootByteCandidate, ShapeActor,
};
pub use block_highlights::BlockHighlightScan;
pub use chunk::{Chunk, ChunkKey, SubChunkKey};
pub use chunk_grid::{CHUNK_VIEW_SLACK, chunk_in_view, chunk_view_distance};
pub use dimension::dimension_loading_fallback_y;
pub use error::{CollisionRevisionError, DecodeError, MutationError};
pub use light::{
    LIGHT_SAMPLES_PER_SUB_CHUNK, LightChannel, LightNibbleStorage, LightStorageError, LightStore,
    LightStoreSnapshot, LightSubChunkKind, SubChunkLight,
};
pub use light_solver::{
    BlockPos, BoundaryLightSample, DimensionLightProfile, EmptyLight, LightBlockAccess,
    LightBlockSample, LightBounds, LightProperties, LightReadAccess, LightSolveError,
    LightSolveOutput, LightSolveStats, LightSolverScratch, SolverLimits, solve_light,
    solve_light_changes_with_scratch, solve_light_full_with_scratch, solve_light_with_scratch,
};
pub use mesh_neighbourhood::{MeshDependencyMask, MeshNeighbourhood, MeshSample};
pub use mutation::{BlockUpdate, NeighbourReach};
pub use nbt_tree::{NbtCompound, NbtValue};
pub use palette::{BLOCKS_PER_SUB_CHUNK, Palette, PalettedStorage, SUB_CHUNK_SIDE};
pub use section_snapshot::SectionSnapshot;
pub use store::{
    ApplyLevelChunk, ChunkCollisionRevision, ChunkStore, DecodedLevelChunk, DimensionSlots,
    PreparedSubChunkMutation, decode_column_tail,
};
pub use sub_chunk::{BlockIds, MAX_PALETTE_ENTRIES, MAX_STORAGE_COUNT, RawBlockIds, SubChunk};

/// Bedrock simulation ticks in one day-night cycle.
pub const TICKS_PER_DAY: u32 = 24_000;

/// Stable identities shared by decoded and compiled block states.
pub mod block_state_identity;
