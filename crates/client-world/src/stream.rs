use std::{
    collections::{BTreeMap, BTreeSet, HashMap, HashSet, VecDeque},
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
use protocol::{
    ActorAttribute, ActorEvent, BiomeDefinitionEvent, BlockCrackEvent, BlockEntityUpdateEvent,
    BlockUpdateEvent, ChangeDimensionEvent, DaylightCycleUpdateEvent, LevelChunkEvent,
    LevelChunkMode, MovePlayerEvent, Packet, PlayerMovementCorrectionEvent, SetTimeEvent,
    SubChunkBatchEvent, SubChunkResult, UiEvent, WeatherUpdateEvent, WorldBootstrap, WorldEvent,
    request_sub_chunk_column, vanilla_dimension_range,
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
    solve_light,
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
mod requests;
mod residency;
mod retries;
mod sequencing;

use helpers::*;
use lighting::types::*;
use meshing::types::*;

pub use diagnostics::{
    BuildProfileIdentity, CohortManifestIdentity, Phase2PresentationSnapshot,
    Phase2PublicationSnapshot, PresentModeIdentity, PublicationStageCounters, RequestClass,
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
pub const MAX_IN_FLIGHT_LIGHT_JOBS: usize = 512;
pub const LIGHT_DISPATCH_BUDGET_PER_POLL: usize = MAX_IN_FLIGHT_LIGHT_JOBS;
const LIGHT_RESULT_CAPACITY: usize = LIGHT_DISPATCH_BUDGET_PER_POLL;
const LIGHT_SOLVE_LIMITS: SolverLimits = SolverLimits::new(4_096, 1_000_000);

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
    in_flight_light: HashMap<SubChunkKey, LightJobIdentity>,
    light_waiters: HashMap<SubChunkKey, BTreeSet<SubChunkKey>>,
    fatal_light_failure: bool,
    fatal_error: Option<WorldStreamFatalError>,
    revisions: RevisionTracker,
    applied_mesh_generations: HashMap<SubChunkKey, u64>,
    mesh_dependency_masks: HashMap<SubChunkKey, (u64, MeshDependencyMask)>,
    pending_mesh: HashMap<SubChunkKey, PendingMesh>,
    in_flight: HashMap<SubChunkKey, u64>,
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
    connectivity: HashMap<SubChunkKey, FaceConnectivity>,
    connectivity_generation: u64,
    requests: VecDeque<OutboundRequestSlot>,
    transport_pending_requests: usize,
    publication_allowance: Option<PublicationAllowance>,
    mesh_changes: VecDeque<WorldMeshChange>,
    committed_controls: VecDeque<CommittedControlEvent>,
    committed_ui: VecDeque<CommittedUiEvent>,
    publisher_center: Option<[i32; 3]>,
    publisher_radius_blocks: Option<u32>,
    publisher_radius_chunks: Option<i32>,
    committed_view_cohort: Option<ViewCohort>,
    publisher_epoch: u64,
    required_columns: BTreeSet<ChunkKey>,
    source_columns: BTreeSet<ChunkKey>,
    source_capture_sequence: Option<u64>,
    chunk_radius: Option<i32>,
    resolved_server_position: ResolvedServerPosition,
    latest_movement_correction_tick: Option<u64>,
    stats: WorldStreamStats,
}

#[cfg(test)]
mod tests;
