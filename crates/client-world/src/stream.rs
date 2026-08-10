use bytes::Bytes;
use std::{
    collections::{BTreeMap, BTreeSet, BinaryHeap, HashMap, HashSet, VecDeque},
    sync::{
        Arc,
        atomic::{AtomicU64, Ordering},
    },
    time::{Duration, Instant},
};

use ::meshing::{
    BIOME_NEIGHBOUR_SLOT_COUNT, BlockClassifier, CameraMedium, ChunkBiomeTintIdentity, ChunkMesh,
    FaceConnectivity, MeshLightSample, MeshLightSampler, PackedBiomeRecord, biome_neighbour_index,
    chunk_publication_byte_len, mesh_dependency_mask,
    mesh_sub_chunk_in_neighbourhood_with_lighting, sample_camera_medium,
};
use assets::{
    LiveBiomeDefinition, NetworkIdMode, ResolvedBiomeTints, RuntimeAssets, RuntimeEntityAssets,
};
use crossbeam_channel::{Receiver, Sender, bounded};
use hashbrown::HashMap as FastHashMap;
use protocol::{
    ActorAttribute, ActorEvent, BiomeDefinitionEvent, BlockCrackEvent, BlockEntityUpdateEvent,
    BlockUpdateEvent, ChangeDimensionEvent, DaylightCycleUpdateEvent, DimensionRange,
    LevelChunkEvent, LevelChunkMode, MovePlayerEvent, Packet, PlayerMovementCorrectionEvent,
    RespawnEvent, SetTimeEvent, SubChunkBatchEvent, SubChunkReplyAdmissionEvent, SubChunkResult,
    UiEvent, WeatherUpdateEvent, WorldBootstrap, WorldEvent, request_sub_chunk_column,
    vanilla_dimension_range,
};
use thiserror::Error;
use world::{
    BiomeStorage, BlockEntityError, BlockEntityKey, BlockEntityNbt, BlockPos, BlockUpdate,
    BoundaryLightSample, ChunkKey, ChunkStore, DecodeError, DecodedBiomeColumn,
    DecodedBlockEntities, DecodedLevelChunk, DecodedSubChunk, DimensionLightProfile,
    LightBlockAccess, LightBlockSample, LightBounds, LightChannel,
    LightProperties as SolverLightProperties, LightReadAccess, LightSolveError, LightSolveOutput,
    LightStore, LightStoreSnapshot, LightSubChunkKind, MeshDependencyMask, MeshNeighbourhood,
    MutationError, PreparedSubChunkMutation, SolverLimits, SubChunk, SubChunkKey, SubChunkLight,
    chunk_in_view, solve_light,
};

use super::actor_animation::{ActorAnimationStats, ActorRigSnapshot};
use super::actor_store::{ActorSnapshot, ActorStore, PlayerProfile};
use super::block_entity_visuals::{
    BackingBlockIdentity, BlockEntityVisualDiagnostics, adjudicate_block_entity_visual,
};
use super::server_position::{ResolvedServerPosition, resolve_server_position};
use super::{ActorEquipmentSnapshot, RemoteActionSnapshot, RemoteActionStats};

mod block_entities;
mod cohort;
mod connectivity;
mod construction;
mod decode;
mod diagnostics;
mod dirty;
mod helpers;
mod lighting;
mod meshing;
mod model;
mod polling;
mod publication;
#[path = "publication_config.rs"]
mod publication_config;
#[cfg(feature = "publication-test-support")]
mod publication_test_support;
mod request_queue;
mod requests;
mod residency;
mod retries;
mod sequencing;

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
pub use publication_config::{
    PublicationAllowance, PublicationPermit, PublicationPermitStage, PublicationServiceConfig,
};
#[cfg(feature = "publication-test-support")]
pub use publication_test_support::{PublicationFixtureIdentity, PublicationFixtureSnapshot};

/// Decode and mesh workers may each have at most this many completed results
/// waiting for the main thread. A full channel applies backpressure to Rayon.
pub const WORK_RESULT_CAPACITY: usize = 512;
pub const MAX_ADMITTED_WORLD_EVENTS: usize = 64;
pub const MAX_ADMITTED_HEAVY_EVENTS: usize = 32;
pub const MAX_IN_FLIGHT_DECODE_JOBS: usize = MAX_ADMITTED_HEAVY_EVENTS;
pub const DECODE_DISPATCH_BUDGET_PER_POLL: usize = MAX_ADMITTED_HEAVY_EVENTS;
pub const PHASE0_MAX_VIEW_RADIUS_CHUNKS: i32 = 16;
static NEXT_BIOME_TINT_STREAM_ID: AtomicU64 = AtomicU64::new(1);
static NEXT_ACTOR_SESSION_ID: AtomicU64 = AtomicU64::new(1);
pub const COMMITTED_CONTROL_CAPACITY: usize = MAX_ADMITTED_WORLD_EVENTS;
pub const COMMITTED_UI_CAPACITY: usize = MAX_ADMITTED_WORLD_EVENTS;
pub const OUTBOUND_REQUEST_CAPACITY: usize = 64;
pub const DEFERRED_RETRY_CAPACITY: usize = 64;
pub const MAX_SUB_CHUNK_RETRIES: u8 = 2;
pub const SUB_CHUNK_RESPONSE_TIMEOUT: Duration = Duration::from_secs(2);
pub const MAX_PENDING_MESH_CHANGES: usize = 512;
const MAX_PENDING_SCHEDULER_SCANS_PER_POLL: usize = 128;
const MAX_PENDING_MESH_QUEUE_WORK_PER_POLL: usize = MAX_PENDING_MESH_CHANGES;
pub const MAX_IN_FLIGHT_LIGHT_JOBS: usize = 32;
const MIN_EFFECTIVE_LIGHT_JOB_CAP: usize = 2;
const MAX_LIGHT_COLUMN_BATCH_SUB_CHUNKS: usize = 32;
const INITIAL_LIGHT_BACKLOG_THRESHOLD: usize = 256;
fn light_job_cap_for_threads(worker_threads: usize) -> usize {
    MAX_IN_FLIGHT_LIGHT_JOBS.min(
        worker_threads
            .saturating_div(4)
            .max(MIN_EFFECTIVE_LIGHT_JOB_CAP),
    )
}
fn effective_light_job_cap() -> usize {
    // Lighting is the largest initial-world workload. Leave most of the
    // shared Rayon workers available for meshing, asset work, and the
    // render-side background jobs instead of monopolising the pool with
    // column solves. Two concurrent batches are the minimum because
    // dependency invalidation can make one completion stale while adjacent
    // work still needs to make progress.
    light_job_cap_for_threads(rayon::current_num_threads())
}
fn initial_light_job_cap() -> usize {
    // Initial joins cannot mesh most resident sub-chunks until their light
    // columns complete. Use half of the shared pool for those column solves;
    // on SMT CPUs this fills the physical cores while retaining the sibling
    // workers for newly-ready meshes and render-side jobs. Return to the
    // conservative quarter-pool cap once the initial dependency wall drains.
    MAX_IN_FLIGHT_LIGHT_JOBS.min(
        rayon::current_num_threads()
            .saturating_div(2)
            .max(MIN_EFFECTIVE_LIGHT_JOB_CAP),
    )
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
    key: SubChunkKey,
    revision: u64,
    urgent: bool,
}

impl PendingSchedulerCandidate {
    fn new(key: SubChunkKey, revision: u64, camera_position: [f32; 3], urgent: bool) -> Self {
        Self {
            distance_squared: distance_squared(key, camera_position),
            key,
            revision,
            urgent,
        }
    }
}

impl PartialEq for PendingSchedulerCandidate {
    fn eq(&self, other: &Self) -> bool {
        self.urgent == other.urgent
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
        self.urgent.cmp(&other.urgent).then_with(|| {
            other
                .distance_squared
                .total_cmp(&self.distance_squared)
                .then_with(|| other.key.cmp(&self.key))
                .then_with(|| other.revision.cmp(&self.revision))
        })
    }
}

fn scheduler_camera_cell(camera_position: [f32; 3]) -> [i32; 3] {
    camera_position.map(|value| floor_to_i32(value).div_euclid(16))
}

use model::{
    BlockMutationBatch, CorrelatedSubChunkAttempts, DecodeCompletion, DecodeJob, MeshCompletion,
    NormalizationErrorReason, OutboundRequestSlot, PendingMesh, PendingSubChunk,
    PendingSubChunkColumn, PreparedSubChunk, PreparedSubChunkResult, PreparedWorldEvent,
    QueuedDecodeJob, RetrySchedule, RevisionTracker, SequenceBuffer, SequenceError, queue_wait,
    split_block_update,
};

pub use model::{
    CommittedControlEvent, CommittedUiEvent, ForcedRemeshManifest, ForcedRemeshManifestState,
    PendingSubChunkRequest, PublisherViewGeometry, ViewCohort, ViewCohortStatus, WorldMeshChange,
    WorldStreamError, WorldStreamFatalError, WorldStreamNormalizationStats, WorldStreamPoll,
    WorldStreamStats,
};

/// Ordered Bedrock world ingestion and bounded background meshing.
pub struct WorldStream {
    store: ChunkStore,
    block_entity_visuals: BlockEntityVisualDiagnostics,
    actors: ActorStore,
    actor_session_id: u64,
    classifier: BlockClassifier,
    network_id_mode: NetworkIdMode,
    runtime_assets: Arc<RuntimeAssets>,
    biome_definitions: Arc<[BiomeDefinitionEvent]>,
    resolved_biome_tints: Arc<ResolvedBiomeTints>,
    biome_tint_stream_id: u64,
    biome_tint_revision: u64,
    current_dimension: i32,
    local_player_runtime_id: u64,
    local_player_unique_id: i64,
    ordered: SequenceBuffer<PreparedWorldEvent>,
    submitted: HashSet<u64>,
    heavy_sequences: HashSet<u64>,
    pending_decode: VecDeque<QueuedDecodeJob>,
    in_flight_decode_jobs: usize,
    blocking_block_updates: Option<u64>,
    decode_tx: Sender<DecodeCompletion>,
    decode_rx: Receiver<DecodeCompletion>,
    light_tx: Sender<LightCompletion>,
    light_rx: Receiver<LightCompletion>,
    mesh_tx: Sender<MeshCompletion>,
    mesh_rx: Receiver<MeshCompletion>,
    next_block_generation: u64,
    block_generations: HashMap<SubChunkKey, u64>,
    light_store: LightStore,
    light_ownership: HashMap<SubChunkKey, LightOwnership>,
    direct_sky: BTreeMap<SubChunkKey, StoredDirectSky>,
    light_failures: HashMap<SubChunkKey, LightFailure>,
    light_revisions: RevisionTracker,
    pending_light: HashMap<SubChunkKey, PendingLight>,
    pending_light_scan: VecDeque<(SubChunkKey, u64)>,
    pending_light_ready: BinaryHeap<PendingSchedulerCandidate>,
    pending_light_deferred: BinaryHeap<PendingSchedulerCandidate>,
    light_priority_wakeups: HashMap<SubChunkKey, u64>,
    light_scheduler_camera_cell: Option<[i32; 3]>,
    in_flight_light: HashMap<SubChunkKey, LightJobIdentity>,
    next_light_batch_id: u64,
    in_flight_light_batches: HashMap<u64, usize>,
    last_dispatched_light_batch: HashMap<SubChunkKey, u64>,
    light_waiters: HashMap<SubChunkKey, BTreeSet<SubChunkKey>>,
    fatal_light_failure: bool,
    fatal_error: Option<WorldStreamFatalError>,
    revisions: RevisionTracker,
    applied_mesh_generations: HashMap<SubChunkKey, u64>,
    mesh_dependency_masks: HashMap<SubChunkKey, (u64, MeshDependencyMask)>,
    pending_mesh: HashMap<SubChunkKey, PendingMesh>,
    pending_mesh_scan: VecDeque<(SubChunkKey, u64)>,
    pending_resident_mesh_deferred: BinaryHeap<PendingSchedulerCandidate>,
    pending_resident_mesh_ready: BinaryHeap<PendingSchedulerCandidate>,
    pending_mesh_removal_deferred: BinaryHeap<PendingSchedulerCandidate>,
    pending_mesh_removal_ready: BinaryHeap<PendingSchedulerCandidate>,
    mesh_scheduler_camera_cell: Option<[i32; 3]>,
    in_flight: HashMap<SubChunkKey, u64>,
    urgent_mesh_in_flight: HashSet<SubChunkKey>,
    resident: BTreeSet<SubChunkKey>,
    known_air: BTreeSet<SubChunkKey>,
    loaded_columns: BTreeSet<ChunkKey>,
    requested_sub_chunks: HashMap<ChunkKey, PendingSubChunkColumn>,
    request_collision_failures: HashSet<ChunkKey>,
    sub_chunk_deadlines: BTreeSet<(Instant, SubChunkKey)>,
    correlated_sub_chunk_attempts: HashMap<SubChunkKey, CorrelatedSubChunkAttempts>,
    admitted_sub_chunk_replies: HashMap<SubChunkKey, u8>,
    deferred_retries: VecDeque<SubChunkKey>,
    deferred_retry_set: HashSet<SubChunkKey>,
    deferred_recovery_requests: VecDeque<PendingSubChunkRequest>,
    connectivity: FastHashMap<SubChunkKey, FaceConnectivity>,
    connectivity_generation: u64,
    requests: RequestQueue,
    transport_pending_requests: usize,
    last_request_player_chunk: Option<ChunkKey>,
    publication_allowance: Option<PublicationAllowance>,
    mesh_changes: VecDeque<WorldMeshChange>,
    committed_controls: VecDeque<CommittedControlEvent>,
    committed_ui: VecDeque<CommittedUiEvent>,
    local_movement_speed: Option<f64>,
    publisher_center: Option<[i32; 3]>,
    publisher_radius_blocks: Option<u32>,
    publisher_radius_chunks: Option<i32>,
    committed_view_cohort: Option<ViewCohort>,
    provisional_publisher_rebase: bool,
    local_resets_armed: u64,
    local_resets_consumed: u64,
    local_reset_dispatch_count: u8,
    local_reset_dispatch_total: u64,
    local_reset_dispatch_active: bool,
    local_reset_dispatch_classes: [Option<RequestClass>; MAX_LOCAL_RESET_DISPATCH_EVIDENCE],
    publisher_epoch: u64,
    required_columns: BTreeSet<ChunkKey>,
    source_columns: BTreeSet<ChunkKey>,
    source_capture_sequence: Option<u64>,
    chunk_radius: Option<i32>,
    last_retention_center: Option<ChunkKey>,
    last_retention_radius: Option<i32>,
    resolved_server_position: ResolvedServerPosition,
    latest_movement_correction_tick: Option<u64>,
    stats: WorldStreamStats,
}

#[cfg(test)]
mod tests;
