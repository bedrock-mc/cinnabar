//! Client-side Bedrock world model.
//!
//! Chunk block data remains palette + packed indices at runtime. The decoder
//! intentionally never creates flat per-block arrays.

mod biome;
mod block_entity;
mod chunk;
mod collision_revision;
mod error;
mod light;
mod light_solver;
mod mesh_neighbourhood;
mod mutation;
mod palette;
mod store;
mod sub_chunk;

pub use biome::{BiomeStorage, DecodedBiomeColumn};
pub use block_entity::{
    BlockEntityError, BlockEntityKey, BlockEntityNbt, BlockEntityNbtError, DecodedBlockEntities,
    DecodedSubChunk, MAX_BLOCK_ENTITIES_PER_CHUNK, MAX_BLOCK_ENTITIES_PER_SUB_CHUNK,
    MAX_BLOCK_ENTITY_BYTES_PER_CHUNK, MAX_BLOCK_ENTITY_NBT_BYTES, MAX_BLOCK_ENTITY_TAIL_BYTES,
    MAX_NBT_COLLECTION_LENGTH, MAX_NBT_DEPTH, MAX_NBT_STRING_BYTES, MAX_NBT_TAGS,
    RootByteCandidate,
};
pub use chunk::{Chunk, ChunkKey, SubChunkKey};
pub use error::{CollisionRevisionError, DecodeError, MutationError};
pub use light::{
    LIGHT_SAMPLES_PER_SUB_CHUNK, LightChannel, LightNibbleStorage, LightStorageError, LightStore,
    LightStoreSnapshot, LightSubChunkKind, SubChunkLight,
};
pub use light_solver::{
    BlockPos, BoundaryLightSample, DimensionLightProfile, EmptyLight, LightBlockAccess,
    LightBlockSample, LightBounds, LightProperties, LightReadAccess, LightSolveError,
    LightSolveOutput, LightSolveStats, SolverLimits, solve_light,
};
pub use mesh_neighbourhood::{MeshDependencyMask, MeshNeighbourhood, MeshSample};
pub use mutation::BlockUpdate;
pub use palette::{BLOCKS_PER_SUB_CHUNK, Palette, PalettedStorage};
pub use store::{
    ApplyLevelChunk, ChunkCollisionRevision, ChunkStore, DecodedLevelChunk, MAX_LEVEL_SUBCHUNKS,
    PreparedSubChunkMutation,
};
pub use sub_chunk::{MAX_PALETTE_ENTRIES, MAX_STORAGE_COUNT, SubChunk};
