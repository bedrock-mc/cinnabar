//! Bounded terrain scheduling and publication for an ordered client world.

mod culling;
mod stream;

pub use culling::{CaveVisibilityScratch, CaveVisibilityWork, CaveVisibleSet};

#[cfg(feature = "benchmark-support")]
#[doc(hidden)]
pub use stream::benchmark_support;
pub use stream::distant_terrain::{
    DistantTerrainColours, DistantTerrainLink, DistantTerrainMeshStats,
    DistantTerrainSelectionStats, DistantTerrainStats, DistantTerrainUnavailable,
    DistantTerrainView, LOD_CPU_MEMORY_BYTES, LOD_MESH_CACHE_BYTES, LOD_SETTLE_POLLS,
    MAX_LOD_BACKLOG, MAX_LOD_COLUMNS_PER_POLL, MAX_LOD_FINAL_CAPTURES_PER_POLL, MAX_LOD_MESH_JOBS,
    MAX_LOD_PENDING_FINAL_CAPTURES, MAX_LOD_UPDATE_BYTES, MAX_LOD_WRITERS,
};
pub use stream::{
    ActiveBlockCrack, ActorBlockSyncFence, BlockCrackSnapshot, BlockCrackStatus,
    BuildProfileIdentity, CohortManifestIdentity, CohortProgress, DECODE_DISPATCH_BUDGET_PER_POLL,
    DEFERRED_RETRY_CAPACITY, ForcedRemeshManifest, ForcedRemeshManifestState,
    LIGHT_DISPATCH_BUDGET_PER_POLL, MAX_ACTIVE_BLOCK_CRACKS, MAX_IN_FLIGHT_DECODE_JOBS,
    MAX_IN_FLIGHT_LIGHT_JOBS, MAX_LOCAL_RESET_DISPATCH_EVIDENCE, MAX_PENDING_MESH_CHANGES,
    MAX_SUB_CHUNK_RETRIES, OUTBOUND_REQUEST_CAPACITY, PendingSubChunkRequest, PresentModeIdentity,
    PresentationSnapshot, PublicationSnapshot, PublicationStageCounters, RequestClass,
    RequestClassDepth, RequestQueueEvidence, SUB_CHUNK_RESPONSE_TIMEOUT, ServicedStream,
    StageDurations, SubChunkOutcomeCounters, ViewCohortStatus, WORK_RESULT_CAPACITY,
    WorldMeshChange, WorldStream, WorldStreamFatalError, WorldStreamNormalizationStats,
    WorldStreamPoll, WorldStreamService, WorldStreamStats, on_idle_world_cores,
    world_worker_threads,
};
#[cfg(feature = "publication-test-support")]
pub use stream::{PublicationFixtureIdentity, PublicationFixtureSnapshot};

pub use stream::ResourceMeshSnapshot;

use client_world::server_position;

#[cfg(test)]
#[global_allocator]
static ALLOCATOR: test_alloc::CountingAllocator = test_alloc::CountingAllocator;
