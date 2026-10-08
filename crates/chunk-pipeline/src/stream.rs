use bytes::Bytes;
use std::{
    collections::{BTreeMap, BTreeSet, BinaryHeap, HashMap, HashSet, VecDeque},
    sync::{
        Arc,
        atomic::{AtomicBool, AtomicU64, AtomicUsize, Ordering},
    },
    time::{Duration, Instant},
};

use ::meshing::{
    BIOME_NEIGHBOUR_SLOT_COUNT, BlockClassifier, CameraMedium, ChunkBiomeTintIdentity, ChunkMesh,
    FaceConnectivity, MeshLightSample, MeshLightSampler, PackedBiomeRecord,
    chunk_publication_byte_len, mesh_dependency_mask,
    mesh_sub_chunk_in_neighbourhood_with_lighting, sample_camera_medium,
};
use assets::{
    LiveBiomeDefinition, NetworkIdMode, ResolvedBiomeTints, RuntimeAssets, RuntimeEntityAssets,
};
#[cfg(test)]
use client_world::ingestion::vanilla_dimension_range;
use client_world::ingestion::{
    BiomeDefinitionEvent, BlockCrackEvent, BlockUpdateEvent, DimensionRange, LevelChunkEvent,
    LevelChunkMode, Packet, SubChunkBatchEvent, SubChunkReplyAdmissionEvent,
    SyncedBlockUpdateEvent, WorldBootstrap, WorldEvent, request_sub_chunk_column,
};
use crossbeam_channel::{Receiver, Sender, bounded};
use thiserror::Error;
use world::{
    BiomeStorage, BlockEntityKey, BlockIds, BlockPos, BlockUpdate, BoundaryLightSample, ChunkKey,
    ChunkStore, DecodeError, DecodedBiomeColumn, DecodedBlockEntities, DimensionLightProfile,
    LightBlockAccess, LightBlockSample, LightBounds, LightChannel,
    LightProperties as SolverLightProperties, LightReadAccess, LightSolveError, LightSolveOutput,
    LightStore, LightStoreSnapshot, LightSubChunkKind, MeshDependencyMask, MeshNeighbourhood,
    PreparedSubChunkMutation, SectionSnapshot, SolverLimits, SubChunk, SubChunkKey, SubChunkLight,
    chunk_in_view,
};

#[cfg(test)]
use world::solve_light;

use client_world::LocalPlayerFeed;
use client_world::ResolvedServerPosition;
use client_world::{
    BackingBlockIdentity, BlockEntityVisualDiagnostics, adjudicate_block_entity_visual,
};

mod actor_block_sync;
pub use actor_block_sync::ActorBlockSyncFence;
#[cfg(feature = "benchmark-support")]
pub mod benchmark_support;
mod block_cracks;
mod block_entities;
mod block_events;
mod cave_visibility;
mod cohort;
mod column_set;
mod commit_budget;
mod connectivity;
mod construction;
mod decode;
mod diagnostics;
mod dimension_transfer;
mod dirty;
mod helpers;
mod light_diagnostics;
mod lighting;
mod map_data;
mod meshing;
mod model;
mod particle_events;
mod polling;
mod prediction;
mod publication;
#[cfg(feature = "publication-test-support")]
mod publication_test_support;
mod request_queue;
mod requests;
mod residency;
mod resource_reload;
pub use resource_reload::ResourceMeshSnapshot;
mod retries;
mod scheduler;
mod seasonal_foliage;
mod sequencing;
mod sign_edit;
mod transfer_priority;
mod workers;

pub use client_world::ingestion::WorldStreamError;
use client_world::ingestion::{
    BlockMutationBatch, CommitStep, DecodeCommit, DecodeCompletion, DecodeIds, DecodeJob,
    PreparedSubChunkResult, PreparedWorldEvent, QueuedDecodeJob, dimension_slots,
};
use column_set::ColumnSubChunkSet;
use helpers::*;
use lighting::types::*;
use meshing::types::*;
use request_queue::RequestQueue;

pub use diagnostics::{
    BuildProfileIdentity, CohortManifestIdentity, MAX_LOCAL_RESET_DISPATCH_EVIDENCE,
    Phase2PresentationSnapshot, Phase2PublicationSnapshot, PresentModeIdentity,
    PublicationStageCounters, RequestClass, RequestClassDepth, RequestQueueEvidence,
    StageDurations, SubChunkOutcomeCounters,
};
#[cfg(feature = "publication-test-support")]
pub use publication_test_support::{PublicationFixtureIdentity, PublicationFixtureSnapshot};
pub use render_api::{
    PublicationAllowance, PublicationPermit, PublicationPermitStage, PublicationServiceConfig,
};

/// Decode and mesh workers may each have at most this many completed results
/// waiting for the main thread. A full channel applies backpressure to Rayon.
pub const WORK_RESULT_CAPACITY: usize = 512;
pub use client_world::ingestion::{MAX_ADMITTED_HEAVY_EVENTS, MAX_ADMITTED_WORLD_EVENTS};
pub const MAX_IN_FLIGHT_DECODE_JOBS: usize = MAX_ADMITTED_HEAVY_EVENTS;
pub const DECODE_DISPATCH_BUDGET_PER_POLL: usize = MAX_ADMITTED_HEAVY_EVENTS;
pub use render_api::PHASE0_MAX_VIEW_RADIUS_CHUNKS;
pub const OUTBOUND_REQUEST_CAPACITY: usize = 64;
pub const DEFERRED_RETRY_CAPACITY: usize = 64;
pub const MAX_SUB_CHUNK_RETRIES: u8 = 2;
pub const SUB_CHUNK_RESPONSE_TIMEOUT: Duration = Duration::from_secs(2);
pub const MAX_PENDING_MESH_CHANGES: usize = 512;
/// Quiet wait after local relevance or new publisher-cohort progress.
const UNSENT_COLUMN_GRACE: Duration = Duration::from_secs(1);
/// Completed meshes held for a publication permit rather than remeshed.
const MAX_STAGED_MESH_COMPLETIONS: usize = 256;
const MAX_STAGED_MESH_BYTES: u64 = 32 * 1024 * 1024;
const MAX_PENDING_SCHEDULER_SCANS_PER_POLL: usize = 128;
const MAX_PENDING_MESH_QUEUE_WORK_PER_POLL: usize = MAX_PENDING_MESH_CHANGES;
pub const MAX_IN_FLIGHT_LIGHT_JOBS: usize = 32;
const MIN_EFFECTIVE_LIGHT_JOB_CAP: usize = 2;
const MAX_LIGHT_COLUMN_BATCH_SUB_CHUNKS: usize = 32;
const INITIAL_LIGHT_BACKLOG_THRESHOLD: usize = 256;
/// Quiet relighting admits half the light workers' width.
fn light_job_cap_for_threads(light_workers: usize) -> usize {
    (light_workers / 2).clamp(MIN_EFFECTIVE_LIGHT_JOB_CAP, MAX_IN_FLIGHT_LIGHT_JOBS)
}
fn effective_light_job_cap() -> usize {
    light_job_cap_for_threads(workers::WORKERS.size().background)
}
fn initial_light_job_cap() -> usize {
    // Initial lighting fills every light worker because it gates ready geometry.
    workers::WORKERS
        .size()
        .background
        .clamp(MIN_EFFECTIVE_LIGHT_JOB_CAP, MAX_IN_FLIGHT_LIGHT_JOBS)
}
pub const LIGHT_DISPATCH_BUDGET_PER_POLL: usize = MAX_IN_FLIGHT_LIGHT_JOBS;
const LIGHT_RESULT_CAPACITY: usize = MAX_IN_FLIGHT_LIGHT_JOBS * MAX_LIGHT_COLUMN_BATCH_SUB_CHUNKS;
const LIGHT_SOLVE_LIMITS: SolverLimits = SolverLimits::new(4_096, 1_000_000);
const LIGHT_COLUMN_SOLVE_LIMITS: SolverLimits = SolverLimits::new(
    4_096 * MAX_LIGHT_COLUMN_BATCH_SUB_CHUNKS,
    1_000_000 * MAX_LIGHT_COLUMN_BATCH_SUB_CHUNKS,
);

#[derive(Debug, Clone, Copy)]
struct PendingSchedulerCandidate {
    distance_squared: f32,
    startup_class: u8,
    key: SubChunkKey,
    revision: u64,
    urgent: bool,
    transfer: bool,
}

impl PendingSchedulerCandidate {
    fn new(key: SubChunkKey, revision: u64, view: SchedulerView, urgent: bool) -> Self {
        Self {
            distance_squared: view.rank(key),
            startup_class: view.startup_class(key),
            key,
            revision,
            urgent,
            transfer: false,
        }
    }

    fn refresh_rank(&mut self, view: SchedulerView) {
        self.distance_squared = view.rank(self.key);
        self.startup_class = view.startup_class(self.key);
    }
}

impl PartialEq for PendingSchedulerCandidate {
    fn eq(&self, other: &Self) -> bool {
        self.transfer == other.transfer
            && self.urgent == other.urgent
            && self.startup_class == other.startup_class
            && self
                .distance_squared
                .total_cmp(&other.distance_squared)
                .is_eq()
            && self.key == other.key
            && self.revision == other.revision
    }
}

impl Eq for PendingSchedulerCandidate {}

impl PartialOrd for PendingSchedulerCandidate {
    fn partial_cmp(&self, other: &Self) -> Option<std::cmp::Ordering> {
        Some(self.cmp(other))
    }
}

impl Ord for PendingSchedulerCandidate {
    fn cmp(&self, other: &Self) -> std::cmp::Ordering {
        self.transfer
            .cmp(&other.transfer)
            .then_with(|| self.urgent.cmp(&other.urgent))
            .then_with(|| other.startup_class.cmp(&self.startup_class))
            .then_with(|| {
                other
                    .distance_squared
                    .total_cmp(&self.distance_squared)
                    .then_with(|| other.key.cmp(&self.key))
                    .then_with(|| other.revision.cmp(&self.revision))
            })
    }
}

/// Camera the work schedulers order by: nearest first, sub-chunks in front of the view ahead of
/// those behind it, as vanilla queues render-chunk builds from the visible set.
#[derive(Debug, Clone, Copy)]
struct SchedulerView {
    position: [f32; 3],
    forward: Option<[f32; 3]>,
    /// Spawn column while startup priority holds; `None` orders by the camera alone.
    startup_center: Option<ChunkKey>,
}

impl SchedulerView {
    /// 0 for the spawn columns, 1 for their light halo, 2 for everything else.
    fn startup_class(self, key: SubChunkKey) -> u8 {
        let Some(center) = self
            .startup_center
            .filter(|center| center.dimension == key.dimension)
        else {
            return 2;
        };
        let distance = key.x.abs_diff(center.x).max(key.z.abs_diff(center.z));
        if distance <= cohort::STARTUP_RADIUS as u32 {
            0
        } else if distance <= (cohort::STARTUP_RADIUS + 1) as u32 {
            1
        } else {
            2
        }
    }

    /// Squared distance, quadrupled (twice the distance) behind the view plane.
    fn rank(self, key: SubChunkKey) -> f32 {
        let distance = distance_squared(key, self.position);
        let behind = self.forward.is_some_and(|forward| {
            let offset = [
                key.x as f32 * 16.0 + 8.0 - self.position[0],
                key.y as f32 * 16.0 + 8.0 - self.position[1],
                key.z as f32 * 16.0 + 8.0 - self.position[2],
            ];
            offset[0].mul_add(
                forward[0],
                offset[1].mul_add(forward[1], offset[2] * forward[2]),
            ) < -8.0
        });
        if behind { distance * 4.0 } else { distance }
    }

    /// Changes when the camera crosses a sub-chunk or turns into another eighth of the compass.
    fn cell(self) -> [i32; 4] {
        let [x, y, z] = self
            .position
            .map(|value| floor_to_i32(value).div_euclid(16));
        let sector = self.forward.map_or(-1, |forward| {
            ((forward[2].atan2(forward[0]) / std::f32::consts::TAU * 8.0).floor() as i32)
                .rem_euclid(8)
        });
        [x, y, z, sector]
    }
}

use model::{
    CorrelatedSubChunkAttempts, MeshChangeQueue, MeshCompletion, NormalizationErrorReason,
    PendingMesh, PendingSubChunk, PendingSubChunkColumn, RetrySchedule, RevisionTracker,
    queue_wait, split_block_update,
};

pub use block_cracks::{
    ActiveBlockCrack, BlockCrackSnapshot, BlockCrackStatus, MAX_ACTIVE_BLOCK_CRACKS,
};
pub use model::{
    ForcedRemeshManifest, ForcedRemeshManifestState, PendingSubChunkRequest, ViewCohortStatus,
    WorldMeshChange, WorldStreamFatalError, WorldStreamNormalizationStats, WorldStreamPoll,
    WorldStreamStats,
};

/// Ordered Bedrock world ingestion and bounded background meshing.
pub struct WorldStream {
    authority: client_world::WorldAuthority,
    light_diagnostics: light_diagnostics::LightingDiagnostics,
    order: client_world::ingestion::OrderedCommitState,
    block_cracks: block_cracks::BlockCracks,
    block_entity_visuals: BlockEntityVisualDiagnostics,
    classifier: BlockClassifier,
    /// Whether the server sent terrain before spawn; when it did not, startup
    /// has no view to wait for until the server publishes one.
    startup_terrain_announced: bool,
    seasonal_foliage: seasonal_foliage::SeasonalFoliage,
    pending_decode: VecDeque<QueuedDecodeJob>,
    in_flight_decode_jobs: usize,
    predictions: prediction::DeferredPredictions,
    decode_tx: Sender<DecodeCompletion>,
    decode_rx: Receiver<DecodeCompletion>,
    mesh_tx: Sender<MeshCompletion>,
    mesh_rx: Receiver<MeshCompletion>,
    lighting: lighting::Lighting,
    fatal_error: Option<WorldStreamFatalError>,
    revisions: RevisionTracker,
    applied_mesh_generations: HashMap<SubChunkKey, u64>,
    actor_block_syncs: actor_block_sync::ActorBlockSyncs,
    mesh_dependency_masks: HashMap<SubChunkKey, (u64, MeshDependencyMask)>,
    mesh_jobs: scheduler::KeyedJobs<PendingMesh, u64, 2>,
    /// Unit view direction the schedulers favour; `None` orders by distance alone.
    view_forward: Option<[f32; 3]>,
    /// Orders the spawn columns and their light halo first until local terrain is ready.
    startup_priority: bool,
    dimension_transfer_priority: Option<transfer_priority::DimensionTransferPriority>,
    admitted_mesh_jobs: Arc<AtomicUsize>,
    mesh_memory: meshing::memory::MeshMemoryBudget,
    mesh_cancellations: HashMap<SubChunkKey, Arc<AtomicBool>>,
    urgent_mesh_in_flight: HashSet<SubChunkKey>,
    staged_mesh_completions: VecDeque<MeshCompletion>,
    staged_mesh_bytes: u64,
    resident: ColumnSubChunkSet,
    known_air: ColumnSubChunkSet,
    loaded_columns: BTreeSet<ChunkKey>,
    connectivity: crate::culling::ConnectivityGrid,
    connectivity_generation: u64,
    requests: requests::SubChunkRequests,
    unsent_column_deadlines: HashMap<ChunkKey, Instant>,
    arrival_cohort: Option<residency::ArrivalCohort>,
    poll_deadline: Option<Instant>,
    frame_deadline: Option<Instant>,
    /// Per-frame ingress, commit and scheduling allocation.
    poll_budget: Duration,
    polling: bool,
    publication_allowance: Option<PublicationAllowance>,
    mesh_changes: MeshChangeQueue,
    publisher: cohort::PublisherScope,
    chunk_radius: Option<i32>,
    last_retention_center: Option<ChunkKey>,
    last_retention_radius: Option<i32>,
    local_player_chunk: Option<ChunkKey>,
    stats: WorldStreamStats,
}

#[cfg(test)]
pub(crate) mod tests;

use client_world::{
    CommittedAudioEvent, CommittedCameraEvent, CommittedControlEvent, CommittedParticleEvent,
    CommittedUiEvent, ViewCohort,
};
