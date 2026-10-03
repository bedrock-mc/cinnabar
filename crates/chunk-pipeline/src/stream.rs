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
use client_world::ingestion::{
    ActorHandedness, BiomeDefinitionEvent, BlockCrackEvent, BlockUpdateEvent, DimensionRange,
    LevelChunkEvent, LevelChunkMode, Packet, SubChunkBatchEvent, SubChunkReplyAdmissionEvent,
    WorldBootstrap, WorldEvent, request_sub_chunk_column, vanilla_dimension_range,
};
use crossbeam_channel::{Receiver, Sender, bounded};
use hashbrown::HashMap as FastHashMap;
use thiserror::Error;
use world::{
    BiomeStorage, BlockEntityKey, BlockIds, BlockPos, BlockUpdate, BoundaryLightSample, ChunkKey,
    ChunkStore, DecodeError, DecodedBiomeColumn, DecodedBlockEntities, DimensionLightProfile,
    LightBlockAccess, LightBlockSample, LightBounds, LightChannel,
    LightProperties as SolverLightProperties, LightReadAccess, LightSolveError, LightSolveOutput,
    LightStore, LightStoreSnapshot, LightSubChunkKind, MeshDependencyMask, MeshNeighbourhood,
    PreparedSubChunkMutation, SolverLimits, SubChunk, SubChunkKey, SubChunkLight, chunk_in_view,
    solve_light,
};

use super::{ActorArmorSnapshot, ActorEquipmentSnapshot, RemoteActionSnapshot, RemoteActionStats};
use client_world::ResolvedServerPosition;
use client_world::{ActorAnimationStats, ActorRigSnapshot};
use client_world::{ActorSnapshot, LocalPlayerFeed, PlayerProfile};
use client_world::{
    BackingBlockIdentity, BlockEntityVisualDiagnostics, adjudicate_block_entity_visual,
};

mod block_cracks;
mod block_entities;
mod block_events;
mod cave_visibility;
mod cohort;
mod commit_budget;
mod connectivity;
mod construction;
mod decode;
mod diagnostics;
mod dirty;
mod helpers;
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
mod scheduler_refresh;
mod sequencing;
mod sign_edit;
mod workers;

pub use client_world::ingestion::WorldStreamError;
use client_world::ingestion::{
    BlockMutationBatch, CommitStep, DecodeCommit, DecodeCompletion, DecodeIds, DecodeJob,
    PreparedSubChunkResult, PreparedWorldEvent, QueuedDecodeJob, dimension_slots,
};
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
pub const PHASE0_MAX_VIEW_RADIUS_CHUNKS: i32 = 16;
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
fn light_job_cap_for_threads(worker_threads: usize) -> usize {
    MAX_IN_FLIGHT_LIGHT_JOBS.min(
        worker_threads
            .saturating_div(4)
            .max(MIN_EFFECTIVE_LIGHT_JOB_CAP),
    )
}
fn effective_light_job_cap() -> usize {
    // Quiet relighting uses fewer admissions; its workers have a separate queue.
    light_job_cap_for_threads(rayon::current_num_threads())
}
fn initial_light_job_cap() -> usize {
    // Initial lighting fills a larger bounded wave because it gates ready geometry.
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
    fn new(key: SubChunkKey, revision: u64, view: SchedulerView, urgent: bool) -> Self {
        Self {
            distance_squared: view.rank(key),
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

/// Camera the work schedulers order by: nearest first, sub-chunks in front of the view ahead of
/// those behind it, as vanilla queues render-chunk builds from the visible set.
#[derive(Debug, Clone, Copy)]
struct SchedulerView {
    position: [f32; 3],
    forward: Option<[f32; 3]>,
}

impl SchedulerView {
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
    CorrelatedSubChunkAttempts, MeshCompletion, NormalizationErrorReason, OutboundRequestSlot,
    PendingMesh, PendingSubChunk, PendingSubChunkColumn, RetrySchedule, RevisionTracker,
    queue_wait, split_block_update,
};

pub use block_cracks::{
    ActiveBlockCrack, BlockCrackSnapshot, BlockCrackStatus, MAX_ACTIVE_BLOCK_CRACKS,
};
pub use block_events::BlockEventCue;
pub use map_data::MapImage;
pub use model::{
    ForcedRemeshManifest, ForcedRemeshManifestState, PendingSubChunkRequest, ViewCohortStatus,
    WorldMeshChange, WorldStreamFatalError, WorldStreamNormalizationStats, WorldStreamPoll,
    WorldStreamStats,
};
pub use sign_edit::SignEditRequest;

/// Ordered Bedrock world ingestion and bounded background meshing.
pub struct WorldStream {
    authority: client_world::WorldAuthority,
    order: client_world::ingestion::OrderedCommitState,
    block_cracks: block_cracks::BlockCracks,
    block_entity_visuals: BlockEntityVisualDiagnostics,
    classifier: BlockClassifier,
    /// Whether the server sent terrain before spawn; when it did not, startup
    /// has no view to wait for until the server publishes one.
    startup_terrain_announced: bool,
    pending_decode: VecDeque<QueuedDecodeJob>,
    in_flight_decode_jobs: usize,
    predictions: prediction::DeferredPredictions,
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
    light_scheduler_refresh: scheduler_refresh::SchedulerRefresh<2>,
    in_flight_light: HashMap<SubChunkKey, LightJobIdentity>,
    next_light_batch_id: u64,
    in_flight_light_batches: HashMap<u64, usize>,
    /// Solves still executing, including ones whose keys were evicted meanwhile.
    running_light_jobs: Arc<AtomicUsize>,
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
    mesh_scheduler_refresh: scheduler_refresh::SchedulerRefresh<4>,
    /// Unit view direction the schedulers favour; `None` orders by distance alone.
    view_forward: Option<[f32; 3]>,
    in_flight: HashMap<SubChunkKey, u64>,
    admitted_mesh_jobs: Arc<AtomicUsize>,
    mesh_memory: meshing::memory::MeshMemoryBudget,
    mesh_cancellations: HashMap<SubChunkKey, Arc<AtomicBool>>,
    urgent_mesh_in_flight: HashSet<SubChunkKey>,
    staged_mesh_completions: VecDeque<MeshCompletion>,
    staged_mesh_bytes: u64,
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
    unsent_column_deadlines: HashMap<ChunkKey, Instant>,
    arrival_cohort: Option<residency::ArrivalCohort>,
    poll_deadline: Option<Instant>,
    frame_deadline: Option<Instant>,
    polling: bool,
    publication_allowance: Option<PublicationAllowance>,
    mesh_changes: VecDeque<WorldMeshChange>,
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
    stats: WorldStreamStats,
}

#[cfg(test)]
mod tests;

pub use client_world::{
    COMMITTED_AUDIO_CAPACITY, COMMITTED_CAMERA_CAPACITY, COMMITTED_CONTROL_CAPACITY,
};

pub use client_world::{
    CommittedAudioEvent, CommittedCameraEvent, CommittedControlEvent, CommittedParticleEvent,
    CommittedUiEvent, PublisherViewGeometry, ViewCohort,
};
