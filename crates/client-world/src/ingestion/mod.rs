//! Bounded, ordered world admission and synchronous mutation progress.

mod contracts;
mod decode;
mod decode_diagnostics;
mod ids;
mod jobs;
mod ordered;
mod prepare;

pub use contracts::{
    PreparedBlockMutations, PreparedSubChunk, PreparedSubChunkResult, PreparedWorldEvent,
    WorldStreamError,
};
pub(crate) use decode_diagnostics::{DecodeDiagnostics, block_registry_sha256};
pub use ids::{DecodeIds, default_biome_id, dimension_slots};
pub use jobs::{BlockMutationBatch, DecodeCompletion, DecodeJob, QueuedDecodeJob};
pub use ordered::{CommitStep, DecodeCommit, OrderedCommitState};
pub use prepare::{light_semantics_changed, prepare_block_mutations, prepare_sub_chunks};
pub use protocol::{CustomBlocks, CustomStateValue};

/// Maximum admitted world events, including retained committed consumers.
pub const MAX_ADMITTED_WORLD_EVENTS: usize = 64;
/// Maximum admitted terrain and block mutation events.
pub const MAX_ADMITTED_HEAVY_EVENTS: usize = 32;

#[cfg(test)]
mod tests;

// Wire records accepted by the world ingress adapter. Decoding remains in this crate.
pub use protocol::{
    ActorAttribute, ActorEvent, ActorHandedness, AudioEvent, BiomeDefinitionEvent,
    BiomeDefinitionsEvent, BlockCrackAction, BlockCrackEvent, BlockEntityUpdateEvent,
    BlockEventEvent, BlockUpdateEvent, ChangeDimensionEvent, DaylightCycleUpdateEvent,
    DimensionHeightDiagnostic, DimensionRange, HeightmapDiagnostic, ItemRegistryEvent,
    LevelChunkEvent, LevelChunkMode, MAP_IMAGE_SIDE, MapDataEvent, MovePlayerEvent,
    NetworkItemStack, OpenSignEvent, PLAYER_NETWORK_OFFSET, Packet, ParticleEvent,
    PlayerMovementCorrectionEvent, RespawnEvent, SetTimeEvent, SubChunkBatchEvent,
    SubChunkDiagnostic, SubChunkReplyAdmissionEvent, SubChunkResult, SubChunkUnavailable, UiEvent,
    WeatherUpdateEvent, WorldBootstrap, WorldEvent, request_sub_chunk_column,
    vanilla_dimension_range,
};
