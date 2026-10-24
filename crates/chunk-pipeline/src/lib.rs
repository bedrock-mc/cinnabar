//! Bounded terrain scheduling and publication for an ordered client world.

mod culling;
#[cfg(target_arch = "wasm32")]
pub mod jobs;
mod stream;

pub use culling::{CaveVisibilityScratch, CaveVisibilityWork, CaveVisibleSet};

#[cfg(feature = "benchmark-support")]
#[doc(hidden)]
pub use stream::benchmark_support;
pub use stream::{
    ActiveBlockCrack, ActorBlockSyncFence, BlockCrackSnapshot, BlockCrackStatus,
    BuildProfileIdentity, CohortManifestIdentity, CohortProgress, DECODE_DISPATCH_BUDGET_PER_POLL,
    DEFERRED_RETRY_CAPACITY, FULL_DETAIL_HYSTERESIS_CHUNKS, FarColumnSummary, FarSummaryInbox,
    ForcedRemeshManifest, ForcedRemeshManifestState, LIGHT_DISPATCH_BUDGET_PER_POLL,
    MAX_ACTIVE_BLOCK_CRACKS, MAX_IN_FLIGHT_DECODE_JOBS, MAX_IN_FLIGHT_LIGHT_JOBS,
    MAX_LOCAL_RESET_DISPATCH_EVIDENCE, MAX_PENDING_MESH_CHANGES, MAX_SUB_CHUNK_RETRIES,
    OUTBOUND_REQUEST_CAPACITY, PendingSubChunkRequest, PresentModeIdentity, PresentationSnapshot,
    PublicationSnapshot, PublicationStageCounters, RequestClass, RequestClassDepth,
    RequestQueueEvidence, SUB_CHUNK_RESPONSE_TIMEOUT, ServicedStream, StageDurations,
    SubChunkOutcomeCounters, SubChunkRequestBatch, ViewCohortStatus, WORK_RESULT_CAPACITY,
    WorldMeshChange, WorldStream, WorldStreamFatalError, WorldStreamNormalizationStats,
    WorldStreamPoll, WorldStreamService, WorldStreamStats, world_worker_threads,
};
#[cfg(feature = "publication-test-support")]
pub use stream::{FarSummaryFixture, PublicationFixtureIdentity, PublicationFixtureSnapshot};

#[cfg(not(target_arch = "wasm32"))]
pub use stream::{logical_cores, on_idle_world_cores};

pub use stream::ResourceMeshSnapshot;

use client_world::server_position;

#[cfg(test)]
#[global_allocator]
static ALLOCATOR: test_alloc::CountingAllocator = test_alloc::CountingAllocator;
