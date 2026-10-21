use bytes::Bytes;
use web_time::Instant;
// Keyed stream state uses hashbrown's seeded foldhash: SipHash dominated frame-thread lookups.
use hashbrown::{HashMap, HashSet};
use std::{
    collections::{BTreeMap, BTreeSet, BinaryHeap, VecDeque},
    sync::{
        Arc,
        atomic::{AtomicBool, AtomicU64, AtomicUsize, Ordering},
    },
    time::Duration,
};

use ::meshing::{
    BIOME_NEIGHBOUR_SLOT_COUNT, BlockClassifier, CameraMedium, ChunkBiomeTintIdentity, ChunkMesh,
    FaceConnectivity, MeshLightSample, MeshLightSampler, PackedBiomeRecord,
    chunk_publication_byte_len, mesh_dependency_mask, sample_camera_medium,
};
use assets::{
    LiveBiomeDefinition, NetworkIdMode, ResolvedBiomeTints, RuntimeAssets, RuntimeEntityAssets,
};
#[cfg(test)]
use client_world::ingestion::vanilla_dimension_range;
use client_world::ingestion::{
    BiomeDefinitionEvent, BlockCrackEvent, BlockUpdateEvent, DimensionRange, LevelChunkEvent,
    LevelChunkMode, SubChunkBatchEvent, SubChunkReplyAdmissionEvent, SyncedBlockUpdateEvent,
    WorldBootstrap, WorldEvent,
};
use crossbeam_channel::{Receiver, Sender, bounded};
use render_api::sub_chunk_requests::{
    MAX_SUB_CHUNK_COLUMN_HEIGHT, MAX_SUB_CHUNK_REQUESTS, SubChunkRequestMember,
    SubChunkRequestReceipt, sub_chunk_request_offset,
};
use thiserror::Error;
use world::{
    BiomeStorage, BlockEntityKey, BlockIds, BlockPos, BlockUpdate, BoundaryLightSample, ChunkKey,
    ChunkStore, DecodeError, DecodedBiomeColumn, DecodedBlockEntities, DimensionLightProfile,
    LightBlockAccess, LightBlockSample, LightBounds, LightChannel,
    LightProperties as SolverLightProperties, LightReadAccess, LightSolveError, LightSolveOutput,
    LightStore, LightStoreSnapshot, LightSubChunkKind, MeshDependencyMask, MeshNeighbourhood,
    NeighbourReach, PreparedSubChunkMutation, SectionSnapshot, SolverLimits, SubChunk, SubChunkKey,
    SubChunkLight, chunk_in_view,
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
mod column_bookkeeping;
use column_bookkeeping::{ColumnOwned, ColumnQueue, ColumnRecords};
mod column_scan;
mod column_set;
mod commit_budget;
mod connectivity;
mod construction;
mod decode;
mod diagnostics;
mod dimension_transfer;
mod dirty;
mod entity_occlusion;
mod far_summaries;
mod full_detail;
#[cfg(feature = "publication-test-support")]
pub use far_summaries::FarSummaryFixture;
pub use far_summaries::{FarColumnSummary, FarSummaryInbox};
pub use full_detail::FULL_DETAIL_HYSTERESIS_CHUNKS;
mod helpers;
mod light_diagnostics;
mod light_queries;
mod lighting;
mod map_data;
mod meshing;
mod model;
mod near_drawn;
mod particle_events;
mod polling;
mod prediction;
mod publication;
#[cfg(feature = "publication-test-support")]
mod publication_test_support;
mod ray_queries;
mod request_queue;
mod requests;
mod residency;
mod resource_reload;
pub use resource_reload::ResourceMeshSnapshot;
mod retries;
mod scheduler;
mod seasonal_foliage;
mod sequencing;
mod service;
#[cfg(all(test, feature = "tracy"))]
mod trace_test_support;
pub use service::{ServicedStream, WorldStreamService};
mod sign_edit;
mod transfer_priority;
pub(crate) mod workers;
#[cfg(not(target_arch = "wasm32"))]
pub use workers::on_idle_world_cores;
pub use workers::world_worker_threads;

use client_world::ingestion::{
    BlockMutationBatch, CommitBudget, CommitStep, DecodeCommit, DecodeCompletion, DecodeIds,
    DecodeJob, Footprint, LaneContext, PreparedSubChunkResult, PreparedWorldEvent, QueuedDecodeJob,
    dimension_slots,
};
use column_set::ColumnSubChunkSet;
use helpers::*;
use lighting::types::*;
use meshing::biome_records::pack_biome_record;
use meshing::types::*;
use request_queue::RequestQueue;

pub use diagnostics::{
    BuildProfileIdentity, CohortManifestIdentity, MAX_LOCAL_RESET_DISPATCH_EVIDENCE,
    PresentModeIdentity, PresentationSnapshot, PublicationSnapshot, PublicationStageCounters,
    RequestClass, RequestClassDepth, RequestQueueEvidence, StageDurations, SubChunkOutcomeCounters,
};
#[cfg(feature = "publication-test-support")]
pub use publication_test_support::{PublicationFixtureIdentity, PublicationFixtureSnapshot};
use render_api::PublicationAllowance;

/// Decode and mesh workers may each have at most this many completed results
/// waiting for the main thread. A full channel applies backpressure to Rayon.
pub const WORK_RESULT_CAPACITY: usize = 512;
use client_world::ingestion::MAX_ADMITTED_HEAVY_EVENTS;
pub const MAX_IN_FLIGHT_DECODE_JOBS: usize = MAX_ADMITTED_HEAVY_EVENTS;
pub const DECODE_DISPATCH_BUDGET_PER_POLL: usize = MAX_ADMITTED_HEAVY_EVENTS;
pub const OUTBOUND_REQUEST_CAPACITY: usize = 64;
pub const DEFERRED_RETRY_CAPACITY: usize = 64;
pub const MAX_SUB_CHUNK_RETRIES: u8 = 2;
pub const SUB_CHUNK_RESPONSE_TIMEOUT: Duration = Duration::from_secs(2);
/// How long an abandoned sub-chunk waits before it is asked for again, doubling each time it is
/// abandoned again up to [`MAX_SUB_CHUNK_RECOVERY_DELAY`].
const SUB_CHUNK_RECOVERY_DELAY: Duration = Duration::from_secs(2);
const MAX_SUB_CHUNK_RECOVERY_DELAY: Duration = Duration::from_secs(16);
pub const MAX_PENDING_MESH_CHANGES: usize = 512;
/// Quiet wait after local relevance or new publisher-cohort progress.
const UNSENT_COLUMN_GRACE: Duration = Duration::from_secs(1);
/// Completed meshes held for a publication permit rather than remeshed.
const MAX_STAGED_MESH_COMPLETIONS: usize = 256;
const MAX_STAGED_MESH_BYTES: u64 = 32 * 1024 * 1024;
const MAX_PENDING_SCHEDULER_SCANS_PER_POLL: usize = 128;
/// Block batches within both bounds prepare on the commit thread.
const INLINE_BLOCK_MUTATION_SUB_CHUNKS: usize = 4;
const INLINE_BLOCK_MUTATION_UPDATES: usize = 64;
/// Jobs one urgent pass may dispatch outside the poll.
const URGENT_DISPATCH_BUDGET: usize = 4;
/// Jobs a local block change may queue beyond full light or mesh workers, so streaming backlog
/// cannot hold it back: enough for its section and a few neighbours its relight reaches.
const URGENT_JOB_RESERVE: usize = 4;
/// Results one urgent pass may accept outside the poll.
const URGENT_RESULTS_PER_PASS: usize = 16;
/// Cooperative time for one urgent pass outside the poll, so it never sweeps a backlog.
const URGENT_PASS_BUDGET: Duration = Duration::from_millis(1);
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
/// Changed cells a relight revisits on its own; more than this solve the whole sub-chunk.
const MAX_SEEDED_LIGHT_CELLS: usize = 64;
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

    /// Squared distance, doubled more than 60° off the view axis and quadrupled behind the view
    /// plane, so sections in the view cone build before nearer ones beside or behind it.
    fn rank(self, key: SubChunkKey) -> f32 {
        let distance = distance_squared(key, self.position);
        let Some(forward) = self.forward else {
            return distance;
        };
        let offset = [
            key.x as f32 * 16.0 + 8.0 - self.position[0],
            key.y as f32 * 16.0 + 8.0 - self.position[1],
            key.z as f32 * 16.0 + 8.0 - self.position[2],
        ];
        let along = offset[0].mul_add(
            forward[0],
            offset[1].mul_add(forward[1], offset[2] * forward[2]),
        );
        if along < -8.0 {
            distance * 4.0
        } else if along < 0.5 * distance.sqrt() {
            distance * 2.0
        } else {
            distance
        }
    }

    /// The sub-chunk holding the camera.
    fn cell(self) -> [i32; 3] {
        self.position
            .map(|value| floor_to_i32(value).div_euclid(16))
    }

    /// Whether ranks queued under `previous` are stale: the camera changed sub-chunk or startup
    /// footing, or its view direction turned, in any axis, by more than a sixth of the 60° cone.
    fn reranks_from(self, previous: Self) -> bool {
        self.cell() != previous.cell()
            || self.startup_center != previous.startup_center
            || match (self.forward, previous.forward) {
                (None, None) => false,
                (Some(current), Some(previous)) => {
                    current[0].mul_add(
                        previous[0],
                        current[1].mul_add(previous[1], current[2] * previous[2]),
                    ) < RERANK_TURN_COSINE
                }
                _ => true,
            }
    }
}

/// Cosine of the 10° turn after which queued view ranks are refreshed.
const RERANK_TURN_COSINE: f32 = 0.984_807_8;

use model::{
    CorrelatedSubChunkAttempts, MeshChangeQueue, MeshCompletion, NormalizationErrorReason,
    PendingMesh, PendingSubChunk, PendingSubChunkColumn, RetrySchedule, RevisionTracker,
    SubChunkRecovery, queue_wait, split_block_update,
};

pub use block_cracks::{
    ActiveBlockCrack, BlockCrackSnapshot, BlockCrackStatus, MAX_ACTIVE_BLOCK_CRACKS,
};
pub use model::{
    CohortProgress, ForcedRemeshManifest, ForcedRemeshManifestState, PendingSubChunkRequest,
    SubChunkRequestBatch, ViewCohortStatus, WorldMeshChange, WorldStreamFatalError,
    WorldStreamNormalizationStats, WorldStreamPoll, WorldStreamStats,
};

/// Ordered Bedrock world ingestion and bounded background meshing.
pub struct WorldStream {
    authority: client_world::WorldAuthority,
    light_diagnostics: light_diagnostics::LightingDiagnostics,
    order: client_world::ingestion::OrderedCommitState,
    block_cracks: block_cracks::BlockCracks,
    block_entity_visuals: BlockEntityVisualDiagnostics,
    classifier: BlockClassifier,
    mesh_render_settings: ::meshing::MeshRenderSettings,
    mesh_render_revision: u64,
    /// Whether the server sent terrain before spawn; when it did not, startup
    /// has no view to wait for until the server publishes one.
    startup_terrain_announced: bool,
    /// Java meshing for shader packs, with the sorted network values of the blocks Java fills
    /// with water; see [`WorldStream::set_java_meshing`].
    java_meshing: Option<Arc<[u32]>>,
    seasonal_foliage: seasonal_foliage::SeasonalFoliage,
    /// Dimension at the newest admitted wire position; commits may still lag behind it.
    ingress_dimension: i32,
    /// Terrain ordered behind full decode admission, prepared once capacity frees.
    deferred_ingress: VecDeque<(u64, WorldEvent, Option<Bytes>)>,
    pending_decode: VecDeque<QueuedDecodeJob>,
    in_flight_decode_jobs: usize,
    predictions: prediction::DeferredPredictions,
    decode_tx: Sender<DecodeCompletion>,
    decode_rx: Receiver<DecodeCompletion>,
    biome_records: Option<Arc<meshing::biome_records::BiomeRecordCache>>,
    mesh_tx: Sender<MeshCompletion>,
    mesh_rx: Receiver<MeshCompletion>,
    lighting: lighting::Lighting,
    fatal_error: Option<WorldStreamFatalError>,
    revisions: RevisionTracker,
    applied_mesh_generations: HashMap<SubChunkKey, u64>,
    /// Sections whose last acknowledged mesh withdrew them, as past full detail, so none is drawn.
    withdrawn_meshes: HashSet<SubChunkKey>,
    actor_block_syncs: actor_block_sync::ActorBlockSyncs,
    mesh_dependency_masks: HashMap<SubChunkKey, dirty::RegisteredMeshDependencies>,
    mesh_jobs: scheduler::KeyedJobs<PendingMesh, u64, 2>,
    /// Unit view direction the schedulers favour; `None` orders by distance alone.
    view_forward: Option<[f32; 3]>,
    /// Orders the spawn columns and their light halo first until local terrain is ready.
    startup_priority: bool,
    dimension_transfer_priority: Option<transfer_priority::DimensionTransferPriority>,
    admitted_mesh_jobs: Arc<AtomicUsize>,
    mesh_memory: meshing::memory::MeshMemoryBudget,
    mesh_cancellations: HashMap<SubChunkKey, Arc<AtomicBool>>,
    /// Queued sections found to be air, by their pending revision; drained under removal credit.
    air_removals: BTreeMap<SubChunkKey, u64>,
    /// Last air entry examined, so bounded passes over `air_removals` rotate through it.
    air_removal_cursor: Option<SubChunkKey>,
    /// Sections the air pass returned to the worker queue, by revision, until a dispatch pops them.
    air_requeued: BTreeMap<SubChunkKey, u64>,
    /// Last `air_requeued` entry pruned, so bounded passes rotate through it.
    air_requeue_cursor: Option<SubChunkKey>,
    urgent_mesh_in_flight: HashSet<SubChunkKey>,
    staged_mesh_completions: VecDeque<MeshCompletion>,
    staged_mesh_bytes: u64,
    resident: ColumnSubChunkSet,
    known_air: ColumnSubChunkSet,
    loaded_columns: BTreeSet<ChunkKey>,
    connectivity: crate::culling::ConnectivityGrid,
    connectivity_generation: u64,
    requests: requests::SubChunkRequests,
    unsent_column_deadlines: BTreeMap<ChunkKey, Instant>,
    arrival_cohort: Option<residency::ArrivalCohort>,
    poll_deadline: Option<Instant>,
    /// Test allocation for exercising an expired urgent pass without timing assertions.
    #[cfg(test)]
    urgent_pass_budget: Duration,
    frame_deadline: Option<Instant>,
    /// Camera the last poll ordered work by, reused by urgent passes between polls.
    last_camera_position: [f32; 3],
    /// A live block change left urgent light or mesh work to dispatch.
    urgent_work_due: bool,
    /// Per-frame ingress, commit and scheduling allocation.
    poll_budget: Duration,
    polling: bool,
    /// The poll's one heavy commit allowed past its deadline is still unspent.
    poll_heavy_guarantee: bool,
    /// Set while a between-frames service owns the stream; a raised flag ends its work early.
    service_yield: Option<Arc<AtomicBool>>,
    /// Lent to a between-frames service every frame: retention evictions wait for the next poll,
    /// which the service reaches first, and frame polls leave heavy work to a service that had
    /// a full allocation's window.
    between_frames_service: bool,
    /// How long the service last held the stream, which bounds what it could take over.
    service_window: Duration,
    /// A deferred retention re-evaluation awaits the next poll.
    retention_due: bool,
    /// Last column visited by the current retention change.
    retention_cursor: Option<ChunkKey>,
    /// Maintenance items completed since construction, including interior row skips.
    maintenance_items: u64,
    /// Maximum elapsed time of a maintenance item, commit or worker-result acceptance.
    service_item_max: Duration,
    /// One maintenance item may progress past the active allocation's deadline.
    maintenance_guarantee: bool,
    /// This frame poll leaves chunk-data commits to the service.
    chunk_data_offloaded: bool,
    /// Ordered commit steps applied so far, so a poll can report commit progress.
    commit_steps: u64,
    publication_allowance: Option<PublicationAllowance>,
    mesh_changes: MeshChangeQueue,
    publisher: cohort::PublisherScope,
    chunk_radius: Option<i32>,
    /// The view radius the player asked the server for; it sets the render distance.
    requested_radius: Option<i32>,
    last_retention_center: Option<ChunkKey>,
    last_retention_radius: Option<i32>,
    local_player_chunk: Option<ChunkKey>,
    far_terrain: far_summaries::FarTerrainFeed,
    full_detail: full_detail::FullDetail,
    near_drawn: near_drawn::NearDrawn,
    stats: WorldStreamStats,
}

#[cfg(test)]
pub(crate) mod tests;

use client_world::{
    CommittedAudioEvent, CommittedCameraEvent, CommittedControlEvent, CommittedParticleEvent,
    CommittedUiEvent, ViewCohort,
};
