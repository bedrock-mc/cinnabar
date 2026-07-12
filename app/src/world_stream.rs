use std::{
    collections::{BTreeMap, BTreeSet, HashMap, HashSet, VecDeque},
    sync::{
        Arc,
        atomic::{AtomicU64, Ordering},
    },
    time::{Duration, Instant},
};

use assets::{LiveBiomeDefinition, NetworkIdMode, ResolvedBiomeTints, RuntimeAssets};
use bevy::prelude::Resource;
use crossbeam_channel::{Receiver, Sender, bounded};
use protocol::{
    BiomeDefinitionEvent, BlockUpdateEvent, ChangeDimensionEvent, LevelChunkEvent, LevelChunkMode,
    MovePlayerEvent, Packet, SubChunkBatchEvent, SubChunkResult, WorldBootstrap, WorldEvent,
    request_sub_chunk_column, vanilla_dimension_range,
};
use render::{
    BlockClassifier, ChunkBiomeTintIdentity, ChunkMesh, FaceConnectivity, PackedBiomeRecord,
    mesh_dependency_mask, mesh_sub_chunk_in_neighbourhood,
};
use thiserror::Error;
use world::{
    BiomeStorage, BlockUpdate, ChunkKey, ChunkStore, DecodeError, DecodedBiomeColumn,
    DecodedLevelChunk, MeshDependencyMask, MeshNeighbourhood, MutationError,
    PreparedSubChunkMutation, SubChunk, SubChunkKey,
};

use crate::server_position::{ResolvedServerPosition, resolve_server_position};

/// Decode and mesh workers may each have at most this many completed results
/// waiting for the main thread. A full channel applies backpressure to Rayon.
pub const WORK_RESULT_CAPACITY: usize = 128;
pub const MAX_ADMITTED_WORLD_EVENTS: usize = 64;
pub const MAX_ADMITTED_HEAVY_EVENTS: usize = 32;
pub const MAX_IN_FLIGHT_DECODE_JOBS: usize = 4;
pub const DECODE_DISPATCH_BUDGET_PER_POLL: usize = 4;
pub const PHASE0_MAX_VIEW_RADIUS_CHUNKS: i32 = 16;
static NEXT_BIOME_TINT_STREAM_ID: AtomicU64 = AtomicU64::new(1);
pub const COMMITTED_CONTROL_CAPACITY: usize = MAX_ADMITTED_WORLD_EVENTS;
pub const OUTBOUND_REQUEST_CAPACITY: usize = 64;
pub const DEFERRED_RETRY_CAPACITY: usize = 64;
pub const MAX_SUB_CHUNK_RETRIES: u8 = 2;
pub const SUB_CHUNK_RESPONSE_TIMEOUT: Duration = Duration::from_secs(2);
pub const MAX_PENDING_MESH_CHANGES: usize = 256;

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
struct PendingSubChunk {
    retry_attempts: u8,
    pending_transport_attempts: u8,
    confirmed_attempts: u8,
    response_deadline: Option<Instant>,
}

type PendingSubChunkColumn = BTreeMap<i32, PendingSubChunk>;

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
struct CorrelatedSubChunkAttempts {
    pending_transport_attempts: u8,
    confirmed_attempts: u8,
}

/// One exact horizontal publisher view, expressed in chunk columns.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ViewCohort {
    pub dimension: i32,
    pub center: [i32; 2],
    pub radius: i32,
}

impl ViewCohort {
    #[must_use]
    pub fn from_publisher(dimension: i32, center: [i32; 3], radius_blocks: u32) -> Self {
        let chunks = radius_blocks.saturating_add(15) / 16;
        Self {
            dimension,
            center: [center[0].div_euclid(16), center[2].div_euclid(16)],
            radius: i32::try_from(chunks).unwrap_or(i32::MAX),
        }
    }

    #[must_use]
    pub fn contains_column(self, dimension: i32, column: [i32; 2]) -> bool {
        dimension == self.dimension
            && i64::from(column[0]).abs_diff(i64::from(self.center[0])) <= self.radius.max(0) as u64
            && i64::from(column[1]).abs_diff(i64::from(self.center[1])) <= self.radius.max(0) as u64
    }

    #[must_use]
    pub fn expected_columns(self) -> BTreeSet<ChunkKey> {
        let radius = self.radius.max(0);
        (-radius..=radius)
            .flat_map(|x_offset| {
                (-radius..=radius).map(move |z_offset| {
                    ChunkKey::new(
                        self.dimension,
                        self.center[0].saturating_add(x_offset),
                        self.center[1].saturating_add(z_offset),
                    )
                })
            })
            .collect()
    }
}

/// Deterministic evidence that the committed world state matches one view.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ViewCohortStatus {
    pub target: ViewCohort,
    pub committed: Option<ViewCohort>,
    pub expected: usize,
    pub loaded_target: usize,
    pub missing_target: usize,
    pub foreign_loaded: usize,
    pub foreign_requested: usize,
    pub foreign_resident: usize,
    pub source_leftover: usize,
    pub resident_count: usize,
    pub resident_hash: u64,
    pub known_air_count: usize,
    pub known_air_hash: u64,
}

impl ViewCohortStatus {
    #[must_use]
    pub fn is_exact(self) -> bool {
        self.committed == Some(self.target)
            && self.loaded_target == self.expected
            && self.missing_target == 0
            && self.foreign_loaded == 0
            && self.foreign_requested == 0
            && self.foreign_resident == 0
            && self.source_leftover == 0
    }
}

#[derive(Debug)]
struct SequenceBuffer<T> {
    next: u64,
    ready: BTreeMap<u64, T>,
}

impl<T> SequenceBuffer<T> {
    fn new(first_sequence: u64) -> Self {
        Self {
            next: first_sequence,
            ready: BTreeMap::new(),
        }
    }

    fn insert(&mut self, sequence: u64, value: T) -> Result<(), SequenceError> {
        if sequence < self.next || self.ready.contains_key(&sequence) {
            return Err(SequenceError::DuplicateOrPast {
                sequence,
                next: self.next,
            });
        }
        self.ready.insert(sequence, value);
        Ok(())
    }

    fn pop_next(&mut self) -> Option<T> {
        let value = self.ready.remove(&self.next)?;
        self.next = self.next.saturating_add(1);
        Some(value)
    }

    const fn next_sequence(&self) -> u64 {
        self.next
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Error)]
enum SequenceError {
    #[error("world sequence {sequence} is duplicate or older than next sequence {next}")]
    DuplicateOrPast { sequence: u64, next: u64 },
}

#[derive(Debug, Clone, Copy)]
struct DirtyRevision {
    revision: u64,
    since: Instant,
}

#[derive(Debug, Default)]
struct RevisionTracker {
    entries: HashMap<SubChunkKey, DirtyRevision>,
    next_revision: u64,
}

impl RevisionTracker {
    fn mark_dirty(&mut self, key: SubChunkKey, now: Instant) -> u64 {
        self.next_revision = self.next_revision.wrapping_add(1).max(1);
        let revision = self.next_revision;
        let entry = self.entries.entry(key).or_insert(DirtyRevision {
            revision,
            since: now,
        });
        entry.revision = revision;
        entry.revision
    }

    fn is_current(&self, key: SubChunkKey, revision: u64) -> bool {
        self.entries
            .get(&key)
            .is_some_and(|entry| entry.revision == revision)
    }

    fn force_dirty_since(&mut self, key: SubChunkKey, now: Instant) -> u64 {
        self.next_revision = self.next_revision.wrapping_add(1).max(1);
        let revision = self.next_revision;
        self.entries.insert(
            key,
            DirtyRevision {
                revision,
                since: now,
            },
        );
        revision
    }

    fn dirty(&self, key: SubChunkKey) -> Option<DirtyRevision> {
        self.entries.get(&key).copied()
    }

    fn clear_if_current(&mut self, key: SubChunkKey, revision: u64) {
        if self.is_current(key, revision) {
            self.entries.remove(&key);
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Error)]
enum BlockUpdateConversionError {
    #[error("block update layer {0} cannot fit the world mutation layer type")]
    LayerOverflow(usize),
}

fn split_block_update(
    event: BlockUpdateEvent,
) -> Result<(SubChunkKey, BlockUpdate), BlockUpdateConversionError> {
    let [x, y, z] = event.position;
    let layer = u32::try_from(event.layer)
        .map_err(|_| BlockUpdateConversionError::LayerOverflow(event.layer))?;
    Ok((
        SubChunkKey::new(
            event.dimension,
            x.div_euclid(16),
            y.div_euclid(16),
            z.div_euclid(16),
        ),
        BlockUpdate::new(
            x.rem_euclid(16) as u8,
            y.rem_euclid(16) as u8,
            z.rem_euclid(16) as u8,
            layer,
            event.network_id,
        ),
    ))
}

/// One SubChunkRequest packet plus the normalized range used to build it.
/// The metadata lets the app and tests inspect the request without depending
/// on generated protocol packet internals.
pub struct PendingSubChunkRequest {
    pub packet: Packet,
    pub dimension: i32,
    pub chunk: ChunkKey,
    pub base_sub_chunk_y: i32,
    pub count: usize,
}

enum OutboundRequestSlot {
    Reserved(u64),
    Ready(PendingSubChunkRequest),
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum RetrySchedule {
    Scheduled,
    CapacityFull,
    EncodingFailure,
}

/// A current packed mesh update, or removal, ready for `ChunkRenderQueue`.
#[derive(Debug)]
pub enum WorldMeshChange {
    Upsert {
        key: SubChunkKey,
        mesh: ChunkMesh,
        biome: PackedBiomeRecord,
        tint_identity: ChunkBiomeTintIdentity,
        generation: u64,
        dirty_since: Instant,
    },
    Remove {
        key: SubChunkKey,
        generation: u64,
        dirty_since: Instant,
    },
}

/// Exact generations dirtied together for the forced full-view remesh gate.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ForcedRemeshManifest {
    pub started_at: Instant,
    pub entries: Arc<[(SubChunkKey, u64)]>,
}

impl ForcedRemeshManifest {
    #[must_use]
    pub fn is_empty(&self) -> bool {
        self.entries.is_empty()
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ForcedRemeshManifestState {
    Pending,
    Complete,
    Invalid,
}

#[derive(Debug, Clone, Copy, PartialEq)]
pub enum CommittedControlEvent {
    MovePlayer {
        sequence: u64,
        movement: MovePlayerEvent,
        resolved: ResolvedServerPosition,
        source_cohort: Option<ViewCohort>,
    },
    ChangeDimension {
        change: ChangeDimensionEvent,
        resolved: ResolvedServerPosition,
    },
}

#[cfg(test)]
impl WorldMeshChange {
    #[must_use]
    pub const fn key(&self) -> SubChunkKey {
        match self {
            Self::Upsert { key, .. } | Self::Remove { key, .. } => *key,
        }
    }
}

/// Cumulative reasons behind [`WorldStreamStats::normalization_errors`].
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct WorldStreamNormalizationStats {
    pub ordered_completion_rejections: u64,
    pub inactive_block_updates: u64,
    pub malformed_block_updates: u64,
    pub inactive_inline_chunks: u64,
    pub inactive_sub_chunks: u64,
    pub unexpected_sub_chunks: u64,
    pub invalid_dimension_sub_chunks: u64,
    pub block_mutation_failures: u64,
    pub empty_sub_chunk_batches: u64,
    pub invalid_chunk_radii: u64,
    pub inactive_level_chunks: u64,
    pub unsupported_level_chunk_dimensions: u64,
    pub outbound_request_placement_failures: u64,
    pub request_encoding_failures: u64,
    pub deferred_retry_capacity_failures: u64,
    pub retry_request_encoding_failures: u64,
    pub biome_definition_resolution_failures: u64,
    pub biome_tint_revision_overflows: u64,
}

impl WorldStreamNormalizationStats {
    #[must_use]
    pub fn total(self) -> u64 {
        [
            self.ordered_completion_rejections,
            self.inactive_block_updates,
            self.malformed_block_updates,
            self.inactive_inline_chunks,
            self.inactive_sub_chunks,
            self.unexpected_sub_chunks,
            self.invalid_dimension_sub_chunks,
            self.block_mutation_failures,
            self.empty_sub_chunk_batches,
            self.invalid_chunk_radii,
            self.inactive_level_chunks,
            self.unsupported_level_chunk_dimensions,
            self.outbound_request_placement_failures,
            self.request_encoding_failures,
            self.deferred_retry_capacity_failures,
            self.retry_request_encoding_failures,
            self.biome_definition_resolution_failures,
            self.biome_tint_revision_overflows,
        ]
        .into_iter()
        .fold(0, u64::saturating_add)
    }
}

#[derive(Debug, Clone, Copy)]
enum NormalizationErrorReason {
    OrderedCompletionRejection,
    InactiveBlockUpdate,
    MalformedBlockUpdate,
    InactiveInlineChunk,
    UnexpectedSubChunk,
    InvalidDimensionSubChunk,
    BlockMutationFailure,
    EmptySubChunkBatch,
    InvalidChunkRadius,
    InactiveLevelChunk,
    UnsupportedLevelChunkDimension,
    OutboundRequestPlacementFailure,
    RequestEncodingFailure,
    DeferredRetryCapacityFailure,
    RetryRequestEncodingFailure,
    BiomeDefinitionResolutionFailure,
    BiomeTintRevisionOverflow,
}

impl WorldStreamNormalizationStats {
    fn record(&mut self, reason: NormalizationErrorReason) {
        let counter = match reason {
            NormalizationErrorReason::OrderedCompletionRejection => {
                &mut self.ordered_completion_rejections
            }
            NormalizationErrorReason::InactiveBlockUpdate => &mut self.inactive_block_updates,
            NormalizationErrorReason::MalformedBlockUpdate => &mut self.malformed_block_updates,
            NormalizationErrorReason::InactiveInlineChunk => &mut self.inactive_inline_chunks,
            NormalizationErrorReason::UnexpectedSubChunk => &mut self.unexpected_sub_chunks,
            NormalizationErrorReason::InvalidDimensionSubChunk => {
                &mut self.invalid_dimension_sub_chunks
            }
            NormalizationErrorReason::BlockMutationFailure => &mut self.block_mutation_failures,
            NormalizationErrorReason::EmptySubChunkBatch => &mut self.empty_sub_chunk_batches,
            NormalizationErrorReason::InvalidChunkRadius => &mut self.invalid_chunk_radii,
            NormalizationErrorReason::InactiveLevelChunk => &mut self.inactive_level_chunks,
            NormalizationErrorReason::UnsupportedLevelChunkDimension => {
                &mut self.unsupported_level_chunk_dimensions
            }
            NormalizationErrorReason::OutboundRequestPlacementFailure => {
                &mut self.outbound_request_placement_failures
            }
            NormalizationErrorReason::RequestEncodingFailure => &mut self.request_encoding_failures,
            NormalizationErrorReason::DeferredRetryCapacityFailure => {
                &mut self.deferred_retry_capacity_failures
            }
            NormalizationErrorReason::RetryRequestEncodingFailure => {
                &mut self.retry_request_encoding_failures
            }
            NormalizationErrorReason::BiomeDefinitionResolutionFailure => {
                &mut self.biome_definition_resolution_failures
            }
            NormalizationErrorReason::BiomeTintRevisionOverflow => {
                &mut self.biome_tint_revision_overflows
            }
        };
        *counter = counter.saturating_add(1);
    }
}

/// Cumulative diagnostics and current bounded-work gauges.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct WorldStreamStats {
    pub decode_errors: u64,
    pub normalization_errors: u64,
    pub normalization_reasons: WorldStreamNormalizationStats,
    pub unavailable_sub_chunks: u64,
    pub stale_mesh_jobs: u64,
    pub received_radius_chunks: Option<i32>,
    pub publisher_radius_chunks: Option<i32>,
    pub resident_sub_chunks: usize,
    pub pending_mesh_jobs: usize,
    pub in_flight_mesh_jobs: usize,
    pub admitted_world_events: usize,
    pub admitted_heavy_events: usize,
    pub queued_decode_jobs: usize,
    pub in_flight_decode_jobs: usize,
    pub completed_decode_results: usize,
    pub pending_retry_requests: usize,
    pub awaiting_sub_chunk_responses: usize,
    pub sub_chunk_timeouts: u64,
    pub sub_chunk_retries_scheduled: u64,
    pub sub_chunk_retry_exhaustions: u64,
    pub max_decode_duration: Duration,
    pub max_mesh_duration: Duration,
    pub max_remesh_latency: Duration,
    pub last_chunk_commit_at: Option<Instant>,
    pub last_mesh_dispatch_at: Option<Instant>,
    pub last_mesh_completion_at: Option<Instant>,
    pub last_mesh_ack_at: Option<Instant>,
}

/// Work performed by one call to [`WorldStream::poll`].
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct WorldStreamPoll {
    pub decoded_results: usize,
    pub mesh_results: usize,
    pub mesh_jobs_dispatched: usize,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Error)]
pub enum WorldStreamError {
    #[error("world sequence {sequence} is duplicate or older than next sequence {next}")]
    DuplicateOrPast { sequence: u64, next: u64 },
    #[error(
        "world admission is full at sequence {sequence} ({admitted}/{capacity} events, {heavy_admitted}/{heavy_capacity} heavy)"
    )]
    AdmissionFull {
        sequence: u64,
        admitted: usize,
        capacity: usize,
        heavy_admitted: usize,
        heavy_capacity: usize,
    },
    #[error("outbound SubChunkRequest FIFO is full at sequence {sequence} ({pending}/{capacity})")]
    OutboundFull {
        sequence: u64,
        pending: usize,
        capacity: usize,
    },
}

impl From<SequenceError> for WorldStreamError {
    fn from(error: SequenceError) -> Self {
        match error {
            SequenceError::DuplicateOrPast { sequence, next } => {
                Self::DuplicateOrPast { sequence, next }
            }
        }
    }
}

#[derive(Debug)]
enum PreparedWorldEvent {
    InlineLevelChunk {
        event: LevelChunkEvent,
        decoded: Result<DecodedLevelChunk, DecodeError>,
        duration: Duration,
    },
    RequestLevelChunk {
        event: LevelChunkEvent,
        decoded: Result<DecodedBiomeColumn, DecodeError>,
        duration: Duration,
    },
    SubChunks {
        dimension: i32,
        entries: Vec<PreparedSubChunk>,
        duration: Duration,
    },
    BlockUpdates {
        result: Result<Vec<PreparedSubChunkMutation>, MutationError>,
        duration: Duration,
    },
    Immediate(WorldEvent),
    NormalizationFailure,
}

#[derive(Debug)]
struct PreparedSubChunk {
    position: [i32; 3],
    result: PreparedSubChunkResult,
}

#[derive(Debug)]
enum PreparedSubChunkResult {
    Decoded(Result<SubChunk, DecodeError>),
    AllAir,
    Unavailable(protocol::SubChunkUnavailable),
}

#[derive(Debug)]
struct DecodeCompletion {
    sequence: u64,
    event: PreparedWorldEvent,
}

#[derive(Debug)]
enum DecodeJob {
    InlineLevelChunk {
        sequence: u64,
        event: LevelChunkEvent,
        base_sub_chunk_y: i32,
        count: usize,
        biome_storage_count: usize,
    },
    RequestLevelChunk {
        sequence: u64,
        event: LevelChunkEvent,
        biome_base_sub_chunk_y: i32,
        biome_storage_count: usize,
    },
    SubChunks {
        sequence: u64,
        batch: SubChunkBatchEvent,
    },
    BlockUpdates {
        sequence: u64,
        batches: Vec<BlockMutationBatch>,
        air_runtime_id: u32,
    },
}

#[derive(Debug)]
struct BlockMutationBatch {
    key: SubChunkKey,
    previous: Option<Arc<SubChunk>>,
    updates: Vec<BlockUpdate>,
}

#[derive(Debug, Clone, Copy)]
struct PendingMesh {
    revision: u64,
    since: Instant,
}

#[derive(Debug)]
struct MeshCompletion {
    key: SubChunkKey,
    revision: u64,
    source: Arc<SubChunk>,
    biome_source: Option<Arc<BiomeStorage>>,
    biome: PackedBiomeRecord,
    tint_identity: ChunkBiomeTintIdentity,
    mesh: ChunkMesh,
    dependency_mask: MeshDependencyMask,
    duration: Duration,
}

struct MeshSnapshot {
    center: Arc<SubChunk>,
    biome: Option<Arc<BiomeStorage>>,
    adjacent: [Option<Arc<SubChunk>>; 27],
}

fn pack_biome_record(
    storage: Option<&BiomeStorage>,
    resolved: &ResolvedBiomeTints,
) -> PackedBiomeRecord {
    storage.map_or_else(PackedBiomeRecord::fallback, |storage| {
        PackedBiomeRecord::from_storage(storage, |raw_id| resolved.dense_index(raw_id))
    })
}

impl MeshSnapshot {
    fn neighbourhood(&self) -> MeshNeighbourhood<'_> {
        let mut neighbourhood = MeshNeighbourhood::new(&self.center);
        for offset in MeshNeighbourhood::adjacent_offsets() {
            if let Some(sub_chunk) = self.adjacent[mesh_offset_index(offset)].as_deref() {
                let inserted = neighbourhood.insert(offset, sub_chunk);
                debug_assert!(inserted);
            }
        }
        neighbourhood
    }

    fn mesh(
        &self,
        classifier: BlockClassifier,
        runtime_assets: &RuntimeAssets,
        network_id_mode: NetworkIdMode,
    ) -> ChunkMesh {
        mesh_sub_chunk_in_neighbourhood(
            &classifier,
            runtime_assets,
            network_id_mode,
            &self.neighbourhood(),
        )
    }

    fn dependency_mask(
        &self,
        classifier: BlockClassifier,
        runtime_assets: &RuntimeAssets,
        network_id_mode: NetworkIdMode,
    ) -> MeshDependencyMask {
        mesh_dependency_mask(&classifier, runtime_assets, network_id_mode, &self.center)
    }
}

fn mesh_offset_index([x, y, z]: [i8; 3]) -> usize {
    (usize::from((x + 1) as u8) * 3 + usize::from((y + 1) as u8)) * 3 + usize::from((z + 1) as u8)
}

/// Ordered Bedrock world ingestion and bounded background meshing.
///
/// `submit` is intentionally `(sequence, WorldEvent)` rather than coupled to
/// the network module's wrapper type. This keeps protocol normalization on the
/// network thread while preserving its FIFO sequence at the commit boundary.
#[derive(Resource)]
pub struct WorldStream {
    store: ChunkStore,
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
    pending_decode: VecDeque<DecodeJob>,
    in_flight_decode_jobs: usize,
    blocking_block_updates: Option<u64>,
    decode_tx: Sender<DecodeCompletion>,
    decode_rx: Receiver<DecodeCompletion>,
    mesh_tx: Sender<MeshCompletion>,
    mesh_rx: Receiver<MeshCompletion>,
    revisions: RevisionTracker,
    applied_mesh_generations: HashMap<SubChunkKey, u64>,
    mesh_dependency_masks: HashMap<SubChunkKey, (u64, MeshDependencyMask)>,
    pending_mesh: HashMap<SubChunkKey, PendingMesh>,
    in_flight: HashMap<SubChunkKey, u64>,
    resident: BTreeSet<SubChunkKey>,
    known_air: BTreeSet<SubChunkKey>,
    loaded_columns: BTreeSet<ChunkKey>,
    requested_sub_chunks: HashMap<ChunkKey, PendingSubChunkColumn>,
    sub_chunk_deadlines: BTreeSet<(Instant, SubChunkKey)>,
    correlated_sub_chunk_attempts: HashMap<SubChunkKey, CorrelatedSubChunkAttempts>,
    admitted_sub_chunk_replies: HashMap<SubChunkKey, u8>,
    deferred_retries: VecDeque<SubChunkKey>,
    deferred_retry_set: HashSet<SubChunkKey>,
    connectivity: HashMap<SubChunkKey, FaceConnectivity>,
    connectivity_generation: u64,
    requests: VecDeque<OutboundRequestSlot>,
    mesh_changes: VecDeque<WorldMeshChange>,
    committed_controls: VecDeque<CommittedControlEvent>,
    publisher_center: Option<[i32; 3]>,
    publisher_radius_chunks: Option<i32>,
    committed_view_cohort: Option<ViewCohort>,
    source_columns: BTreeSet<ChunkKey>,
    source_capture_sequence: Option<u64>,
    chunk_radius: Option<i32>,
    resolved_server_position: ResolvedServerPosition,
    stats: WorldStreamStats,
}

impl WorldStream {
    #[cfg(test)]
    #[must_use]
    pub fn new(bootstrap: WorldBootstrap) -> Self {
        Self::new_with_assets(
            bootstrap,
            Arc::new(RuntimeAssets::diagnostic()),
            [0.0, crate::server_position::SAFE_SERVER_HEIGHT, 0.0],
            None,
        )
    }

    #[must_use]
    pub fn new_with_assets(
        bootstrap: WorldBootstrap,
        runtime_assets: Arc<RuntimeAssets>,
        current_position: [f32; 3],
        existing_anchor: Option<[i32; 2]>,
    ) -> Self {
        Self::with_first_sequence_and_recovery(
            bootstrap,
            runtime_assets,
            1,
            current_position,
            existing_anchor,
        )
    }

    fn with_first_sequence_and_recovery(
        bootstrap: WorldBootstrap,
        runtime_assets: Arc<RuntimeAssets>,
        first_sequence: u64,
        current_position: [f32; 3],
        existing_anchor: Option<[i32; 2]>,
    ) -> Self {
        let (decode_tx, decode_rx) = bounded(WORK_RESULT_CAPACITY);
        let (mesh_tx, mesh_rx) = bounded(WORK_RESULT_CAPACITY);
        let resolved_server_position =
            resolve_server_position(bootstrap.player_position, current_position, existing_anchor);
        let resolved_biome_tints = Arc::new(
            runtime_assets
                .biome_assets()
                .resolve_live(&[])
                .expect("validated runtime biome assets resolve without live definitions"),
        );
        let biome_tint_stream_id = NEXT_BIOME_TINT_STREAM_ID
            .fetch_update(Ordering::Relaxed, Ordering::Relaxed, |current| {
                current.checked_add(1)
            })
            .expect("biome tint stream identity space exhausted");
        Self {
            store: ChunkStore::new(),
            classifier: BlockClassifier::new(bootstrap.air_network_id),
            network_id_mode: if bootstrap.block_network_ids_are_hashes {
                NetworkIdMode::Hashed
            } else {
                NetworkIdMode::Sequential
            },
            runtime_assets,
            biome_definitions: Arc::from([]),
            resolved_biome_tints,
            biome_tint_stream_id,
            biome_tint_revision: 0,
            current_dimension: bootstrap.dimension,
            local_player_runtime_id: bootstrap.local_player_runtime_id,
            ordered: SequenceBuffer::new(first_sequence),
            submitted: HashSet::new(),
            heavy_sequences: HashSet::new(),
            pending_decode: VecDeque::new(),
            in_flight_decode_jobs: 0,
            blocking_block_updates: None,
            decode_tx,
            decode_rx,
            mesh_tx,
            mesh_rx,
            revisions: RevisionTracker::default(),
            applied_mesh_generations: HashMap::new(),
            mesh_dependency_masks: HashMap::new(),
            pending_mesh: HashMap::new(),
            in_flight: HashMap::new(),
            resident: BTreeSet::new(),
            known_air: BTreeSet::new(),
            loaded_columns: BTreeSet::new(),
            requested_sub_chunks: HashMap::new(),
            sub_chunk_deadlines: BTreeSet::new(),
            correlated_sub_chunk_attempts: HashMap::new(),
            admitted_sub_chunk_replies: HashMap::new(),
            deferred_retries: VecDeque::new(),
            deferred_retry_set: HashSet::new(),
            connectivity: HashMap::new(),
            connectivity_generation: 0,
            requests: VecDeque::new(),
            mesh_changes: VecDeque::new(),
            committed_controls: VecDeque::new(),
            publisher_center: Some([
                floor_to_i32(resolved_server_position.position[0]),
                floor_to_i32(resolved_server_position.position[1]),
                floor_to_i32(resolved_server_position.position[2]),
            ]),
            publisher_radius_chunks: None,
            committed_view_cohort: None,
            source_columns: BTreeSet::new(),
            source_capture_sequence: None,
            chunk_radius: None,
            resolved_server_position,
            stats: WorldStreamStats::default(),
        }
    }

    /// Accepts a normalized event using the FIFO sequence assigned by the
    /// network thread. Heavy payloads are decoded on Rayon workers.
    pub fn submit(&mut self, sequence: u64, event: WorldEvent) -> Result<(), WorldStreamError> {
        if sequence < self.ordered.next_sequence() || self.submitted.contains(&sequence) {
            return Err(SequenceError::DuplicateOrPast {
                sequence,
                next: self.ordered.next_sequence(),
            }
            .into());
        }

        let heavy = matches!(
            event,
            WorldEvent::LevelChunk(_) | WorldEvent::SubChunks(_) | WorldEvent::BlockUpdates(_)
        );
        let creates_request = matches!(
            &event,
            WorldEvent::LevelChunk(LevelChunkEvent {
                mode: LevelChunkMode::LimitedRequests { .. } | LevelChunkMode::LimitlessRequests,
                ..
            })
        );
        if creates_request && self.requests.len() >= OUTBOUND_REQUEST_CAPACITY {
            return Err(WorldStreamError::OutboundFull {
                sequence,
                pending: self.requests.len(),
                capacity: OUTBOUND_REQUEST_CAPACITY,
            });
        }
        if self.submitted.len()
            >= MAX_ADMITTED_WORLD_EVENTS.saturating_sub(self.committed_controls.len())
            || (heavy && self.heavy_sequences.len() >= MAX_ADMITTED_HEAVY_EVENTS)
        {
            return Err(WorldStreamError::AdmissionFull {
                sequence,
                admitted: self.submitted.len(),
                capacity: MAX_ADMITTED_WORLD_EVENTS,
                heavy_admitted: self.heavy_sequences.len(),
                heavy_capacity: MAX_ADMITTED_HEAVY_EVENTS,
            });
        }
        self.submitted.insert(sequence);
        if heavy {
            self.heavy_sequences.insert(sequence);
        }
        if creates_request {
            self.requests
                .push_back(OutboundRequestSlot::Reserved(sequence));
        }

        match event {
            WorldEvent::LevelChunk(
                event @ LevelChunkEvent {
                    mode: LevelChunkMode::Inline { count },
                    ..
                },
            ) => {
                let Some(range) = vanilla_dimension_range(event.dimension)
                    .filter(|range| count <= range.sub_chunk_count)
                else {
                    self.heavy_sequences.remove(&sequence);
                    self.ordered
                        .insert(sequence, PreparedWorldEvent::NormalizationFailure)?;
                    self.apply_ready();
                    return Ok(());
                };
                self.pending_decode.push_back(DecodeJob::InlineLevelChunk {
                    sequence,
                    event,
                    base_sub_chunk_y: range.base_sub_chunk_y,
                    count,
                    biome_storage_count: range.sub_chunk_count,
                });
            }
            WorldEvent::LevelChunk(
                event @ LevelChunkEvent {
                    mode: LevelChunkMode::LimitedRequests { .. } | LevelChunkMode::LimitlessRequests,
                    ..
                },
            ) => {
                let Some(range) = vanilla_dimension_range(event.dimension) else {
                    self.heavy_sequences.remove(&sequence);
                    self.ordered
                        .insert(sequence, PreparedWorldEvent::NormalizationFailure)?;
                    self.apply_ready();
                    return Ok(());
                };
                self.pending_decode.push_back(DecodeJob::RequestLevelChunk {
                    sequence,
                    event,
                    biome_base_sub_chunk_y: range.base_sub_chunk_y,
                    biome_storage_count: range.sub_chunk_count,
                });
            }
            WorldEvent::SubChunks(batch) => {
                if batch.entries.is_empty() {
                    self.heavy_sequences.remove(&sequence);
                    self.ordered
                        .insert(sequence, PreparedWorldEvent::NormalizationFailure)?;
                    self.apply_ready();
                    return Ok(());
                }
                self.record_sub_chunk_reply_admissions(&batch);
                self.pending_decode
                    .push_back(DecodeJob::SubChunks { sequence, batch });
            }
            immediate => {
                if let Err(error) = self
                    .ordered
                    .insert(sequence, PreparedWorldEvent::Immediate(immediate))
                {
                    self.cancel_request_reservation(sequence);
                    return Err(error.into());
                }
                self.apply_ready();
            }
        }
        Ok(())
    }

    /// Integrates completed worker results, commits all newly contiguous FIFO
    /// events, rejects stale meshes, and starts nearest pending mesh jobs.
    pub fn poll(&mut self, camera_position: [f32; 3], max_mesh_jobs: usize) -> WorldStreamPoll {
        let mut report = WorldStreamPoll::default();
        while let Ok(completion) = self.decode_rx.try_recv() {
            report.decoded_results += 1;
            self.accept_decode_completion(completion);
        }
        self.apply_ready();
        self.expire_sub_chunk_deadlines(Instant::now());
        self.pump_deferred_retries();
        self.dispatch_decode_jobs();

        while self.mesh_changes.len() < MAX_PENDING_MESH_CHANGES {
            let Ok(completion) = self.mesh_rx.try_recv() else {
                break;
            };
            report.mesh_results += 1;
            self.accept_mesh_completion(completion);
        }
        report.mesh_jobs_dispatched = self.dispatch_mesh_jobs(
            camera_position,
            max_mesh_jobs.min(MAX_PENDING_MESH_CHANGES.saturating_sub(self.mesh_changes.len())),
        );
        report
    }

    #[must_use]
    pub const fn current_dimension(&self) -> i32 {
        self.current_dimension
    }

    /// Returns the latest FIFO-committed live biome definitions for mesh/tint
    /// table construction. The Arc keeps worker snapshots immutable when a
    /// server later replaces the definition list.
    #[must_use]
    #[allow(
        dead_code,
        reason = "Phase 2.5 tint-table consumer lands after ingestion"
    )]
    pub fn biome_definitions_snapshot(&self) -> Arc<[BiomeDefinitionEvent]> {
        Arc::clone(&self.biome_definitions)
    }

    #[must_use]
    pub fn resolved_biome_tints_snapshot(&self) -> Arc<ResolvedBiomeTints> {
        Arc::clone(&self.resolved_biome_tints)
    }

    #[cfg(test)]
    #[must_use]
    pub const fn biome_tint_revision(&self) -> u64 {
        self.biome_tint_revision
    }

    #[must_use]
    pub const fn biome_tint_identity(&self) -> ChunkBiomeTintIdentity {
        ChunkBiomeTintIdentity::new(self.biome_tint_stream_id, self.biome_tint_revision)
    }

    #[must_use]
    pub const fn committed_view_cohort(&self) -> Option<ViewCohort> {
        self.committed_view_cohort
    }

    #[must_use]
    pub const fn local_player_runtime_id(&self) -> u64 {
        self.local_player_runtime_id
    }

    #[must_use]
    pub const fn resolved_server_position(&self) -> ResolvedServerPosition {
        self.resolved_server_position
    }

    #[must_use]
    pub fn remaining_admission_capacity(&self) -> usize {
        MAX_ADMITTED_WORLD_EVENTS
            .saturating_sub(
                self.submitted
                    .len()
                    .saturating_add(self.committed_controls.len()),
            )
            .min(MAX_ADMITTED_HEAVY_EVENTS.saturating_sub(self.heavy_sequences.len()))
            .min(OUTBOUND_REQUEST_CAPACITY.saturating_sub(self.requests.len()))
    }

    #[cfg(test)]
    #[must_use]
    pub fn connectivity(&self, key: SubChunkKey) -> Option<FaceConnectivity> {
        self.connectivity.get(&key).copied()
    }

    /// Monotonic revision for the cave-connectivity graph. The app uses this
    /// to avoid rebuilding the BFS result while both the graph and camera
    /// sub-chunk are unchanged.
    #[must_use]
    pub const fn connectivity_generation(&self) -> u64 {
        self.connectivity_generation
    }

    /// Resolves a safe eye position above the highest non-air block in a
    /// fully received column. Lookups stay in the paletted storages and never
    /// materialise a flat 16x16x16 block array.
    #[must_use]
    pub fn surface_eye_position(&self, block_x: i32, block_z: i32) -> Option<[f32; 3]> {
        let range = vanilla_dimension_range(self.current_dimension)?;
        let chunk = ChunkKey::new(
            self.current_dimension,
            block_x.div_euclid(16),
            block_z.div_euclid(16),
        );
        if !self.loaded_columns.contains(&chunk) {
            return None;
        }
        let keys = (0..range.sub_chunk_count)
            .map(|offset| SubChunkKey::from_chunk(chunk, range.base_sub_chunk_y + offset as i32));

        let local_x = block_x.rem_euclid(16) as u8;
        let local_z = block_z.rem_euclid(16) as u8;
        for key in keys.rev() {
            if self.known_air.contains(&key) {
                continue;
            }
            let Some(sub_chunk) = self.store.sub_chunk(key) else {
                continue;
            };
            for local_y in (0_u8..16).rev() {
                let solid = (0..sub_chunk.storages().len()).any(|layer| {
                    sub_chunk
                        .runtime_id(layer, local_x, local_y, local_z)
                        .is_some_and(|runtime_id| !self.classifier.is_air(runtime_id))
                });
                if solid {
                    let block_y = key.y.saturating_mul(16) + i32::from(local_y);
                    return Some([
                        block_x as f32 + 0.5,
                        block_y as f32 + 2.62,
                        block_z as f32 + 0.5,
                    ]);
                }
            }
        }
        None
    }

    /// Connectivity BFS from a camera sub-chunk. The returned set can be
    /// intersected with Bevy's frustum-visible set before render extraction.
    #[must_use]
    pub fn cave_visible_sub_chunks(&self, camera: SubChunkKey) -> BTreeSet<SubChunkKey> {
        crate::culling::cave_visible_sub_chunks(camera, &self.connectivity)
            .into_iter()
            .collect()
    }

    #[cfg(test)]
    pub fn take_requests(&mut self) -> Vec<PendingSubChunkRequest> {
        let mut ready = Vec::new();
        let mut reserved = VecDeque::new();
        while let Some(slot) = self.requests.pop_front() {
            match slot {
                OutboundRequestSlot::Reserved(sequence) => {
                    reserved.push_back(OutboundRequestSlot::Reserved(sequence));
                }
                OutboundRequestSlot::Ready(request) => ready.push(request),
            }
        }
        self.requests = reserved;
        ready
    }

    pub fn pop_next_request(&mut self) -> Option<PendingSubChunkRequest> {
        if !matches!(self.requests.front(), Some(OutboundRequestSlot::Ready(_))) {
            return None;
        }
        match self.requests.pop_front() {
            Some(OutboundRequestSlot::Ready(request)) => Some(request),
            Some(OutboundRequestSlot::Reserved(_)) | None => None,
        }
    }

    pub fn retry_request_front(
        &mut self,
        request: PendingSubChunkRequest,
    ) -> Result<(), Box<PendingSubChunkRequest>> {
        if self.requests.len() >= OUTBOUND_REQUEST_CAPACITY {
            return Err(Box::new(request));
        }
        self.requests
            .push_front(OutboundRequestSlot::Ready(request));
        Ok(())
    }

    /// Records command admission without starting a response deadline. The
    /// network worker will confirm the transport send separately.
    pub fn record_sub_chunk_request_transport_pending(
        &mut self,
        chunk: ChunkKey,
        base_sub_chunk_y: i32,
        count: usize,
    ) {
        for offset in 0..count {
            let y = base_sub_chunk_y.saturating_add(offset as i32);
            if let Some(pending) = self
                .requested_sub_chunks
                .get_mut(&chunk)
                .and_then(|column| column.get_mut(&y))
            {
                pending.pending_transport_attempts = pending
                    .pending_transport_attempts
                    .saturating_add(1)
                    .min(MAX_SUB_CHUNK_RETRIES.saturating_add(1));
            }
        }
    }

    /// Starts the response timeout only after the network layer confirms that
    /// the SubChunkRequest transport send completed successfully.
    pub fn acknowledge_sub_chunk_request_sent(
        &mut self,
        chunk: ChunkKey,
        base_sub_chunk_y: i32,
        count: usize,
        sent_at: Instant,
    ) {
        let deadline = sent_at
            .checked_add(SUB_CHUNK_RESPONSE_TIMEOUT)
            .unwrap_or(sent_at);
        for offset in 0..count {
            let y = base_sub_chunk_y.saturating_add(offset as i32);
            let key = SubChunkKey::from_chunk(chunk, y);
            let reply_admitted = self
                .admitted_sub_chunk_replies
                .get(&key)
                .is_some_and(|admitted| *admitted != 0);
            let pending = self
                .requested_sub_chunks
                .get_mut(&chunk)
                .and_then(|column| column.get_mut(&y));
            let Some(pending) = pending else {
                if let Some(correlated) = self.correlated_sub_chunk_attempts.get_mut(&key)
                    && correlated.pending_transport_attempts != 0
                {
                    correlated.pending_transport_attempts =
                        correlated.pending_transport_attempts.saturating_sub(1);
                    correlated.confirmed_attempts = correlated
                        .confirmed_attempts
                        .saturating_add(1)
                        .min(MAX_SUB_CHUNK_RETRIES.saturating_add(1));
                }
                continue;
            };
            pending.pending_transport_attempts =
                pending.pending_transport_attempts.saturating_sub(1);
            pending.confirmed_attempts = pending
                .confirmed_attempts
                .saturating_add(1)
                .min(MAX_SUB_CHUNK_RETRIES.saturating_add(1));
            if reply_admitted {
                if let Some(previous) = pending.response_deadline.take() {
                    self.sub_chunk_deadlines.remove(&(previous, key));
                }
                continue;
            }
            let previous = pending.response_deadline.replace(deadline);
            if let Some(previous) = previous {
                self.sub_chunk_deadlines.remove(&(previous, key));
            }
            self.sub_chunk_deadlines.insert((deadline, key));
        }
        debug_assert!(self.sub_chunk_deadlines.len() <= self.outstanding_sub_chunk_count());
    }

    #[must_use]
    pub fn pending_request_count(&self) -> usize {
        self.requests
            .iter()
            .filter(|slot| matches!(slot, OutboundRequestSlot::Ready(_)))
            .count()
    }

    #[must_use]
    pub fn pending_request_work_count(&self) -> usize {
        self.requests.len()
    }

    #[must_use]
    pub fn outstanding_sub_chunk_count(&self) -> usize {
        self.requested_sub_chunks
            .values()
            .fold(0, |total, pending| total.saturating_add(pending.len()))
    }

    #[cfg(test)]
    pub fn take_mesh_changes(&mut self) -> Vec<WorldMeshChange> {
        self.mesh_changes.drain(..).collect()
    }

    pub fn pop_mesh_change(&mut self) -> Option<WorldMeshChange> {
        self.mesh_changes.pop_front()
    }

    #[must_use]
    pub fn pending_mesh_change_count(&self) -> usize {
        self.mesh_changes.len()
    }

    #[must_use]
    pub fn unacknowledged_mesh_count(&self) -> usize {
        self.revisions.entries.len()
    }

    #[must_use]
    pub fn is_mesh_clean(&self, key: SubChunkKey) -> bool {
        self.resident.contains(&key) && self.revisions.dirty(key).is_none()
    }

    // A rejected change must be returned intact so the caller can retry it
    // without cloning or dropping any packed stream. Boxing would move this
    // established hot-path ownership contract behind an allocation.
    #[allow(clippy::result_large_err)]
    pub fn retry_mesh_change_front(
        &mut self,
        change: WorldMeshChange,
    ) -> Result<(), WorldMeshChange> {
        if self.mesh_changes.len() >= MAX_PENDING_MESH_CHANGES {
            return Err(change);
        }
        self.mesh_changes.push_front(change);
        Ok(())
    }

    pub fn acknowledge_mesh_upload(
        &mut self,
        key: SubChunkKey,
        generation: u64,
        dirty_since: Instant,
        applied_at: Instant,
    ) {
        let Some(dirty) = self.revisions.dirty(key) else {
            return;
        };
        if dirty.revision != generation || dirty.since != dirty_since {
            return;
        }
        self.stats.max_remesh_latency = self
            .stats
            .max_remesh_latency
            .max(applied_at.saturating_duration_since(dirty_since));
        self.stats.last_mesh_ack_at = Some(
            self.stats
                .last_mesh_ack_at
                .map_or(applied_at, |latest| latest.max(applied_at)),
        );
        self.applied_mesh_generations.insert(key, generation);
        self.revisions.clear_if_current(key, generation);
    }

    pub fn take_committed_controls(&mut self) -> Vec<CommittedControlEvent> {
        self.committed_controls.drain(..).collect()
    }

    #[must_use]
    pub fn stats(&self) -> WorldStreamStats {
        let completed_decode_results = self
            .heavy_sequences
            .len()
            .saturating_sub(self.pending_decode.len())
            .saturating_sub(self.in_flight_decode_jobs);
        WorldStreamStats {
            received_radius_chunks: self.chunk_radius,
            publisher_radius_chunks: self.publisher_radius_chunks,
            resident_sub_chunks: self.resident.len(),
            pending_mesh_jobs: self.pending_mesh.len(),
            in_flight_mesh_jobs: self.in_flight.len(),
            admitted_world_events: self.submitted.len(),
            admitted_heavy_events: self.heavy_sequences.len(),
            queued_decode_jobs: self.pending_decode.len(),
            in_flight_decode_jobs: self.in_flight_decode_jobs,
            completed_decode_results,
            pending_retry_requests: self.queued_retry_request_count(),
            awaiting_sub_chunk_responses: self.sub_chunk_deadlines.len(),
            ..self.stats
        }
    }

    pub fn begin_timed_session(&mut self) {
        self.stats.max_decode_duration = Duration::ZERO;
        self.stats.max_mesh_duration = Duration::ZERO;
        self.stats.max_remesh_latency = Duration::ZERO;
        self.stats.last_chunk_commit_at = None;
        self.stats.last_mesh_dispatch_at = None;
        self.stats.last_mesh_completion_at = None;
        self.stats.last_mesh_ack_at = None;
    }

    #[must_use]
    pub fn loaded_column_count(&self) -> usize {
        self.loaded_columns.len()
    }

    /// Captures key-only source evidence at the caller-selected commit edge.
    pub fn capture_source_columns(&mut self) {
        self.source_columns = self.tracked_columns();
    }

    /// Schedules source-column capture at the exact FIFO commit edge for one
    /// already-observed MovePlayer sequence.
    pub fn schedule_source_capture(&mut self, sequence: u64) {
        self.source_capture_sequence = Some(sequence);
    }

    #[must_use]
    pub fn cohort_status(&self, target: ViewCohort) -> ViewCohortStatus {
        let expected_columns = target.expected_columns();
        let loaded_target = self.loaded_columns.intersection(&expected_columns).count();
        let missing_target = expected_columns.difference(&self.loaded_columns).count();
        let foreign_loaded = self.loaded_columns.difference(&expected_columns).count();
        let foreign_requested = self
            .requested_sub_chunks
            .keys()
            .filter(|column| !expected_columns.contains(column))
            .count();
        let foreign_resident = self
            .resident
            .iter()
            .chain(&self.known_air)
            .copied()
            .filter(|key| !expected_columns.contains(&key.chunk()))
            .collect::<BTreeSet<_>>()
            .len();
        let source_leftover = self
            .tracked_columns()
            .intersection(&self.source_columns)
            .count();

        ViewCohortStatus {
            target,
            committed: self.committed_view_cohort,
            expected: expected_columns.len(),
            loaded_target,
            missing_target,
            foreign_loaded,
            foreign_requested,
            foreign_resident,
            source_leftover,
            resident_count: self.resident.len(),
            resident_hash: deterministic_sub_chunk_key_hash(&self.resident),
            known_air_count: self.known_air.len(),
            known_air_hash: deterministic_sub_chunk_key_hash(&self.known_air),
        }
    }

    pub fn remesh_all_resident(&mut self, now: Instant) -> ForcedRemeshManifest {
        let keys = self
            .resident
            .iter()
            .chain(&self.known_air)
            .copied()
            .collect::<BTreeSet<_>>();
        let entries = keys
            .into_iter()
            .map(|key| (key, self.mark_forced_dirty_exact(key, now)))
            .collect::<Vec<_>>();
        ForcedRemeshManifest {
            started_at: now,
            entries: Arc::from(entries),
        }
    }

    #[must_use]
    pub fn forced_remesh_manifest_state(
        &self,
        manifest: &ForcedRemeshManifest,
    ) -> ForcedRemeshManifestState {
        let current_keys = self
            .resident
            .iter()
            .chain(&self.known_air)
            .copied()
            .collect::<BTreeSet<_>>();
        let manifest_keys = manifest
            .entries
            .iter()
            .map(|(key, _)| *key)
            .collect::<BTreeSet<_>>();
        if manifest.entries.is_empty()
            || manifest_keys.len() != manifest.entries.len()
            || manifest_keys != current_keys
        {
            return ForcedRemeshManifestState::Invalid;
        }

        let mut pending = false;
        for &(key, generation) in manifest.entries.iter() {
            match self.revisions.dirty(key) {
                Some(dirty)
                    if dirty.revision == generation && dirty.since == manifest.started_at =>
                {
                    pending = true;
                }
                Some(_) => return ForcedRemeshManifestState::Invalid,
                None if self.applied_mesh_generations.get(&key) == Some(&generation) => {}
                None => return ForcedRemeshManifestState::Invalid,
            }
        }
        if pending {
            ForcedRemeshManifestState::Pending
        } else {
            ForcedRemeshManifestState::Complete
        }
    }

    fn record_normalization_error(&mut self, reason: NormalizationErrorReason) {
        self.stats.normalization_errors = self.stats.normalization_errors.saturating_add(1);
        self.stats.normalization_reasons.record(reason);
    }

    fn apply_ready(&mut self) {
        if self.blocking_block_updates.is_some() {
            return;
        }
        while let Some(event) = self.ordered.pop_next() {
            let sequence = self.ordered.next_sequence().saturating_sub(1);
            match event {
                PreparedWorldEvent::Immediate(WorldEvent::BlockUpdates(events)) => {
                    let batches = self.snapshot_block_mutation_batches(events);
                    if batches.is_empty() {
                        self.submitted.remove(&sequence);
                        self.heavy_sequences.remove(&sequence);
                        continue;
                    }
                    self.pending_decode.push_back(DecodeJob::BlockUpdates {
                        sequence,
                        batches,
                        air_runtime_id: self.classifier.air_network_id(),
                    });
                    self.blocking_block_updates = Some(sequence);
                    break;
                }
                event => {
                    self.submitted.remove(&sequence);
                    self.heavy_sequences.remove(&sequence);
                    self.apply_prepared_with_sequence(event, Some(sequence));
                    self.cancel_request_reservation(sequence);
                }
            }
        }
    }

    fn accept_decode_completion(&mut self, completion: DecodeCompletion) {
        self.in_flight_decode_jobs = self.in_flight_decode_jobs.saturating_sub(1);
        if self.blocking_block_updates == Some(completion.sequence)
            && matches!(&completion.event, PreparedWorldEvent::BlockUpdates { .. })
        {
            self.blocking_block_updates = None;
            self.submitted.remove(&completion.sequence);
            self.heavy_sequences.remove(&completion.sequence);
            self.apply_prepared(completion.event);
            self.apply_ready();
            return;
        }
        if self
            .ordered
            .insert(completion.sequence, completion.event)
            .is_err()
        {
            self.heavy_sequences.remove(&completion.sequence);
            self.record_normalization_error(NormalizationErrorReason::OrderedCompletionRejection);
        }
    }

    fn snapshot_block_mutation_batches(
        &mut self,
        events: Vec<BlockUpdateEvent>,
    ) -> Vec<BlockMutationBatch> {
        let mut grouped = BTreeMap::<SubChunkKey, Vec<BlockUpdate>>::new();
        for event in events {
            match split_block_update(event) {
                Ok((key, update)) if self.column_is_active(key.chunk()) => {
                    grouped.entry(key).or_default().push(update);
                }
                Ok(_) => {
                    self.record_normalization_error(NormalizationErrorReason::InactiveBlockUpdate)
                }
                Err(_) => {
                    self.record_normalization_error(NormalizationErrorReason::MalformedBlockUpdate)
                }
            }
        }
        grouped
            .into_iter()
            .map(|(key, updates)| BlockMutationBatch {
                key,
                previous: self.store.sub_chunk(key),
                updates,
            })
            .collect()
    }

    fn dispatch_decode_jobs(&mut self) {
        let budget = DECODE_DISPATCH_BUDGET_PER_POLL
            .min(MAX_IN_FLIGHT_DECODE_JOBS.saturating_sub(self.in_flight_decode_jobs));
        for _ in 0..budget {
            let Some(job) = self.pending_decode.pop_front() else {
                break;
            };
            self.in_flight_decode_jobs += 1;
            let tx = self.decode_tx.clone();
            rayon::spawn(move || {
                let started = Instant::now();
                let completion = match job {
                    DecodeJob::InlineLevelChunk {
                        sequence,
                        mut event,
                        base_sub_chunk_y,
                        count,
                        biome_storage_count,
                    } => {
                        let payload = std::mem::take(&mut event.payload);
                        let decoded = DecodedLevelChunk::decode_with_biomes(
                            base_sub_chunk_y,
                            count,
                            base_sub_chunk_y,
                            biome_storage_count,
                            &payload,
                        );
                        DecodeCompletion {
                            sequence,
                            event: PreparedWorldEvent::InlineLevelChunk {
                                event,
                                decoded,
                                duration: started.elapsed(),
                            },
                        }
                    }
                    DecodeJob::RequestLevelChunk {
                        sequence,
                        mut event,
                        biome_base_sub_chunk_y,
                        biome_storage_count,
                    } => {
                        let payload = std::mem::take(&mut event.payload);
                        let decoded = DecodedBiomeColumn::decode(
                            biome_base_sub_chunk_y,
                            biome_storage_count,
                            &payload,
                        );
                        DecodeCompletion {
                            sequence,
                            event: PreparedWorldEvent::RequestLevelChunk {
                                event,
                                decoded,
                                duration: started.elapsed(),
                            },
                        }
                    }
                    DecodeJob::SubChunks { sequence, batch } => {
                        let dimension = batch.dimension;
                        let entries = prepare_sub_chunks(batch);
                        DecodeCompletion {
                            sequence,
                            event: PreparedWorldEvent::SubChunks {
                                dimension,
                                entries,
                                duration: started.elapsed(),
                            },
                        }
                    }
                    DecodeJob::BlockUpdates {
                        sequence,
                        batches,
                        air_runtime_id,
                    } => {
                        let result = batches
                            .into_iter()
                            .map(|batch| {
                                ChunkStore::prepare_sub_chunk_blocks(
                                    batch.key,
                                    batch.previous.as_deref(),
                                    &batch.updates,
                                    air_runtime_id,
                                )
                            })
                            .collect();
                        DecodeCompletion {
                            sequence,
                            event: PreparedWorldEvent::BlockUpdates {
                                result,
                                duration: started.elapsed(),
                            },
                        }
                    }
                };
                let _ = tx.send(completion);
            });
        }
    }

    fn apply_prepared(&mut self, event: PreparedWorldEvent) {
        self.apply_prepared_with_sequence(event, None);
    }

    fn apply_prepared_with_sequence(&mut self, event: PreparedWorldEvent, sequence: Option<u64>) {
        match event {
            PreparedWorldEvent::InlineLevelChunk {
                event,
                decoded,
                duration,
            } => {
                self.stats.max_decode_duration = self.stats.max_decode_duration.max(duration);
                let key = ChunkKey::new(event.dimension, event.x, event.z);
                if !self.column_is_active(key) {
                    self.record_normalization_error(NormalizationErrorReason::InactiveInlineChunk);
                    return;
                }
                match decoded {
                    Ok(decoded) => {
                        let range = vanilla_dimension_range(event.dimension)
                            .expect("inline events are range-checked before decode");
                        let count = match event.mode {
                            LevelChunkMode::Inline { count } => count,
                            _ => unreachable!("prepared LevelChunk must be inline"),
                        };
                        let stored_keys = decoded
                            .sub_chunks()
                            .map(|(y, _)| SubChunkKey::from_chunk(key, y))
                            .collect::<BTreeSet<_>>();
                        let new_keys = (0..count)
                            .map(|offset| {
                                SubChunkKey::from_chunk(key, range.base_sub_chunk_y + offset as i32)
                            })
                            .collect::<BTreeSet<_>>();
                        let air_keys = new_keys
                            .difference(&stored_keys)
                            .copied()
                            .collect::<BTreeSet<_>>();
                        let old_keys = self
                            .resident
                            .iter()
                            .copied()
                            .filter(|resident| resident.chunk() == key)
                            .collect::<BTreeSet<_>>();
                        let old_air = self
                            .known_air
                            .iter()
                            .copied()
                            .filter(|resident| resident.chunk() == key)
                            .collect::<BTreeSet<_>>();
                        let applied = self.store.commit_level_chunk(key, decoded);
                        self.loaded_columns.insert(key);
                        self.purge_sub_chunk_column_state(key);
                        self.resident.retain(|resident| resident.chunk() != key);
                        self.known_air.retain(|resident| resident.chunk() != key);
                        for stale in old_keys.difference(&new_keys) {
                            self.set_connectivity(*stale, None);
                        }
                        for no_longer_air in old_air.difference(&air_keys) {
                            self.set_connectivity(*no_longer_air, None);
                        }
                        self.resident.extend(new_keys.iter().copied());
                        for air in air_keys {
                            self.record_known_air(air);
                        }
                        let now = Instant::now();
                        let mut changed_sources =
                            applied.changed.into_iter().collect::<BTreeSet<_>>();
                        changed_sources.extend(old_keys.difference(&new_keys).copied());
                        self.mark_changed_sources(changed_sources, now);
                        self.stats.last_chunk_commit_at = Some(now);
                    }
                    Err(_) => self.stats.decode_errors = self.stats.decode_errors.saturating_add(1),
                }
            }
            PreparedWorldEvent::RequestLevelChunk {
                event,
                decoded,
                duration,
            } => {
                self.stats.max_decode_duration = self.stats.max_decode_duration.max(duration);
                match decoded {
                    Ok(decoded) => self.apply_request_level_chunk(event, decoded, sequence),
                    Err(_) => self.stats.decode_errors = self.stats.decode_errors.saturating_add(1),
                }
            }
            PreparedWorldEvent::SubChunks {
                dimension,
                entries,
                duration,
            } => {
                self.stats.max_decode_duration = self.stats.max_decode_duration.max(duration);
                let mut committed_any = false;
                for entry in entries {
                    let key = SubChunkKey::new(
                        dimension,
                        entry.position[0],
                        entry.position[1],
                        entry.position[2],
                    );
                    if !self.column_is_active(key.chunk()) {
                        continue;
                    }
                    let admitted = self.consume_admitted_sub_chunk_reply(key);
                    if !self.is_expected_sub_chunk(key) {
                        if admitted && self.consume_correlated_sub_chunk_attempt(key) {
                            continue;
                        }
                        self.record_normalization_error(
                            NormalizationErrorReason::UnexpectedSubChunk,
                        );
                        continue;
                    }
                    self.consume_confirmed_sub_chunk_attempt(key);
                    self.disarm_sub_chunk_deadline(key);
                    let (completed, committed) = match entry.result {
                        PreparedSubChunkResult::Decoded(Ok(decoded)) => {
                            let decoded_air = decoded.has_no_storages();
                            let committed = match self.store.commit_sub_chunk(key, decoded) {
                                Ok(Some(changed)) => {
                                    if decoded_air {
                                        self.record_known_air(changed);
                                    } else {
                                        self.sync_resident(changed);
                                    }
                                    self.mark_changed(changed, Instant::now());
                                    true
                                }
                                Ok(None) => {
                                    if decoded_air {
                                        self.record_known_air(key);
                                    }
                                    true
                                }
                                Err(_) => {
                                    self.stats.decode_errors =
                                        self.stats.decode_errors.saturating_add(1);
                                    false
                                }
                            };
                            (true, committed)
                        }
                        PreparedSubChunkResult::Decoded(Err(_)) => {
                            self.stats.decode_errors = self.stats.decode_errors.saturating_add(1);
                            (self.retry_or_complete_sub_chunk(key), false)
                        }
                        PreparedSubChunkResult::AllAir => {
                            let changed = self.store.apply_all_air(key);
                            self.record_known_air(key);
                            if let Some(changed) = changed {
                                self.mark_changed(changed, Instant::now());
                            }
                            (true, true)
                        }
                        PreparedSubChunkResult::Unavailable(unavailable) => {
                            self.stats.unavailable_sub_chunks =
                                self.stats.unavailable_sub_chunks.saturating_add(1);
                            match unavailable {
                                protocol::SubChunkUnavailable::YIndexOutOfBounds => {
                                    let changed = self.store.apply_all_air(key);
                                    self.record_known_air(key);
                                    if let Some(changed) = changed {
                                        self.mark_changed(changed, Instant::now());
                                    }
                                    (true, true)
                                }
                                protocol::SubChunkUnavailable::InvalidDimension => {
                                    self.record_normalization_error(
                                        NormalizationErrorReason::InvalidDimensionSubChunk,
                                    );
                                    (true, false)
                                }
                                protocol::SubChunkUnavailable::ChunkNotFound
                                | protocol::SubChunkUnavailable::PlayerNotFound => {
                                    (self.retry_or_complete_sub_chunk(key), false)
                                }
                                protocol::SubChunkUnavailable::Undefined
                                | protocol::SubChunkUnavailable::Unknown(_) => (true, false),
                            }
                        }
                    };
                    committed_any |= committed;
                    if completed {
                        self.complete_requested_sub_chunk(key);
                    }
                }
                if committed_any {
                    self.stats.last_chunk_commit_at = Some(Instant::now());
                }
            }
            PreparedWorldEvent::BlockUpdates { result, duration } => {
                self.stats.max_decode_duration = self.stats.max_decode_duration.max(duration);
                match result {
                    Ok(prepared) => {
                        let changed = self.store.commit_prepared_block_updates(prepared);
                        let now = Instant::now();
                        for key in changed {
                            self.sync_resident(key);
                            self.mark_changed(key, now);
                        }
                    }
                    Err(_) => {
                        self.record_normalization_error(
                            NormalizationErrorReason::BlockMutationFailure,
                        );
                    }
                }
            }
            PreparedWorldEvent::Immediate(event) => self.apply_immediate(event, sequence),
            PreparedWorldEvent::NormalizationFailure => {
                self.record_normalization_error(NormalizationErrorReason::EmptySubChunkBatch);
            }
        }
    }

    fn apply_immediate(&mut self, event: WorldEvent, sequence: Option<u64>) {
        match event {
            WorldEvent::BiomeDefinitions(event) => {
                let live = event
                    .definitions
                    .iter()
                    .map(|definition| LiveBiomeDefinition {
                        name: definition.name.as_ref(),
                        biome_id: definition.biome_id,
                        temperature: definition.temperature,
                        downfall: definition.downfall,
                        map_water_argb: definition.map_water_color,
                    })
                    .collect::<Vec<_>>();
                let Ok(resolved) = self.runtime_assets.biome_assets().resolve_live(&live) else {
                    self.record_normalization_error(
                        NormalizationErrorReason::BiomeDefinitionResolutionFailure,
                    );
                    return;
                };
                let Some(next_revision) = self.biome_tint_revision.checked_add(1) else {
                    self.record_normalization_error(
                        NormalizationErrorReason::BiomeTintRevisionOverflow,
                    );
                    return;
                };
                self.biome_tint_revision = next_revision;
                self.biome_definitions = event.definitions;
                self.resolved_biome_tints = Arc::new(resolved);
                self.invalidate_resident_biome_tints(Instant::now());
            }
            WorldEvent::LevelChunk(_) => {
                unreachable!("LevelChunk packets are prepared on workers")
            }
            WorldEvent::BlockUpdates(_) => {
                unreachable!("block-update batches are prepared on workers")
            }
            WorldEvent::ChunkRadiusUpdated(radius) => {
                if radius < 0 {
                    self.record_normalization_error(NormalizationErrorReason::InvalidChunkRadius);
                    return;
                }
                self.chunk_radius = Some(radius.min(PHASE0_MAX_VIEW_RADIUS_CHUNKS));
                self.evict_outside_active_radius();
            }
            WorldEvent::PublisherUpdate(update) => {
                self.publisher_center = Some(update.center);
                let cohort = ViewCohort::from_publisher(
                    self.current_dimension,
                    update.center,
                    update.radius_blocks,
                );
                self.publisher_radius_chunks =
                    Some(cohort.radius.min(PHASE0_MAX_VIEW_RADIUS_CHUNKS));
                self.committed_view_cohort = Some(cohort);
                self.evict_outside_active_radius();
            }
            WorldEvent::ChangeDimension(change) => {
                self.evict_all_resident();
                self.current_dimension = change.dimension;
                let resolved = resolve_server_position(
                    change.position,
                    self.resolved_server_position.position,
                    self.resolved_server_position.surface_anchor,
                );
                self.resolved_server_position = resolved;
                self.publisher_center = Some([
                    floor_to_i32(resolved.position[0]),
                    floor_to_i32(resolved.position[1]),
                    floor_to_i32(resolved.position[2]),
                ]);
                self.publisher_radius_chunks = None;
                self.committed_view_cohort = None;
                self.push_committed_control(CommittedControlEvent::ChangeDimension {
                    change,
                    resolved,
                });
            }
            WorldEvent::MovePlayer(movement) => {
                let sequence = sequence.expect("sequenced MovePlayer commits through submit");
                let source_cohort = self.committed_view_cohort;
                if self.source_capture_sequence == Some(sequence) {
                    self.capture_source_columns();
                    self.source_capture_sequence = None;
                }
                let resolved = resolve_server_position(
                    movement.position,
                    self.resolved_server_position.position,
                    self.resolved_server_position.surface_anchor,
                );
                self.resolved_server_position = resolved;
                self.push_committed_control(CommittedControlEvent::MovePlayer {
                    sequence,
                    movement,
                    resolved,
                    source_cohort,
                });
            }
            WorldEvent::SubChunks(_) => unreachable!("sub-chunk batches are prepared on workers"),
        }
    }

    fn apply_request_level_chunk(
        &mut self,
        event: LevelChunkEvent,
        decoded: DecodedBiomeColumn,
        sequence: Option<u64>,
    ) {
        let key = ChunkKey::new(event.dimension, event.x, event.z);
        if !self.column_is_active(key) {
            self.record_normalization_error(NormalizationErrorReason::InactiveLevelChunk);
            return;
        }
        let Some(range) = vanilla_dimension_range(event.dimension) else {
            self.record_normalization_error(
                NormalizationErrorReason::UnsupportedLevelChunkDimension,
            );
            return;
        };
        let count = match event.mode {
            LevelChunkMode::LimitedRequests { highest } => {
                usize::from(highest).min(range.sub_chunk_count)
            }
            LevelChunkMode::LimitlessRequests => range.sub_chunk_count,
            LevelChunkMode::Inline { .. } => {
                unreachable!("inline LevelChunk packets are prepared on workers")
            }
        };
        self.evict_column(key);
        let _ = self.store.commit_biome_column(key, decoded);
        self.enqueue_request(key, range.base_sub_chunk_y, count, sequence);
    }

    fn push_committed_control(&mut self, event: CommittedControlEvent) {
        assert!(
            self.committed_controls.len() < COMMITTED_CONTROL_CAPACITY,
            "control admission invariant exceeded bounded commit-delta capacity"
        );
        self.committed_controls.push_back(event);
    }

    fn enqueue_request(
        &mut self,
        key: ChunkKey,
        base_sub_chunk_y: i32,
        count: usize,
        sequence: Option<u64>,
    ) {
        match request_sub_chunk_column(key.dimension, key.x, key.z, base_sub_chunk_y, count) {
            Ok(packet) => {
                let request = PendingSubChunkRequest {
                    packet,
                    dimension: key.dimension,
                    chunk: key,
                    base_sub_chunk_y,
                    count,
                };
                if !self.place_outbound_request(sequence, request) {
                    self.record_normalization_error(
                        NormalizationErrorReason::OutboundRequestPlacementFailure,
                    );
                    return;
                }
                let expected = (0..count)
                    .map(|offset| {
                        (
                            base_sub_chunk_y.saturating_add(offset as i32),
                            PendingSubChunk::default(),
                        )
                    })
                    .collect::<PendingSubChunkColumn>();
                if expected.is_empty() {
                    self.loaded_columns.insert(key);
                } else {
                    self.requested_sub_chunks.insert(key, expected);
                }
            }
            Err(_) => {
                self.record_normalization_error(NormalizationErrorReason::RequestEncodingFailure)
            }
        }
    }

    fn place_outbound_request(
        &mut self,
        sequence: Option<u64>,
        request: PendingSubChunkRequest,
    ) -> bool {
        if let Some(sequence) = sequence
            && let Some(slot) = self.requests.iter_mut().find(|slot| {
                matches!(slot, OutboundRequestSlot::Reserved(reserved) if *reserved == sequence)
            })
        {
            *slot = OutboundRequestSlot::Ready(request);
            return true;
        }
        if self.requests.len() >= OUTBOUND_REQUEST_CAPACITY {
            return false;
        }
        self.requests.push_back(OutboundRequestSlot::Ready(request));
        true
    }

    fn cancel_request_reservation(&mut self, sequence: u64) {
        self.requests.retain(|slot| {
            !matches!(slot, OutboundRequestSlot::Reserved(reserved) if *reserved == sequence)
        });
    }

    fn sync_resident(&mut self, key: SubChunkKey) {
        if self.store.sub_chunk(key).is_some() {
            self.resident.insert(key);
            self.known_air.remove(&key);
        } else {
            self.record_known_air(key);
        }
    }

    fn record_known_air(&mut self, key: SubChunkKey) {
        self.resident.insert(key);
        if self.known_air.insert(key) {
            self.mesh_dependency_masks.remove(&key);
        }
        self.set_connectivity(key, Some(FaceConnectivity::all()));
    }

    fn mark_changed(&mut self, key: SubChunkKey, now: Instant) {
        self.mark_changed_sources(std::iter::once(key), now);
    }

    fn mark_changed_sources(
        &mut self,
        sources: impl IntoIterator<Item = SubChunkKey>,
        now: Instant,
    ) {
        let mut dirty = BTreeSet::new();
        for key in sources {
            dirty.extend(key.mesh_dependents());
            for dependent in key.mesh_neighbourhood_dependents() {
                let ao_needed = self.resident.contains(&dependent)
                    && self
                        .current_mesh_dependency_mask(dependent)
                        .is_none_or(|mask| mask.diagonal_ao);
                if ao_needed {
                    dirty.insert(dependent);
                }
            }
            for dependent in key.liquid_mesh_dependents() {
                let liquid_needed = self.resident.contains(&dependent)
                    && self
                        .current_mesh_dependency_mask(dependent)
                        .is_none_or(|mask| mask.liquid);
                if liquid_needed {
                    dirty.insert(dependent);
                }
            }
        }
        for dependent in dirty {
            self.mark_dirty_exact(dependent, now);
        }
    }

    fn current_mesh_dependency_mask(&self, key: SubChunkKey) -> Option<MeshDependencyMask> {
        let (generation, mask) = self.mesh_dependency_masks.get(&key).copied()?;
        let current_generation = self
            .revisions
            .dirty(key)
            .map(|dirty| dirty.revision)
            .or_else(|| self.applied_mesh_generations.get(&key).copied())?;
        (generation == current_generation).then_some(mask)
    }

    fn register_mesh_dependency_mask(
        &mut self,
        key: SubChunkKey,
        generation: u64,
        mask: MeshDependencyMask,
    ) -> bool {
        if !self.resident.contains(&key) || !self.revisions.is_current(key, generation) {
            return false;
        }
        self.mesh_dependency_masks.insert(key, (generation, mask));
        true
    }

    #[cfg(test)]
    fn mesh_dependency_mask(&self, key: SubChunkKey) -> Option<(u64, MeshDependencyMask)> {
        self.mesh_dependency_masks.get(&key).copied()
    }

    fn mark_dirty_exact(&mut self, key: SubChunkKey, now: Instant) -> u64 {
        let revision = self.revisions.mark_dirty(key, now);
        let since = self.revisions.dirty(key).map_or(now, |dirty| dirty.since);
        self.pending_mesh
            .insert(key, PendingMesh { revision, since });
        revision
    }

    fn mark_forced_dirty_exact(&mut self, key: SubChunkKey, now: Instant) -> u64 {
        let revision = self.revisions.force_dirty_since(key, now);
        self.pending_mesh.insert(
            key,
            PendingMesh {
                revision,
                since: now,
            },
        );
        revision
    }

    fn invalidate_resident_biome_tints(&mut self, now: Instant) {
        let renderable = self
            .resident
            .iter()
            .copied()
            .filter(|key| self.store.sub_chunk(*key).is_some())
            .collect::<Vec<_>>();
        for key in renderable {
            self.mark_forced_dirty_exact(key, now);
            self.in_flight.remove(&key);
        }
        self.mesh_changes
            .retain(|change| !matches!(change, WorldMeshChange::Upsert { .. }));
    }

    fn evict_column(&mut self, key: ChunkKey) {
        self.loaded_columns.remove(&key);
        self.purge_sub_chunk_column_state(key);
        let mut changed = self
            .resident
            .iter()
            .copied()
            .filter(|resident| resident.chunk() == key)
            .collect::<BTreeSet<_>>();
        changed.extend(self.store.evict_chunk(key));
        self.resident.retain(|resident| resident.chunk() != key);
        self.known_air.retain(|resident| resident.chunk() != key);
        self.applied_mesh_generations
            .retain(|resident, _| resident.chunk() != key);
        self.mesh_dependency_masks
            .retain(|resident, _| resident.chunk() != key);
        let old_connectivity_len = self.connectivity.len();
        self.connectivity
            .retain(|resident, _| resident.chunk() != key);
        if self.connectivity.len() != old_connectivity_len {
            self.bump_connectivity_generation();
        }
        let now = Instant::now();
        for changed in changed {
            self.mark_changed(changed, now);
        }
    }

    fn evict_all_resident(&mut self) {
        let mut columns = self
            .resident
            .iter()
            .map(|key| key.chunk())
            .collect::<BTreeSet<_>>();
        columns.extend(self.loaded_columns.iter().copied());
        columns.extend(self.requested_sub_chunks.keys().copied());
        for column in columns {
            self.evict_column(column);
        }
    }

    fn tracked_columns(&self) -> BTreeSet<ChunkKey> {
        let mut columns = self.loaded_columns.clone();
        columns.extend(self.requested_sub_chunks.keys().copied());
        columns.extend(self.resident.iter().map(|key| key.chunk()));
        columns.extend(self.known_air.iter().map(|key| key.chunk()));
        columns
    }

    fn evict_outside_active_radius(&mut self) {
        let Some(center) = self.publisher_center else {
            return;
        };
        let radius = self.active_radius_chunks();
        let center_x = center[0].div_euclid(16);
        let center_z = center[2].div_euclid(16);
        let mut columns = self
            .resident
            .iter()
            .map(|key| key.chunk())
            .filter(|key| {
                key.dimension != self.current_dimension
                    || i64::from(key.x).abs_diff(i64::from(center_x)) > radius as u64
                    || i64::from(key.z).abs_diff(i64::from(center_z)) > radius as u64
            })
            .collect::<BTreeSet<_>>();
        columns.extend(self.loaded_columns.iter().copied().filter(|key| {
            key.dimension != self.current_dimension
                || i64::from(key.x).abs_diff(i64::from(center_x)) > radius as u64
                || i64::from(key.z).abs_diff(i64::from(center_z)) > radius as u64
        }));
        columns.extend(self.requested_sub_chunks.keys().copied().filter(|key| {
            key.dimension != self.current_dimension
                || i64::from(key.x).abs_diff(i64::from(center_x)) > radius as u64
                || i64::from(key.z).abs_diff(i64::from(center_z)) > radius as u64
        }));
        for column in columns {
            self.evict_column(column);
        }
    }

    fn active_radius_chunks(&self) -> i32 {
        match (self.publisher_radius_chunks, self.chunk_radius) {
            (Some(publisher), Some(chunk)) => publisher.min(chunk),
            (Some(radius), None) | (None, Some(radius)) => radius,
            (None, None) => PHASE0_MAX_VIEW_RADIUS_CHUNKS,
        }
        .clamp(0, PHASE0_MAX_VIEW_RADIUS_CHUNKS)
    }

    fn column_is_active(&self, key: ChunkKey) -> bool {
        if key.dimension != self.current_dimension {
            return false;
        }
        let Some(center) = self.publisher_center else {
            return true;
        };
        let radius = u64::try_from(self.active_radius_chunks()).unwrap_or(0);
        let center_x = center[0].div_euclid(16);
        let center_z = center[2].div_euclid(16);
        i64::from(key.x).abs_diff(i64::from(center_x)) <= radius
            && i64::from(key.z).abs_diff(i64::from(center_z)) <= radius
    }

    fn is_expected_sub_chunk(&self, key: SubChunkKey) -> bool {
        self.requested_sub_chunks
            .get(&key.chunk())
            .is_some_and(|expected| expected.contains_key(&key.y))
    }

    fn dispatch_mesh_jobs(&mut self, camera_position: [f32; 3], budget: usize) -> usize {
        let worker_budget = budget.min(WORK_RESULT_CAPACITY.saturating_sub(self.in_flight.len()));
        let mut candidates = self
            .pending_mesh
            .iter()
            .map(|(&key, &pending)| {
                (
                    distance_squared(key, camera_position),
                    key,
                    pending.revision,
                    pending.since,
                )
            })
            .collect::<Vec<_>>();
        candidates.sort_by(|left, right| {
            left.0
                .total_cmp(&right.0)
                .then_with(|| left.1.cmp(&right.1))
        });

        let mut dispatched = 0;
        for (_, key, revision, dirty_since) in candidates {
            if self.mesh_changes.len() >= MAX_PENDING_MESH_CHANGES {
                break;
            }
            if !self.revisions.is_current(key, revision) {
                continue;
            }
            let Some(center) = self.store.sub_chunk(key) else {
                self.pending_mesh.remove(&key);
                if self.known_air.contains(&key) {
                    self.set_connectivity(key, Some(FaceConnectivity::all()));
                    let registered = self.register_mesh_dependency_mask(
                        key,
                        revision,
                        MeshDependencyMask::default(),
                    );
                    debug_assert!(registered);
                } else {
                    self.set_connectivity(key, None);
                    self.mesh_dependency_masks.remove(&key);
                }
                self.mesh_changes.push_back(WorldMeshChange::Remove {
                    key,
                    generation: revision,
                    dirty_since,
                });
                continue;
            };
            if dispatched >= worker_budget || self.in_flight.contains_key(&key) {
                continue;
            }
            let snapshot = self.mesh_snapshot(key, center);
            self.pending_mesh.remove(&key);
            self.in_flight.insert(key, revision);
            let tx = self.mesh_tx.clone();
            let classifier = self.classifier;
            let network_id_mode = self.network_id_mode;
            let runtime_assets = Arc::clone(&self.runtime_assets);
            let resolved_biome_tints = Arc::clone(&self.resolved_biome_tints);
            let tint_identity = self.biome_tint_identity();
            rayon::spawn(move || {
                let started = Instant::now();
                let source = Arc::clone(&snapshot.center);
                let biome_source = snapshot.biome.clone();
                let biome = pack_biome_record(biome_source.as_deref(), &resolved_biome_tints);
                let mesh = snapshot.mesh(classifier, &runtime_assets, network_id_mode);
                let dependency_mask =
                    snapshot.dependency_mask(classifier, &runtime_assets, network_id_mode);
                let _ = tx.send(MeshCompletion {
                    key,
                    revision,
                    source,
                    biome_source,
                    biome,
                    tint_identity,
                    mesh,
                    dependency_mask,
                    duration: started.elapsed(),
                });
            });
            self.stats.last_mesh_dispatch_at = Some(Instant::now());
            dispatched += 1;
        }
        dispatched
    }

    fn mesh_snapshot(&self, key: SubChunkKey, center: Arc<SubChunk>) -> MeshSnapshot {
        let mut adjacent = std::array::from_fn(|_| None);
        for offset @ [dx, dy, dz] in MeshNeighbourhood::adjacent_offsets() {
            let neighbour = key
                .x
                .checked_add(i32::from(dx))
                .zip(key.y.checked_add(i32::from(dy)))
                .zip(key.z.checked_add(i32::from(dz)))
                .and_then(|((x, y), z)| {
                    self.store
                        .sub_chunk(SubChunkKey::new(key.dimension, x, y, z))
                });
            adjacent[mesh_offset_index(offset)] = neighbour;
        }
        MeshSnapshot {
            center,
            biome: self.store.biome_storage(key),
            adjacent,
        }
    }

    fn accept_mesh_completion(&mut self, completion: MeshCompletion) {
        if self.in_flight.get(&completion.key) == Some(&completion.revision) {
            self.in_flight.remove(&completion.key);
        }
        let source_is_current = self
            .store
            .sub_chunk(completion.key)
            .is_some_and(|current| Arc::ptr_eq(&current, &completion.source));
        let current_biome = self.store.biome_storage(completion.key);
        let biome_source_is_current = match (&completion.biome_source, &current_biome) {
            (Some(completed), Some(current)) => Arc::ptr_eq(completed, current),
            (None, None) => true,
            _ => false,
        };
        if !self
            .revisions
            .is_current(completion.key, completion.revision)
            || !source_is_current
            || !biome_source_is_current
            || completion.tint_identity != self.biome_tint_identity()
        {
            self.stats.stale_mesh_jobs = self.stats.stale_mesh_jobs.saturating_add(1);
            return;
        }
        self.stats.max_mesh_duration = self.stats.max_mesh_duration.max(completion.duration);
        self.stats.last_mesh_completion_at = Some(Instant::now());
        let dirty = self
            .revisions
            .dirty(completion.key)
            .expect("current mesh completion has a dirty revision");
        self.set_connectivity(completion.key, Some(completion.mesh.connectivity()));
        if self.resident.contains(&completion.key) {
            let registered = self.register_mesh_dependency_mask(
                completion.key,
                completion.revision,
                completion.dependency_mask,
            );
            debug_assert!(registered);
        }
        self.mesh_changes.push_back(WorldMeshChange::Upsert {
            key: completion.key,
            mesh: completion.mesh,
            biome: completion.biome,
            tint_identity: completion.tint_identity,
            generation: completion.revision,
            dirty_since: dirty.since,
        });
    }

    fn complete_requested_sub_chunk(&mut self, key: SubChunkKey) {
        self.cancel_sub_chunk_retry(key);
        let chunk = key.chunk();
        let (removed, completed) =
            self.requested_sub_chunks
                .get_mut(&chunk)
                .map_or((None, false), |expected| {
                    let removed = expected.remove(&key.y);
                    (removed, expected.is_empty())
                });
        if let Some(pending) = removed
            && (pending.pending_transport_attempts != 0 || pending.confirmed_attempts != 0)
        {
            self.correlated_sub_chunk_attempts.insert(
                key,
                CorrelatedSubChunkAttempts {
                    pending_transport_attempts: pending.pending_transport_attempts,
                    confirmed_attempts: pending.confirmed_attempts,
                },
            );
        }
        if completed {
            self.requested_sub_chunks.remove(&chunk);
            self.loaded_columns.insert(chunk);
        }
    }

    fn consume_confirmed_sub_chunk_attempt(&mut self, key: SubChunkKey) {
        let Some(pending) = self
            .requested_sub_chunks
            .get_mut(&key.chunk())
            .and_then(|column| column.get_mut(&key.y))
        else {
            return;
        };
        pending.confirmed_attempts = pending.confirmed_attempts.saturating_sub(1);
    }

    fn record_sub_chunk_reply_admissions(&mut self, batch: &SubChunkBatchEvent) {
        for entry in &batch.entries {
            let key = SubChunkKey::new(
                batch.dimension,
                entry.position[0],
                entry.position[1],
                entry.position[2],
            );
            if !self.column_is_active(key.chunk()) {
                continue;
            }
            let expected = self.is_expected_sub_chunk(key);
            let available = self
                .requested_sub_chunks
                .get(&key.chunk())
                .and_then(|column| column.get(&key.y))
                .map_or_else(
                    || {
                        self.correlated_sub_chunk_attempts
                            .get(&key)
                            .map_or(0, |attempts| attempts.confirmed_attempts)
                    },
                    |pending| pending.confirmed_attempts.max(1),
                );
            let admitted = self
                .admitted_sub_chunk_replies
                .get(&key)
                .copied()
                .unwrap_or(0);
            if admitted < available {
                if expected {
                    self.cancel_sub_chunk_retry(key);
                }
                self.admitted_sub_chunk_replies
                    .insert(key, admitted.saturating_add(1));
            }
        }
    }

    fn consume_admitted_sub_chunk_reply(&mut self, key: SubChunkKey) -> bool {
        let Some(admitted) = self.admitted_sub_chunk_replies.get_mut(&key) else {
            return false;
        };
        *admitted = admitted.saturating_sub(1);
        if *admitted == 0 {
            self.admitted_sub_chunk_replies.remove(&key);
        }
        true
    }

    fn consume_correlated_sub_chunk_attempt(&mut self, key: SubChunkKey) -> bool {
        let Some(attempts) = self.correlated_sub_chunk_attempts.get_mut(&key) else {
            return false;
        };
        if attempts.confirmed_attempts == 0 {
            return false;
        }
        attempts.confirmed_attempts = attempts.confirmed_attempts.saturating_sub(1);
        if attempts.confirmed_attempts == 0 && attempts.pending_transport_attempts == 0 {
            self.correlated_sub_chunk_attempts.remove(&key);
        }
        true
    }

    fn retry_or_complete_sub_chunk(&mut self, key: SubChunkKey) -> bool {
        if self.retry_is_queued(key) {
            return false;
        }
        let attempts = self
            .requested_sub_chunks
            .get(&key.chunk())
            .and_then(|column| column.get(&key.y))
            .map_or(0, |pending| pending.retry_attempts);
        if attempts >= MAX_SUB_CHUNK_RETRIES {
            self.stats.sub_chunk_retry_exhaustions =
                self.stats.sub_chunk_retry_exhaustions.saturating_add(1);
            return true;
        }
        match self.try_schedule_exact_retry(key) {
            RetrySchedule::Scheduled => {
                self.record_retry_scheduled(key);
                false
            }
            RetrySchedule::CapacityFull => {
                self.record_normalization_error(
                    NormalizationErrorReason::DeferredRetryCapacityFailure,
                );
                true
            }
            RetrySchedule::EncodingFailure => true,
        }
    }

    fn retry_is_queued(&self, key: SubChunkKey) -> bool {
        self.deferred_retry_set.contains(&key)
            || self.requests.iter().any(|slot| {
                matches!(slot, OutboundRequestSlot::Ready(request)
                    if request.chunk == key.chunk()
                        && request.base_sub_chunk_y == key.y
                        && request.count == 1)
            })
    }

    fn enqueue_exact_retry(&mut self, key: SubChunkKey) -> bool {
        let Ok(packet) = request_sub_chunk_column(key.dimension, key.x, key.z, key.y, 1) else {
            self.record_normalization_error(NormalizationErrorReason::RetryRequestEncodingFailure);
            return false;
        };
        self.place_outbound_request(
            None,
            PendingSubChunkRequest {
                packet,
                dimension: key.dimension,
                chunk: key.chunk(),
                base_sub_chunk_y: key.y,
                count: 1,
            },
        )
    }

    fn try_schedule_exact_retry(&mut self, key: SubChunkKey) -> RetrySchedule {
        if !self.deferred_retries.is_empty() && self.requests.len() < OUTBOUND_REQUEST_CAPACITY {
            self.pump_deferred_retries();
        }
        if !self.deferred_retries.is_empty() {
            if self.deferred_retries.len() >= DEFERRED_RETRY_CAPACITY {
                return RetrySchedule::CapacityFull;
            }
            self.deferred_retries.push_back(key);
            self.deferred_retry_set.insert(key);
            return RetrySchedule::Scheduled;
        }
        if self.requests.len() < OUTBOUND_REQUEST_CAPACITY {
            return if self.enqueue_exact_retry(key) {
                RetrySchedule::Scheduled
            } else {
                RetrySchedule::EncodingFailure
            };
        }
        if self.deferred_retries.len() < DEFERRED_RETRY_CAPACITY {
            self.deferred_retries.push_back(key);
            self.deferred_retry_set.insert(key);
            return RetrySchedule::Scheduled;
        }
        RetrySchedule::CapacityFull
    }

    fn record_retry_scheduled(&mut self, key: SubChunkKey) {
        let pending = self
            .requested_sub_chunks
            .get_mut(&key.chunk())
            .and_then(|column| column.get_mut(&key.y))
            .expect("only an expected SubChunk Y may schedule a retry");
        pending.retry_attempts = pending.retry_attempts.saturating_add(1);
        self.stats.sub_chunk_retries_scheduled =
            self.stats.sub_chunk_retries_scheduled.saturating_add(1);
    }

    fn expire_sub_chunk_deadlines(&mut self, now: Instant) {
        // Older deferred retries own newly free outbound slots. Expirations
        // observed in this pass must never bypass that FIFO.
        self.pump_deferred_retries();
        loop {
            let Some(&(deadline, key)) = self.sub_chunk_deadlines.first() else {
                break;
            };
            if deadline > now {
                break;
            }
            let Some(pending) = self
                .requested_sub_chunks
                .get(&key.chunk())
                .and_then(|column| column.get(&key.y))
                .copied()
            else {
                self.sub_chunk_deadlines.remove(&(deadline, key));
                continue;
            };
            if pending.response_deadline != Some(deadline) {
                self.sub_chunk_deadlines.remove(&(deadline, key));
                continue;
            }

            if pending.retry_attempts >= MAX_SUB_CHUNK_RETRIES {
                self.disarm_sub_chunk_deadline(key);
                self.stats.sub_chunk_timeouts = self.stats.sub_chunk_timeouts.saturating_add(1);
                self.stats.sub_chunk_retry_exhaustions =
                    self.stats.sub_chunk_retry_exhaustions.saturating_add(1);
                self.complete_requested_sub_chunk(key);
                continue;
            }

            match self.try_schedule_exact_retry(key) {
                RetrySchedule::Scheduled => {
                    self.disarm_sub_chunk_deadline(key);
                    self.stats.sub_chunk_timeouts = self.stats.sub_chunk_timeouts.saturating_add(1);
                    self.record_retry_scheduled(key);
                }
                RetrySchedule::CapacityFull => break,
                RetrySchedule::EncodingFailure => {
                    self.disarm_sub_chunk_deadline(key);
                    self.stats.sub_chunk_timeouts = self.stats.sub_chunk_timeouts.saturating_add(1);
                    self.complete_requested_sub_chunk(key);
                }
            }
        }
        debug_assert!(self.sub_chunk_deadlines.len() <= self.outstanding_sub_chunk_count());
    }

    fn pump_deferred_retries(&mut self) {
        while self.requests.len() < OUTBOUND_REQUEST_CAPACITY {
            let Some(key) = self.deferred_retries.pop_front() else {
                break;
            };
            self.deferred_retry_set.remove(&key);
            if !self.is_expected_sub_chunk(key) {
                continue;
            }
            if !self.enqueue_exact_retry(key) {
                self.complete_requested_sub_chunk(key);
            }
        }
    }

    fn cancel_sub_chunk_retry(&mut self, key: SubChunkKey) {
        self.disarm_sub_chunk_deadline(key);
        if self.deferred_retry_set.remove(&key) {
            self.deferred_retries.retain(|pending| *pending != key);
        }
        self.requests.retain(|slot| {
            !matches!(slot, OutboundRequestSlot::Ready(request)
                if request.chunk == key.chunk()
                    && request.base_sub_chunk_y == key.y
                    && request.count == 1)
        });
    }

    fn disarm_sub_chunk_deadline(&mut self, key: SubChunkKey) {
        let deadline = self
            .requested_sub_chunks
            .get_mut(&key.chunk())
            .and_then(|column| column.get_mut(&key.y))
            .and_then(|pending| pending.response_deadline.take());
        if let Some(deadline) = deadline {
            self.sub_chunk_deadlines.remove(&(deadline, key));
        }
    }

    fn purge_sub_chunk_column_state(&mut self, chunk: ChunkKey) {
        if let Some(pending) = self.requested_sub_chunks.remove(&chunk) {
            for (y, pending) in pending {
                if let Some(deadline) = pending.response_deadline {
                    self.sub_chunk_deadlines
                        .remove(&(deadline, SubChunkKey::from_chunk(chunk, y)));
                }
            }
        }
        self.requests.retain(|slot| match slot {
            OutboundRequestSlot::Reserved(_) => true,
            OutboundRequestSlot::Ready(request) => request.chunk != chunk,
        });
        self.deferred_retries
            .retain(|sub_chunk| sub_chunk.chunk() != chunk);
        self.deferred_retry_set
            .retain(|sub_chunk| sub_chunk.chunk() != chunk);
        self.correlated_sub_chunk_attempts
            .retain(|sub_chunk, _| sub_chunk.chunk() != chunk);
        self.admitted_sub_chunk_replies
            .retain(|sub_chunk, _| sub_chunk.chunk() != chunk);
    }

    fn queued_retry_request_count(&self) -> usize {
        let outbound = self
            .requests
            .iter()
            .filter(|slot| {
                let OutboundRequestSlot::Ready(request) = slot else {
                    return false;
                };
                request.count == 1
                    && self
                        .requested_sub_chunks
                        .get(&request.chunk)
                        .and_then(|column| column.get(&request.base_sub_chunk_y))
                        .is_some_and(|pending| pending.retry_attempts != 0)
            })
            .count();
        outbound.saturating_add(self.deferred_retries.len())
    }

    fn set_connectivity(&mut self, key: SubChunkKey, value: Option<FaceConnectivity>) {
        let changed = match value {
            Some(value) => self.connectivity.insert(key, value) != Some(value),
            None => self.connectivity.remove(&key).is_some(),
        };
        if changed {
            self.bump_connectivity_generation();
        }
    }

    fn bump_connectivity_generation(&mut self) {
        self.connectivity_generation = self.connectivity_generation.wrapping_add(1).max(1);
    }
}

fn prepare_sub_chunks(batch: SubChunkBatchEvent) -> Vec<PreparedSubChunk> {
    batch
        .entries
        .into_iter()
        .map(|entry| PreparedSubChunk {
            position: entry.position,
            result: match entry.result {
                SubChunkResult::Success { payload } => PreparedSubChunkResult::Decoded(
                    SubChunk::decode_prefix(&payload).map(|(sub_chunk, _)| sub_chunk),
                ),
                SubChunkResult::AllAir => PreparedSubChunkResult::AllAir,
                SubChunkResult::Unavailable(unavailable) => {
                    PreparedSubChunkResult::Unavailable(unavailable)
                }
            },
        })
        .collect()
}

fn distance_squared(key: SubChunkKey, camera: [f32; 3]) -> f32 {
    let center = [
        key.x as f32 * 16.0 + 8.0,
        key.y as f32 * 16.0 + 8.0,
        key.z as f32 * 16.0 + 8.0,
    ];
    let dx = center[0] - camera[0];
    let dy = center[1] - camera[1];
    let dz = center[2] - camera[2];
    dx.mul_add(dx, dy.mul_add(dy, dz * dz))
}

fn deterministic_sub_chunk_key_hash(keys: &BTreeSet<SubChunkKey>) -> u64 {
    const FNV_OFFSET_BASIS: u64 = 0xcbf2_9ce4_8422_2325;
    const FNV_PRIME: u64 = 0x0000_0100_0000_01b3;

    keys.iter()
        .flat_map(|key| [key.dimension, key.x, key.y, key.z])
        .flat_map(i32::to_le_bytes)
        .fold(FNV_OFFSET_BASIS, |hash, byte| {
            (hash ^ u64::from(byte)).wrapping_mul(FNV_PRIME)
        })
}

fn floor_to_i32(value: f32) -> i32 {
    if value.is_nan() {
        0
    } else if value <= i32::MIN as f32 {
        i32::MIN
    } else if value >= i32::MAX as f32 {
        i32::MAX
    } else {
        value.floor() as i32
    }
}

#[cfg(test)]
mod tests {
    use std::{
        collections::{BTreeMap, BTreeSet},
        sync::Arc,
        time::{Duration, Instant},
    };

    use assets::{
        BlockFlags, BlockVisual, CompiledAssets, CompiledBiomeAssets, Material, NO_ANIMATION,
        NO_MODEL_TEMPLATE, NetworkIdMode, RuntimeAssets, TextureArray, TextureMip, TexturePage,
        TextureRef, VisualKind, encode_blob,
    };
    use protocol::{
        BiomeDefinitionEvent, BiomeDefinitionsEvent, BlockUpdateEvent, ChangeDimensionEvent,
        LevelChunkEvent, LevelChunkMode, MovePlayerEvent, PublisherUpdateEvent, SubChunkBatchEvent,
        SubChunkEntryEvent, SubChunkResult, SubChunkUnavailable, WorldBootstrap, WorldEvent,
    };
    use render::{BlockClassifier, Neighbourhood, PackedBiomeRecord, mesh_sub_chunk};
    use world::{
        BlockUpdate, ChunkKey, ChunkStore, DecodedBiomeColumn, DecodedLevelChunk,
        MeshDependencyMask, SubChunk, SubChunkKey,
    };

    use super::{MeshCompletion, RevisionTracker, SequenceBuffer, WorldStream, split_block_update};

    mod mesh_dependency {
        use std::time::Instant;

        use protocol::WorldBootstrap;
        use world::{DecodedLevelChunk, MeshDependencyMask, SubChunkKey};

        use super::WorldStream;

        fn stream() -> WorldStream {
            WorldStream::new(WorldBootstrap {
                dimension: 0,
                local_player_runtime_id: 1,
                player_position: [0.0; 3],
                world_spawn_position: [0; 3],
                air_network_id: 12_530,
                block_network_ids_are_hashes: false,
            })
        }

        #[test]
        fn diagonal_change_invalidates_ao_dependents() {
            let mut stream = stream();
            let source = SubChunkKey::new(0, 0, 0, 0);
            let dependent = SubChunkKey::new(0, 1, 1, 0);
            stream.resident.insert(dependent);
            let generation = stream.mark_dirty_exact(dependent, Instant::now());
            assert!(stream.register_mesh_dependency_mask(
                dependent,
                generation,
                MeshDependencyMask::new(true, false),
            ));
            stream.pending_mesh.clear();

            stream.mark_changed(source, Instant::now());

            assert_ne!(
                stream.revisions.dirty(dependent).unwrap().revision,
                generation
            );
            assert!(stream.pending_mesh.contains_key(&dependent));
        }

        #[test]
        fn horizontal_corner_change_invalidates_liquid_dependent() {
            let mut stream = stream();
            let source = SubChunkKey::new(0, 0, 0, 0);
            let dependent = SubChunkKey::new(0, 1, 0, 1);
            stream.resident.insert(dependent);
            let generation = stream.mark_dirty_exact(dependent, Instant::now());
            assert!(stream.register_mesh_dependency_mask(
                dependent,
                generation,
                MeshDependencyMask::new(false, true),
            ));
            stream.pending_mesh.clear();

            stream.mark_changed(source, Instant::now());

            assert_ne!(
                stream.revisions.dirty(dependent).unwrap().revision,
                generation
            );
            assert!(stream.pending_mesh.contains_key(&dependent));
        }

        #[test]
        fn liquid_dependency_skips_vertical_corner_outside_sample_set() {
            let mut stream = stream();
            let source = SubChunkKey::new(0, 0, 0, 0);
            let outside = SubChunkKey::new(0, 1, 1, 1);
            stream.resident.insert(outside);
            let generation = stream.mark_dirty_exact(outside, Instant::now());
            assert!(stream.register_mesh_dependency_mask(
                outside,
                generation,
                MeshDependencyMask::new(false, true),
            ));
            stream.pending_mesh.clear();

            stream.mark_changed(source, Instant::now());

            assert_eq!(
                stream.revisions.dirty(outside).unwrap().revision,
                generation
            );
            assert!(!stream.pending_mesh.contains_key(&outside));
        }

        #[test]
        fn face_only_target_skips_diagonal_but_face_neighbour_still_dirties() {
            let mut stream = stream();
            let source = SubChunkKey::new(0, 0, 0, 0);
            let diagonal = SubChunkKey::new(0, 1, 0, 1);
            let face = SubChunkKey::new(0, 1, 0, 0);
            for target in [diagonal, face] {
                stream.resident.insert(target);
                let generation = stream.mark_dirty_exact(target, Instant::now());
                assert!(stream.register_mesh_dependency_mask(
                    target,
                    generation,
                    MeshDependencyMask::default(),
                ));
            }
            let diagonal_generation = stream.revisions.dirty(diagonal).unwrap().revision;
            let face_generation = stream.revisions.dirty(face).unwrap().revision;
            stream.pending_mesh.clear();

            stream.mark_changed(source, Instant::now());

            assert_eq!(
                stream.revisions.dirty(diagonal).unwrap().revision,
                diagonal_generation
            );
            assert!(!stream.pending_mesh.contains_key(&diagonal));
            assert_ne!(
                stream.revisions.dirty(face).unwrap().revision,
                face_generation
            );
            assert!(stream.pending_mesh.contains_key(&face));
        }

        #[test]
        fn rapid_liquid_changes_coalesce_latest_generation_and_oldest_since() {
            let mut stream = stream();
            let source = SubChunkKey::new(0, 0, 0, 0);
            let dependent = SubChunkKey::new(0, 1, 0, 1);
            stream.resident.insert(dependent);
            let registered_at = Instant::now();
            let registered = stream.mark_dirty_exact(dependent, registered_at);
            assert!(stream.register_mesh_dependency_mask(
                dependent,
                registered,
                MeshDependencyMask::new(false, true),
            ));
            stream.pending_mesh.clear();
            let first_at = Instant::now();

            let before_first = stream.revisions.next_revision;
            stream.mark_changed_sources([source, source], first_at);
            assert_eq!(
                stream.revisions.next_revision - before_first,
                8,
                "duplicate sources must assign one revision per deduplicated dirty target"
            );
            let first = stream.pending_mesh[&dependent];
            let second_at = first_at + std::time::Duration::from_millis(5);
            let before_second = stream.revisions.next_revision;
            stream.mark_changed_sources([source, source], second_at);
            assert_eq!(
                stream.revisions.next_revision - before_second,
                8,
                "each rapid batch must revise every deduplicated dirty target exactly once"
            );
            let second = stream.pending_mesh[&dependent];

            assert_ne!(first.revision, second.revision);
            assert_eq!(
                second.revision,
                stream.revisions.dirty(dependent).unwrap().revision
            );
            assert_eq!(first.since, registered_at);
            assert_eq!(second.since, registered_at);
        }

        #[test]
        fn known_empty_mask_skips_diagonal_change() {
            let mut stream = stream();
            let source = SubChunkKey::new(0, 0, 0, 0);
            let diagonal = SubChunkKey::new(0, 1, 1, 0);
            stream.resident.insert(diagonal);
            let generation = stream.mark_dirty_exact(diagonal, Instant::now());
            assert!(stream.register_mesh_dependency_mask(
                diagonal,
                generation,
                MeshDependencyMask::default(),
            ));
            stream.pending_mesh.clear();

            stream.mark_changed(source, Instant::now());

            assert_eq!(
                stream.revisions.dirty(diagonal).unwrap().revision,
                generation
            );
            assert!(!stream.pending_mesh.contains_key(&diagonal));
        }

        #[test]
        fn unknown_new_mask_dirties_diagonal_conservatively() {
            let mut stream = stream();
            let source = SubChunkKey::new(0, 0, 0, 0);
            let diagonal = SubChunkKey::new(0, 1, 1, 0);
            stream.resident.insert(diagonal);

            stream.mark_changed(source, Instant::now());

            assert!(stream.pending_mesh.contains_key(&diagonal));
        }

        #[test]
        fn inline_full_column_change_invalidates_registered_corner_dependency() {
            let mut stream = stream();
            let source = SubChunkKey::new(0, 0, -4, 0);
            let corner = SubChunkKey::new(0, 1, -4, 1);
            let decoded = DecodedLevelChunk::decode(
                source.y,
                1,
                include_bytes!("../../crates/world/fixtures/uniform_non_air.bin"),
            )
            .unwrap();
            stream.store.commit_level_chunk(source.chunk(), decoded);
            stream.resident.insert(source);
            stream.resident.insert(corner);
            let generation = stream.mark_dirty_exact(corner, Instant::now());
            assert!(stream.register_mesh_dependency_mask(
                corner,
                generation,
                MeshDependencyMask::new(true, false),
            ));
            stream.pending_mesh.clear();

            stream.submit(1, super::inline_air_event(0)).unwrap();
            super::complete_pending_decode_jobs(&mut stream);

            assert_ne!(stream.revisions.dirty(corner).unwrap().revision, generation);
            assert!(stream.pending_mesh.contains_key(&corner));
            assert_eq!(
                stream.revisions.next_revision - generation,
                stream.pending_mesh.len() as u64,
                "one inline batch must assign exactly one revision per dirty target"
            );
        }

        #[test]
        fn known_air_removal_replaces_stale_mask_and_skips_later_diagonal_change() {
            let mut stream = stream();
            let target = SubChunkKey::new(0, 1, -4, 0);
            let diagonal_source = SubChunkKey::new(0, 0, -4, 1);
            let decoded = DecodedLevelChunk::decode(
                target.y,
                1,
                include_bytes!("../../crates/world/fixtures/uniform_non_air.bin"),
            )
            .unwrap();
            stream.store.commit_level_chunk(target.chunk(), decoded);
            stream.resident.insert(target);
            let stale_generation = stream.mark_dirty_exact(target, Instant::now());
            assert!(stream.register_mesh_dependency_mask(
                target,
                stale_generation,
                MeshDependencyMask::new(true, true),
            ));
            stream.pending_mesh.clear();

            stream.submit(1, super::inline_air_event(target.x)).unwrap();
            super::complete_pending_decode_jobs(&mut stream);

            assert!(stream.known_air.contains(&target));
            assert_eq!(
                stream.mesh_dependency_mask(target),
                None,
                "transitioning to known air must clear the stale non-empty mask"
            );
            let empty_generation = stream.revisions.dirty(target).unwrap().revision;
            stream.dispatch_mesh_jobs([24.0, -56.0, 8.0], 0);
            assert_eq!(
                stream.mesh_dependency_mask(target),
                Some((empty_generation, MeshDependencyMask::default())),
                "the exact queued removal generation must register known-empty dependencies"
            );

            stream.mark_changed(diagonal_source, Instant::now());

            assert_eq!(
                stream.revisions.dirty(target).unwrap().revision,
                empty_generation
            );
            assert!(!stream.pending_mesh.contains_key(&target));
        }

        #[test]
        fn mask_generation_replacement() {
            let mut stream = stream();
            let key = SubChunkKey::new(0, 3, 4, 5);
            stream.resident.insert(key);
            let first = stream.mark_dirty_exact(key, Instant::now());
            assert!(stream.register_mesh_dependency_mask(
                key,
                first,
                MeshDependencyMask::new(false, false),
            ));
            let second = stream.mark_dirty_exact(key, Instant::now());
            assert!(stream.register_mesh_dependency_mask(
                key,
                second,
                MeshDependencyMask::new(true, false),
            ));

            assert_eq!(
                stream.mesh_dependency_mask(key),
                Some((second, MeshDependencyMask::new(true, false)))
            );
        }

        #[test]
        fn stale_mask_rejection() {
            let mut stream = stream();
            let key = SubChunkKey::new(0, 3, 4, 5);
            stream.resident.insert(key);
            let stale = stream.mark_dirty_exact(key, Instant::now());
            let current = stream.mark_dirty_exact(key, Instant::now());

            assert!(!stream.register_mesh_dependency_mask(
                key,
                stale,
                MeshDependencyMask::new(true, true),
            ));
            assert_eq!(stream.mesh_dependency_mask(key), None);
            assert!(stream.register_mesh_dependency_mask(
                key,
                current,
                MeshDependencyMask::new(false, true),
            ));
        }

        #[test]
        fn private_snapshot_populates_the_shared_liquid_neighbourhood() {
            let mut stream = stream();
            let center_key = SubChunkKey::new(0, 20, 7, -30);
            for (index, [dx, dy, dz]) in
                world::MeshNeighbourhood::liquid_sample_offsets().enumerate()
            {
                let key = SubChunkKey::new(
                    center_key.dimension,
                    center_key.x + i32::from(dx),
                    center_key.y + i32::from(dy),
                    center_key.z + i32::from(dz),
                );
                stream
                    .store
                    .commit_sub_chunk(key, super::uniform_sub_chunk(100 + index as u32))
                    .unwrap();
            }
            let center = stream.store.sub_chunk(center_key).unwrap();

            let snapshot = stream.mesh_snapshot(center_key, center);
            let neighbourhood = snapshot.neighbourhood();
            let liquid = neighbourhood.liquid_sub_chunks().collect::<Vec<_>>();

            assert_eq!(liquid.len(), 23);
            assert!(liquid.iter().all(|(_, sub_chunk)| sub_chunk.is_some()));
            for (index, (_, sub_chunk)) in liquid.into_iter().enumerate() {
                assert_eq!(
                    sub_chunk.unwrap().runtime_id(0, 0, 0, 0),
                    Some(100 + index as u32)
                );
            }
            assert!(neighbourhood.sub_chunk([1, -1, 1]).is_none());
        }
    }

    #[test]
    fn biome_definition_snapshot_commits_in_fifo_and_survives_dimension_changes() {
        let mut stream = WorldStream::new(WorldBootstrap {
            dimension: 0,
            local_player_runtime_id: 1,
            player_position: [0.0; 3],
            world_spawn_position: [0; 3],
            air_network_id: 12_530,
            block_network_ids_are_hashes: false,
        });
        let definitions: Arc<[BiomeDefinitionEvent]> = Arc::from(vec![BiomeDefinitionEvent {
            biome_id: None,
            name: Arc::from("minecraft:plains"),
            temperature: 0.8,
            downfall: 0.4,
            snow_foliage: 0.0,
            map_water_color: 0xff44_6688,
        }]);

        assert!(stream.biome_definitions_snapshot().is_empty());
        stream
            .submit(
                2,
                WorldEvent::BiomeDefinitions(BiomeDefinitionsEvent {
                    definitions: Arc::clone(&definitions),
                }),
            )
            .unwrap();
        assert!(
            stream.biome_definitions_snapshot().is_empty(),
            "sequence two must wait for sequence one"
        );

        stream.submit(1, WorldEvent::ChunkRadiusUpdated(8)).unwrap();
        let committed = stream.biome_definitions_snapshot();
        assert!(Arc::ptr_eq(&committed, &definitions));

        stream
            .submit(
                3,
                WorldEvent::ChangeDimension(ChangeDimensionEvent {
                    dimension: 1,
                    position: [0.0, 64.0, 0.0],
                }),
            )
            .unwrap();
        let after_dimension_change = stream.biome_definitions_snapshot();
        assert!(Arc::ptr_eq(&after_dimension_change, &definitions));
    }

    #[test]
    fn stale_biome_definition_event_cannot_replace_the_committed_snapshot() {
        let mut stream = WorldStream::new(WorldBootstrap {
            dimension: 0,
            local_player_runtime_id: 1,
            player_position: [0.0; 3],
            world_spawn_position: [0; 3],
            air_network_id: 12_530,
            block_network_ids_are_hashes: false,
        });
        let committed: Arc<[BiomeDefinitionEvent]> = Arc::from(vec![BiomeDefinitionEvent {
            biome_id: None,
            name: Arc::from("minecraft:plains"),
            temperature: 0.8,
            downfall: 0.4,
            snow_foliage: 0.0,
            map_water_color: 0xff44_6688,
        }]);
        stream
            .submit(
                1,
                WorldEvent::BiomeDefinitions(BiomeDefinitionsEvent {
                    definitions: Arc::clone(&committed),
                }),
            )
            .unwrap();

        let stale: Arc<[BiomeDefinitionEvent]> = Arc::from(vec![BiomeDefinitionEvent {
            biome_id: Some(600),
            name: Arc::from("example:stale"),
            temperature: 0.0,
            downfall: 0.0,
            snow_foliage: 0.0,
            map_water_color: 0,
        }]);
        let error = stream
            .submit(
                1,
                WorldEvent::BiomeDefinitions(BiomeDefinitionsEvent { definitions: stale }),
            )
            .unwrap_err();

        assert!(matches!(
            error,
            super::WorldStreamError::DuplicateOrPast {
                sequence: 1,
                next: 2
            }
        ));
        assert!(Arc::ptr_eq(
            &stream.biome_definitions_snapshot(),
            &committed
        ));
    }

    #[test]
    fn live_biome_resolution_commits_in_fifo_with_exact_raw_id_lookup() {
        let mut stream = WorldStream::new(WorldBootstrap {
            dimension: 0,
            local_player_runtime_id: 1,
            player_position: [0.0; 3],
            world_spawn_position: [0; 3],
            air_network_id: 12_530,
            block_network_ids_are_hashes: false,
        });
        let definitions: Arc<[BiomeDefinitionEvent]> = Arc::from([BiomeDefinitionEvent {
            biome_id: Some(0xfffe),
            name: Arc::from("example:high"),
            temperature: 0.8,
            downfall: 0.4,
            snow_foliage: 0.0,
            map_water_color: 0xff44_6688,
        }]);

        assert_eq!(stream.biome_tint_revision(), 0);
        assert_eq!(
            stream.resolved_biome_tints_snapshot().dense_index(0xfffe),
            0
        );
        stream
            .submit(
                2,
                WorldEvent::BiomeDefinitions(BiomeDefinitionsEvent {
                    definitions: Arc::clone(&definitions),
                }),
            )
            .unwrap();
        assert_eq!(stream.biome_tint_revision(), 0);

        stream.submit(1, WorldEvent::ChunkRadiusUpdated(8)).unwrap();
        let resolved = stream.resolved_biome_tints_snapshot();
        assert_eq!(stream.biome_tint_revision(), 1);
        assert_ne!(resolved.dense_index(0xfffe), 0);
        assert_eq!(resolved.dense_index(123), 0);
        assert!(Arc::ptr_eq(
            &stream.biome_definitions_snapshot(),
            &definitions
        ));
    }

    #[test]
    fn biome_tint_revision_overflow_keeps_the_previous_atomic_snapshot() {
        let mut stream = WorldStream::new(WorldBootstrap {
            dimension: 0,
            local_player_runtime_id: 1,
            player_position: [0.0; 3],
            world_spawn_position: [0; 3],
            air_network_id: 12_530,
            block_network_ids_are_hashes: false,
        });
        stream.biome_tint_revision = u64::MAX;
        let previous = stream.resolved_biome_tints_snapshot();

        stream
            .submit(
                1,
                WorldEvent::BiomeDefinitions(BiomeDefinitionsEvent {
                    definitions: Arc::from([BiomeDefinitionEvent {
                        biome_id: Some(42),
                        name: Arc::from("example:overflow"),
                        temperature: 0.8,
                        downfall: 0.4,
                        snow_foliage: 0.0,
                        map_water_color: 0xff44_6688,
                    }]),
                }),
            )
            .unwrap();

        assert_eq!(stream.biome_tint_revision(), u64::MAX);
        assert!(stream.biome_definitions_snapshot().is_empty());
        assert!(Arc::ptr_eq(
            &previous,
            &stream.resolved_biome_tints_snapshot()
        ));
        assert_eq!(
            stream
                .stats()
                .normalization_reasons
                .biome_tint_revision_overflows,
            1
        );
    }

    #[test]
    fn palette_native_biome_packing_uses_exact_lookup_and_safe_fallbacks() {
        let mut stream = WorldStream::new(WorldBootstrap {
            dimension: 0,
            local_player_runtime_id: 1,
            player_position: [0.0; 3],
            world_spawn_position: [0; 3],
            air_network_id: 12_530,
            block_network_ids_are_hashes: false,
        });
        stream
            .submit(
                1,
                WorldEvent::BiomeDefinitions(BiomeDefinitionsEvent {
                    definitions: Arc::from([BiomeDefinitionEvent {
                        biome_id: Some(42),
                        name: Arc::from("example:resolved"),
                        temperature: 0.8,
                        downfall: 0.4,
                        snow_foliage: 0.0,
                        map_water_color: 0xff44_6688,
                    }]),
                }),
            )
            .unwrap();
        let key = SubChunkKey::new(0, 0, -4, 0);
        stream.store.commit_biome_column(
            key.chunk(),
            DecodedBiomeColumn::decode(-4, 1, &[1, 84]).unwrap(),
        );
        let resolved_storage = stream.store.biome_storage(key).unwrap();
        let resolved = stream.resolved_biome_tints_snapshot();

        let packed = super::pack_biome_record(Some(resolved_storage.as_ref()), &resolved);
        assert_eq!(packed.tint_index(0, 0, 0), Some(resolved.dense_index(42)));

        stream.store.commit_biome_column(
            key.chunk(),
            DecodedBiomeColumn::decode(-4, 1, &[1, 86]).unwrap(),
        );
        let missing_storage = stream.store.biome_storage(key).unwrap();
        let missing = super::pack_biome_record(Some(missing_storage.as_ref()), &resolved);
        assert_eq!(missing.tint_index(0, 0, 0), Some(0));

        let absent = super::pack_biome_record(None, &resolved);
        assert_eq!(absent.tint_index(0, 0, 0), Some(0));
    }

    #[test]
    fn definition_replacement_supersedes_queued_and_in_flight_old_tints() {
        let mut stream = WorldStream::new(WorldBootstrap {
            dimension: 0,
            local_player_runtime_id: 1,
            player_position: [0.0; 3],
            world_spawn_position: [0; 3],
            air_network_id: 12_530,
            block_network_ids_are_hashes: false,
        });
        let definition = |name: &'static str, temperature| BiomeDefinitionEvent {
            biome_id: Some(42),
            name: Arc::from(name),
            temperature,
            downfall: 0.4,
            snow_foliage: 0.0,
            map_water_color: 0xff44_6688,
        };
        stream
            .submit(
                1,
                WorldEvent::BiomeDefinitions(BiomeDefinitionsEvent {
                    definitions: Arc::from([definition("example:old", 0.8)]),
                }),
            )
            .unwrap();

        let key = SubChunkKey::new(0, 0, -4, 0);
        stream.store.commit_level_chunk(
            key.chunk(),
            DecodedLevelChunk::decode(
                -4,
                1,
                include_bytes!("../../crates/world/fixtures/uniform_non_air.bin"),
            )
            .unwrap(),
        );
        stream.store.commit_biome_column(
            key.chunk(),
            DecodedBiomeColumn::decode(-4, 1, &[1, 84]).unwrap(),
        );
        stream.resident.insert(key);
        let source = stream.store.sub_chunk(key).unwrap();
        let biome_source = stream.store.biome_storage(key).unwrap();
        let old_generation = stream.revisions.mark_dirty(key, Instant::now());
        stream.in_flight.insert(key, old_generation);
        let old_tint_identity = stream.biome_tint_identity();
        let old_tint_revision = old_tint_identity.revision();
        let old_resolved = stream.resolved_biome_tints_snapshot();
        let queued_mesh = mesh_sub_chunk(
            &stream.classifier,
            &stream.runtime_assets,
            stream.network_id_mode,
            &Neighbourhood::empty(),
            &source,
        );
        let in_flight_mesh = mesh_sub_chunk(
            &stream.classifier,
            &stream.runtime_assets,
            stream.network_id_mode,
            &Neighbourhood::empty(),
            &source,
        );

        stream.accept_mesh_completion(MeshCompletion {
            key,
            revision: old_generation,
            source: Arc::clone(&source),
            biome_source: Some(Arc::clone(&biome_source)),
            biome: super::pack_biome_record(Some(&biome_source), &old_resolved),
            tint_identity: old_tint_identity,
            mesh: queued_mesh,
            dependency_mask: MeshDependencyMask::default(),
            duration: Duration::ZERO,
        });
        assert_eq!(stream.pending_mesh_change_count(), 1);
        stream.in_flight.insert(key, old_generation);

        stream
            .submit(
                2,
                WorldEvent::BiomeDefinitions(BiomeDefinitionsEvent {
                    definitions: Arc::from([definition("example:new", 0.2)]),
                }),
            )
            .unwrap();

        assert_eq!(stream.biome_tint_revision(), old_tint_revision + 1);
        assert_eq!(stream.pending_mesh_change_count(), 0);
        assert!(!stream.in_flight.contains_key(&key));
        assert!(stream.pending_mesh.contains_key(&key));
        assert!(!stream.revisions.is_current(key, old_generation));

        stream.accept_mesh_completion(MeshCompletion {
            key,
            revision: old_generation,
            source,
            biome_source: Some(biome_source),
            biome: super::pack_biome_record(None, &old_resolved),
            tint_identity: old_tint_identity,
            mesh: in_flight_mesh,
            dependency_mask: MeshDependencyMask::default(),
            duration: Duration::ZERO,
        });
        assert_eq!(stream.stats().stale_mesh_jobs, 1);
        assert!(stream.pop_mesh_change().is_none());
    }

    #[test]
    fn network_mode_and_runtime_assets_are_selected_once_per_stream() {
        let runtime_assets = Arc::new(RuntimeAssets::diagnostic());
        let stream = WorldStream::new_with_assets(
            WorldBootstrap {
                dimension: 0,
                local_player_runtime_id: 1,
                player_position: [0.0; 3],
                world_spawn_position: [0; 3],
                air_network_id: 0xdbf4_4120,
                block_network_ids_are_hashes: true,
            },
            Arc::clone(&runtime_assets),
            [0.0, crate::server_position::SAFE_SERVER_HEIGHT, 0.0],
            None,
        );

        assert_eq!(stream.network_id_mode, NetworkIdMode::Hashed);
        assert!(Arc::ptr_eq(&stream.runtime_assets, &runtime_assets));
    }

    #[test]
    fn render_mesh_api_consumes_only_the_shared_world_neighbourhood() {
        let _: for<'a, 'b, 'c, 'd> fn(
            &'a BlockClassifier,
            &'b RuntimeAssets,
            NetworkIdMode,
            &'c world::MeshNeighbourhood<'d>,
        ) -> render::ChunkMesh = render::mesh_sub_chunk_in_neighbourhood;
    }

    fn zig_zag_i32(value: i32) -> Vec<u8> {
        let mut value = ((value << 1) ^ (value >> 31)) as u32;
        let mut encoded = Vec::new();
        loop {
            let mut byte = (value & 0x7f) as u8;
            value >>= 7;
            if value != 0 {
                byte |= 0x80;
            }
            encoded.push(byte);
            if value == 0 {
                return encoded;
            }
        }
    }

    fn uniform_sub_chunk(runtime_id: u32) -> SubChunk {
        let mut bytes = vec![8, 1, 1];
        bytes.extend(zig_zag_i32(runtime_id as i32));
        SubChunk::decode(&bytes).expect("decode uniform test subchunk")
    }

    fn biome_payload(dimension: i32, biome_id: i32) -> Vec<u8> {
        let storage_count = protocol::vanilla_dimension_range(dimension)
            .expect("test dimension should have a vanilla range")
            .sub_chunk_count;
        let mut payload = vec![1];
        payload.extend(zig_zag_i32(biome_id));
        payload.extend(std::iter::repeat_n(0xff, storage_count - 1));
        payload.push(0); // border-block count
        payload
    }

    fn request_level_chunk_event(
        dimension: i32,
        x: i32,
        z: i32,
        mode: LevelChunkMode,
        biome_id: i32,
    ) -> WorldEvent {
        WorldEvent::LevelChunk(LevelChunkEvent {
            dimension,
            x,
            z,
            mode,
            payload: biome_payload(dimension, biome_id),
        })
    }

    fn inline_air_event(x: i32) -> WorldEvent {
        let mut payload = vec![9, 0, (-4_i8) as u8];
        payload.extend(biome_payload(0, 1));
        WorldEvent::LevelChunk(LevelChunkEvent {
            dimension: 0,
            x,
            z: 0,
            mode: LevelChunkMode::Inline { count: 1 },
            payload,
        })
    }

    #[test]
    fn inline_level_chunk_decodes_full_dimension_biomes_independent_of_block_count() {
        let mut stream = WorldStream::new(WorldBootstrap {
            dimension: 0,
            local_player_runtime_id: 1,
            player_position: [0.0; 3],
            world_spawn_position: [0; 3],
            air_network_id: 12_530,
            block_network_ids_are_hashes: false,
        });
        let mut payload = vec![9, 0, (-4_i8) as u8];
        payload.extend(biome_payload(0, 7));

        stream
            .submit(
                1,
                WorldEvent::LevelChunk(LevelChunkEvent {
                    dimension: 0,
                    x: 0,
                    z: 0,
                    mode: LevelChunkMode::Inline { count: 1 },
                    payload,
                }),
            )
            .unwrap();
        complete_pending_decode_jobs(&mut stream);

        assert_eq!(
            stream
                .store
                .biome_id(SubChunkKey::new(0, 0, -4, 0), 0, 0, 0),
            Some(7)
        );
        assert_eq!(
            stream
                .store
                .biome_id(SubChunkKey::new(0, 0, 19, 0), 0, 0, 0),
            Some(7)
        );
    }

    #[test]
    fn request_level_chunk_decodes_biomes_before_enqueuing_sub_chunk_requests() {
        let mut stream = WorldStream::new(WorldBootstrap {
            dimension: 0,
            local_player_runtime_id: 1,
            player_position: [0.0; 3],
            world_spawn_position: [0; 3],
            air_network_id: 12_530,
            block_network_ids_are_hashes: false,
        });

        stream
            .submit(
                1,
                request_level_chunk_event(
                    0,
                    0,
                    0,
                    LevelChunkMode::LimitedRequests { highest: 1 },
                    9,
                ),
            )
            .unwrap();

        assert_eq!(stream.pending_decode.len(), 1);
        assert!(stream.take_requests().is_empty());

        complete_pending_decode_jobs(&mut stream);

        assert_eq!(
            stream
                .store
                .biome_id(SubChunkKey::new(0, 0, -4, 0), 0, 0, 0),
            Some(9)
        );
        assert_eq!(
            stream
                .store
                .biome_id(SubChunkKey::new(0, 0, 19, 0), 0, 0, 0),
            Some(9)
        );
        assert_eq!(stream.take_requests().len(), 1);
    }

    #[test]
    fn malformed_request_level_chunk_neither_commits_nor_enqueues() {
        let mut stream = WorldStream::new(WorldBootstrap {
            dimension: 0,
            local_player_runtime_id: 1,
            player_position: [0.0; 3],
            world_spawn_position: [0; 3],
            air_network_id: 12_530,
            block_network_ids_are_hashes: false,
        });

        stream
            .submit(
                1,
                request_level_chunk_event(
                    0,
                    0,
                    0,
                    LevelChunkMode::LimitedRequests { highest: 1 },
                    5,
                ),
            )
            .unwrap();
        complete_pending_decode_jobs(&mut stream);
        assert_eq!(stream.take_requests().len(), 1);

        stream
            .submit(
                2,
                WorldEvent::LevelChunk(LevelChunkEvent {
                    dimension: 0,
                    x: 0,
                    z: 0,
                    mode: LevelChunkMode::LimitedRequests { highest: 1 },
                    payload: vec![1, 18],
                }),
            )
            .unwrap();
        complete_pending_decode_jobs(&mut stream);

        assert_eq!(stream.stats().decode_errors, 1);
        assert!(stream.take_requests().is_empty());
        assert_eq!(
            stream
                .store
                .biome_id(SubChunkKey::new(0, 0, -4, 0), 0, 0, 0),
            Some(5)
        );
        assert_eq!(
            stream
                .store
                .biome_id(SubChunkKey::new(0, 0, 19, 0), 0, 0, 0),
            Some(5)
        );
    }

    fn complete_pending_decode_jobs(stream: &mut WorldStream) {
        while let Some(job) = stream.pending_decode.pop_front() {
            let (sequence, event) = match job {
                super::DecodeJob::InlineLevelChunk {
                    sequence,
                    mut event,
                    base_sub_chunk_y,
                    count,
                    biome_storage_count,
                } => {
                    let payload = std::mem::take(&mut event.payload);
                    (
                        sequence,
                        super::PreparedWorldEvent::InlineLevelChunk {
                            event,
                            decoded: DecodedLevelChunk::decode_with_biomes(
                                base_sub_chunk_y,
                                count,
                                base_sub_chunk_y,
                                biome_storage_count,
                                &payload,
                            ),
                            duration: std::time::Duration::ZERO,
                        },
                    )
                }
                super::DecodeJob::RequestLevelChunk {
                    sequence,
                    mut event,
                    biome_base_sub_chunk_y,
                    biome_storage_count,
                } => {
                    let payload = std::mem::take(&mut event.payload);
                    (
                        sequence,
                        super::PreparedWorldEvent::RequestLevelChunk {
                            event,
                            decoded: world::DecodedBiomeColumn::decode(
                                biome_base_sub_chunk_y,
                                biome_storage_count,
                                &payload,
                            ),
                            duration: std::time::Duration::ZERO,
                        },
                    )
                }
                super::DecodeJob::SubChunks { sequence, batch } => (
                    sequence,
                    super::PreparedWorldEvent::SubChunks {
                        dimension: batch.dimension,
                        entries: super::prepare_sub_chunks(batch),
                        duration: std::time::Duration::ZERO,
                    },
                ),
                super::DecodeJob::BlockUpdates {
                    sequence,
                    batches,
                    air_runtime_id,
                } => (
                    sequence,
                    super::PreparedWorldEvent::BlockUpdates {
                        result: batches
                            .into_iter()
                            .map(|batch| {
                                ChunkStore::prepare_sub_chunk_blocks(
                                    batch.key,
                                    batch.previous.as_deref(),
                                    &batch.updates,
                                    air_runtime_id,
                                )
                            })
                            .collect(),
                        duration: std::time::Duration::ZERO,
                    },
                ),
            };
            stream.accept_decode_completion(super::DecodeCompletion { sequence, event });
        }
        stream.apply_ready();
    }

    fn cave_test_assets() -> RuntimeAssets {
        let compiled = CompiledAssets {
            visuals: vec![
                BlockVisual {
                    faces: [0; 6],
                    flags: BlockFlags::AIR,
                    kind: VisualKind::Invisible,
                    contributor_role: assets::ContributorRole::Air,
                    model_template: NO_MODEL_TEMPLATE,
                    animation: NO_ANIMATION,
                    variant: 0,
                },
                BlockVisual {
                    faces: [1; 6],
                    flags: BlockFlags::CUBE_GEOMETRY | BlockFlags::LEAF_MODEL,
                    kind: VisualKind::Cube,
                    contributor_role: assets::ContributorRole::Primary,
                    model_template: NO_MODEL_TEMPLATE,
                    animation: NO_ANIMATION,
                    variant: 0,
                },
                BlockVisual {
                    faces: [2; 6],
                    flags: BlockFlags::CUBE_GEOMETRY | BlockFlags::OCCLUDES_FULL_FACE,
                    kind: VisualKind::Cube,
                    contributor_role: assets::ContributorRole::Primary,
                    model_template: NO_MODEL_TEMPLATE,
                    animation: NO_ANIMATION,
                    variant: 0,
                },
            ]
            .into_boxed_slice(),
            hashed: Box::new([]),
            materials: vec![
                Material {
                    texture: TextureRef::DIAGNOSTIC,
                    flags: 0,
                    animation: NO_ANIMATION
                };
                3
            ]
            .into_boxed_slice(),
            model_templates: Box::new([]),
            model_quads: Box::new([]),
            animations: Box::new([]),
            animation_frames: Box::new([]),
            texture_pages: vec![TexturePage::new(TextureArray {
                layers: 1,
                mips: [16_u32, 8, 4, 2, 1]
                    .into_iter()
                    .map(|size| TextureMip {
                        size,
                        rgba8: vec![0xff; size as usize * size as usize * 4].into_boxed_slice(),
                    })
                    .collect::<Vec<_>>()
                    .into_boxed_slice(),
            })]
            .into_boxed_slice(),
            biomes: CompiledBiomeAssets::diagnostic(),
        };
        let blob = encode_blob(&compiled).expect("encode cave-connectivity test assets");
        RuntimeAssets::decode(&blob).expect("decode cave-connectivity test assets")
    }

    fn cave_test_slab(runtime_id: u8) -> SubChunk {
        let mut words = vec![0_u32; 128];
        for y in 0..16 {
            for z in 0..16 {
                let linear = (8 << 8) | (z << 4) | y;
                words[linear / 32] |= 1 << (linear % 32);
            }
        }

        let mut encoded = vec![9, 1, 0, 3];
        for word in words {
            encoded.extend_from_slice(&word.to_le_bytes());
        }
        encoded.extend([4, 0, runtime_id << 1]);
        SubChunk::decode(&encoded).expect("decode cave-connectivity slab")
    }

    #[test]
    fn bootstrap_non_finite_horizontal_position_uses_the_shared_finite_scope_anchor() {
        let bootstrap = WorldBootstrap {
            dimension: 0,
            local_player_runtime_id: 1,
            player_position: [f32::NAN, 80.0, f32::INFINITY],
            world_spawn_position: [0; 3],
            air_network_id: 12_530,
            block_network_ids_are_hashes: false,
        };
        let expected = crate::server_position::resolve_server_position(
            bootstrap.player_position,
            [0.0, crate::server_position::SAFE_SERVER_HEIGHT, 0.0],
            None,
        );

        let stream = WorldStream::new(bootstrap);

        assert_eq!(stream.resolved_server_position(), expected);
        let anchor = expected.surface_anchor.unwrap();
        assert!(stream.column_is_active(ChunkKey::new(
            0,
            anchor[0].div_euclid(16),
            anchor[1].div_euclid(16),
        )));
    }

    #[test]
    fn change_dimension_non_finite_horizontal_position_keeps_camera_and_scope_together() {
        let mut stream = WorldStream::new(WorldBootstrap {
            dimension: 0,
            local_player_runtime_id: 1,
            player_position: [7.25, 70.0, -8.75],
            world_spawn_position: [0; 3],
            air_network_id: 12_530,
            block_network_ids_are_hashes: false,
        });
        let change = ChangeDimensionEvent {
            dimension: 1,
            position: [f32::NAN, 32_000.0, f32::INFINITY],
        };
        let expected = crate::server_position::resolve_server_position(
            change.position,
            stream.resolved_server_position().position,
            stream.resolved_server_position().surface_anchor,
        );

        stream
            .submit(1, WorldEvent::ChangeDimension(change))
            .unwrap();

        assert_eq!(stream.resolved_server_position(), expected);
        let anchor = expected.surface_anchor.unwrap();
        assert!(stream.column_is_active(ChunkKey::new(
            1,
            anchor[0].div_euclid(16),
            anchor[1].div_euclid(16),
        )));
        assert!(matches!(
            stream.take_committed_controls().as_slice(),
            [super::CommittedControlEvent::ChangeDimension { resolved, .. }]
                if *resolved == expected
        ));
    }

    #[test]
    fn newer_inline_chunk_is_validated_after_fifo_blocked_publisher_update_commits() {
        let mut stream = WorldStream::new(WorldBootstrap {
            dimension: 0,
            local_player_runtime_id: 1,
            player_position: [0.0; 3],
            world_spawn_position: [0; 3],
            air_network_id: 12_530,
            block_network_ids_are_hashes: false,
        });

        stream.submit(1, inline_air_event(0)).unwrap();
        stream
            .submit(
                2,
                WorldEvent::PublisherUpdate(PublisherUpdateEvent {
                    center: [1_600, 64, 0],
                    radius_blocks: 256,
                }),
            )
            .unwrap();
        stream.submit(3, inline_air_event(100)).unwrap();

        assert_eq!(stream.pending_decode.len(), 2);
        complete_pending_decode_jobs(&mut stream);

        let key = SubChunkKey::new(0, 100, -4, 0);
        assert_eq!(stream.publisher_center, Some([1_600, 64, 0]));
        assert!(stream.resident.contains(&key) || stream.known_air.contains(&key));
    }

    #[test]
    fn equal_loaded_count_with_missing_target_and_source_replacement_is_not_exact() {
        let mut stream = WorldStream::new(WorldBootstrap {
            dimension: 0,
            local_player_runtime_id: 1,
            player_position: [0.0; 3],
            world_spawn_position: [0; 3],
            air_network_id: 12_530,
            block_network_ids_are_hashes: false,
        });
        let target = super::ViewCohort {
            dimension: 0,
            center: [10, 10],
            radius: 1,
        };
        assert_eq!(
            super::ViewCohort {
                dimension: 0,
                center: [0, 0],
                radius: 16,
            }
            .expected_columns()
            .len(),
            1_089
        );
        stream
            .submit(
                1,
                WorldEvent::PublisherUpdate(PublisherUpdateEvent {
                    center: [160, 64, 160],
                    radius_blocks: 16,
                }),
            )
            .unwrap();
        let target_columns = target.expected_columns();
        let missing = *target_columns.last().unwrap();
        let source = ChunkKey::new(0, 0, 0);
        stream.loaded_columns.insert(source);
        stream.capture_source_columns();
        stream.loaded_columns = target_columns
            .iter()
            .copied()
            .filter(|column| *column != missing)
            .collect();
        stream.loaded_columns.insert(source);

        let status = stream.cohort_status(target);

        assert_eq!(status.expected, 9);
        assert_eq!(status.loaded_target, 8);
        assert_eq!(status.missing_target, 1);
        assert_eq!(status.foreign_loaded, 1);
        assert_eq!(status.source_leftover, 1);
        assert!(!status.is_exact());

        stream.loaded_columns.remove(&source);
        stream.loaded_columns.insert(missing);

        assert!(stream.cohort_status(target).is_exact());
    }

    #[test]
    fn publisher_cohort_is_exposed_only_after_fifo_commit() {
        let mut stream = WorldStream::new(WorldBootstrap {
            dimension: 0,
            local_player_runtime_id: 1,
            player_position: [0.0; 3],
            world_spawn_position: [0; 3],
            air_network_id: 12_530,
            block_network_ids_are_hashes: false,
        });
        let target = super::ViewCohort {
            dimension: 0,
            center: [100, 0],
            radius: 16,
        };

        stream.submit(1, inline_air_event(0)).unwrap();
        stream
            .submit(
                2,
                WorldEvent::PublisherUpdate(PublisherUpdateEvent {
                    center: [1_600, 64, 0],
                    radius_blocks: 256,
                }),
            )
            .unwrap();

        assert_ne!(stream.cohort_status(target).committed, Some(target));

        complete_pending_decode_jobs(&mut stream);

        assert_eq!(stream.cohort_status(target).committed, Some(target));
    }

    #[test]
    fn publisher_cohort_accessor_is_exposed_only_after_fifo_commit() {
        let mut stream = WorldStream::new(WorldBootstrap {
            dimension: 0,
            local_player_runtime_id: 1,
            player_position: [0.0; 3],
            world_spawn_position: [0; 3],
            air_network_id: 12_530,
            block_network_ids_are_hashes: false,
        });
        let target = super::ViewCohort {
            dimension: 0,
            center: [100, 0],
            radius: 16,
        };

        assert_eq!(stream.committed_view_cohort(), None);

        stream
            .submit(
                2,
                WorldEvent::PublisherUpdate(PublisherUpdateEvent {
                    center: [1_600, 64, 0],
                    radius_blocks: 256,
                }),
            )
            .unwrap();

        assert_eq!(stream.committed_view_cohort(), None);

        stream
            .submit(1, WorldEvent::ChunkRadiusUpdated(16))
            .unwrap();

        assert_eq!(stream.committed_view_cohort(), Some(target));
    }

    #[test]
    fn source_capture_occurs_at_move_fifo_commit_before_later_publisher_eviction() {
        let mut stream = WorldStream::new(WorldBootstrap {
            dimension: 0,
            local_player_runtime_id: 1,
            player_position: [0.5, 70.0, 0.5],
            world_spawn_position: [0, 70, 0],
            air_network_id: 12_530,
            block_network_ids_are_hashes: false,
        });
        let source = ChunkKey::new(0, 0, 0);
        let source_cohort = super::ViewCohort {
            dimension: 0,
            center: [0, 0],
            radius: 16,
        };
        stream.loaded_columns.insert(source);
        stream.chunk_radius = Some(16);
        stream.schedule_source_capture(2);

        stream
            .submit(
                2,
                WorldEvent::MovePlayer(MovePlayerEvent {
                    runtime_id: 1,
                    position: [1_040.5, 70.0, 1_040.5],
                    pitch: 0.0,
                    yaw: 0.0,
                }),
            )
            .unwrap();
        stream
            .submit(
                3,
                WorldEvent::PublisherUpdate(PublisherUpdateEvent {
                    center: [1_040, 70, 1_040],
                    radius_blocks: 256,
                }),
            )
            .unwrap();
        stream
            .submit(
                1,
                WorldEvent::PublisherUpdate(PublisherUpdateEvent {
                    center: [0, 70, 0],
                    radius_blocks: 256,
                }),
            )
            .unwrap();

        assert!(stream.source_columns.contains(&source));
        assert!(!stream.tracked_columns().contains(&source));
        assert!(matches!(
            stream.take_committed_controls().as_slice(),
            [super::CommittedControlEvent::MovePlayer {
                sequence: 2,
                source_cohort: Some(cohort),
                ..
            }] if *cohort == source_cohort
        ));
    }

    #[test]
    fn publisher_cohort_preserves_over_max_radius_while_runtime_scope_clamps() {
        let mut stream = WorldStream::new(WorldBootstrap {
            dimension: 0,
            local_player_runtime_id: 1,
            player_position: [0.0; 3],
            world_spawn_position: [0; 3],
            air_network_id: 12_530,
            block_network_ids_are_hashes: false,
        });
        let target = super::ViewCohort {
            dimension: 0,
            center: [0, 0],
            radius: 16,
        };

        stream
            .submit(
                1,
                WorldEvent::PublisherUpdate(PublisherUpdateEvent {
                    center: [0, 64, 0],
                    radius_blocks: 272,
                }),
            )
            .unwrap();

        assert_eq!(
            stream.cohort_status(target).committed,
            Some(super::ViewCohort {
                dimension: 0,
                center: [0, 0],
                radius: 17,
            })
        );
        assert_eq!(stream.stats().publisher_radius_chunks, Some(16));
    }

    #[test]
    fn equal_resident_and_known_air_counts_with_key_replacement_change_identity_hashes() {
        let mut stream = WorldStream::new(WorldBootstrap {
            dimension: 0,
            local_player_runtime_id: 1,
            player_position: [0.0; 3],
            world_spawn_position: [0; 3],
            air_network_id: 12_530,
            block_network_ids_are_hashes: false,
        });
        let target = super::ViewCohort {
            dimension: 0,
            center: [0, 0],
            radius: 1,
        };
        stream
            .submit(
                1,
                WorldEvent::PublisherUpdate(PublisherUpdateEvent {
                    center: [0, 64, 0],
                    radius_blocks: 16,
                }),
            )
            .unwrap();
        let key_a = SubChunkKey::new(0, -1, 0, -1);
        let key_b = SubChunkKey::new(0, 1, 0, 1);
        stream.record_known_air(key_a);
        let before = stream.cohort_status(target);

        stream.resident.clear();
        stream.known_air.clear();
        stream.record_known_air(key_b);
        let after = stream.cohort_status(target);

        assert_eq!(before.resident_count, after.resident_count);
        assert_ne!(before.resident_hash, after.resident_hash);
        assert_eq!(before.known_air_count, after.known_air_count);
        assert_ne!(before.known_air_hash, after.known_air_hash);
    }

    #[test]
    fn newer_subchunk_is_validated_after_fifo_blocked_dimension_change_commits() {
        let mut stream = WorldStream::new(WorldBootstrap {
            dimension: 0,
            local_player_runtime_id: 1,
            player_position: [0.0; 3],
            world_spawn_position: [0; 3],
            air_network_id: 12_530,
            block_network_ids_are_hashes: false,
        });

        stream.submit(1, inline_air_event(0)).unwrap();
        stream
            .submit(
                2,
                WorldEvent::ChangeDimension(ChangeDimensionEvent {
                    dimension: 1,
                    position: [1_600.0, 80.0, 0.0],
                }),
            )
            .unwrap();
        stream
            .submit(
                3,
                WorldEvent::LevelChunk(LevelChunkEvent {
                    dimension: 1,
                    x: 100,
                    z: 0,
                    mode: LevelChunkMode::LimitedRequests { highest: 1 },
                    payload: biome_payload(1, 1),
                }),
            )
            .unwrap();
        stream
            .submit(
                4,
                WorldEvent::SubChunks(SubChunkBatchEvent {
                    dimension: 1,
                    entries: vec![SubChunkEntryEvent {
                        position: [100, 0, 0],
                        result: SubChunkResult::AllAir,
                    }],
                }),
            )
            .unwrap();

        assert_eq!(stream.pending_decode.len(), 3);
        complete_pending_decode_jobs(&mut stream);

        let key = SubChunkKey::new(1, 100, 0, 0);
        assert_eq!(stream.current_dimension(), 1);
        assert!(stream.known_air.contains(&key));
        assert!(stream.loaded_columns.contains(&key.chunk()));
    }

    #[test]
    fn deferred_request_events_reserve_outbound_capacity_at_admission() {
        let mut stream = WorldStream::new(WorldBootstrap {
            dimension: 0,
            local_player_runtime_id: 1,
            player_position: [0.0; 3],
            world_spawn_position: [0; 3],
            air_network_id: 12_530,
            block_network_ids_are_hashes: false,
        });

        for sequence in 1..=62_u64 {
            let index = sequence as i32 - 1;
            stream
                .submit(
                    sequence,
                    WorldEvent::LevelChunk(LevelChunkEvent {
                        dimension: 0,
                        x: index.rem_euclid(9) - 4,
                        z: index.div_euclid(9) - 4,
                        mode: LevelChunkMode::LimitedRequests { highest: 1 },
                        payload: biome_payload(0, 1),
                    }),
                )
                .unwrap();
            if sequence == 32 || sequence == 62 {
                complete_pending_decode_jobs(&mut stream);
            }
        }
        // Keep the FIFO blocker on a column that does not supersede one of
        // the 62 queued request-mode columns under test.
        stream.submit(63, inline_air_event(8)).unwrap();
        for (sequence, x) in [(64, 10), (65, 11)] {
            stream
                .submit(
                    sequence,
                    WorldEvent::LevelChunk(LevelChunkEvent {
                        dimension: 0,
                        x,
                        z: 1,
                        mode: LevelChunkMode::LimitedRequests { highest: 1 },
                        payload: biome_payload(0, 1),
                    }),
                )
                .unwrap();
        }

        let error = stream
            .submit(
                66,
                WorldEvent::LevelChunk(LevelChunkEvent {
                    dimension: 0,
                    x: 12,
                    z: 10,
                    mode: LevelChunkMode::LimitedRequests { highest: 1 },
                    payload: biome_payload(0, 1),
                }),
            )
            .unwrap_err();
        assert!(matches!(
            error,
            super::WorldStreamError::OutboundFull { .. }
        ));
        assert_eq!(stream.pending_request_count(), 62);

        complete_pending_decode_jobs(&mut stream);
        assert_eq!(
            stream.pending_request_count(),
            super::OUTBOUND_REQUEST_CAPACITY
        );
    }

    #[test]
    fn heavy_admission_is_bounded_before_rayon_and_retained_work_never_exceeds_constants() {
        let mut stream = WorldStream::new(WorldBootstrap {
            dimension: 0,
            local_player_runtime_id: 1,
            player_position: [0.0; 3],
            world_spawn_position: [0; 3],
            air_network_id: 12_530,
            block_network_ids_are_hashes: false,
        });

        for sequence in 1..=super::MAX_ADMITTED_HEAVY_EVENTS as u64 {
            stream
                .submit(sequence, inline_air_event(sequence as i32))
                .unwrap();
        }
        let stats = stream.stats();
        assert_eq!(
            stats.admitted_heavy_events,
            super::MAX_ADMITTED_HEAVY_EVENTS
        );
        assert_eq!(stats.queued_decode_jobs, super::MAX_ADMITTED_HEAVY_EVENTS);
        assert_eq!(stats.in_flight_decode_jobs, 0);
        assert!(
            stats.queued_decode_jobs + stats.in_flight_decode_jobs + stats.completed_decode_results
                <= super::MAX_ADMITTED_HEAVY_EVENTS
        );

        let error = stream
            .submit(
                super::MAX_ADMITTED_HEAVY_EVENTS as u64 + 1,
                inline_air_event(999),
            )
            .unwrap_err();
        assert!(matches!(
            error,
            super::WorldStreamError::AdmissionFull { .. }
        ));
    }

    #[test]
    fn eviction_purges_unsent_requests_and_late_subchunks_cannot_resurrect_the_column() {
        let mut stream = WorldStream::new(WorldBootstrap {
            dimension: 0,
            local_player_runtime_id: 1,
            player_position: [0.0; 3],
            world_spawn_position: [0; 3],
            air_network_id: 12_530,
            block_network_ids_are_hashes: false,
        });
        let chunk = ChunkKey::new(0, 1, 0);
        let key = SubChunkKey::from_chunk(chunk, -4);
        stream
            .submit(
                1,
                WorldEvent::LevelChunk(LevelChunkEvent {
                    dimension: 0,
                    x: chunk.x,
                    z: chunk.z,
                    mode: LevelChunkMode::LimitedRequests { highest: 1 },
                    payload: biome_payload(0, 1),
                }),
            )
            .unwrap();
        complete_pending_decode_jobs(&mut stream);
        assert_eq!(stream.requests.len(), 1);

        stream.evict_column(chunk);
        assert!(stream.requests.is_empty());
        stream
            .submit(
                2,
                WorldEvent::SubChunks(SubChunkBatchEvent {
                    dimension: 0,
                    entries: vec![SubChunkEntryEvent {
                        position: [key.x, key.y, key.z],
                        result: SubChunkResult::AllAir,
                    }],
                }),
            )
            .unwrap();
        assert_eq!(stream.stats().queued_decode_jobs, 1);
        complete_pending_decode_jobs(&mut stream);
        assert!(!stream.resident.contains(&key));
        assert!(stream.store.sub_chunk(key).is_none());
    }

    #[test]
    fn valid_late_inactive_subchunk_reply_is_ignored_without_side_effects() {
        let mut stream = WorldStream::new(WorldBootstrap {
            dimension: 0,
            local_player_runtime_id: 1,
            player_position: [0.0; 3],
            world_spawn_position: [0; 3],
            air_network_id: 12_530,
            block_network_ids_are_hashes: false,
        });
        let chunk = ChunkKey::new(0, 1, 0);
        let key = SubChunkKey::from_chunk(chunk, -4);
        stream
            .submit(
                1,
                WorldEvent::LevelChunk(LevelChunkEvent {
                    dimension: 0,
                    x: chunk.x,
                    z: chunk.z,
                    mode: LevelChunkMode::LimitedRequests { highest: 1 },
                    payload: biome_payload(0, 1),
                }),
            )
            .unwrap();
        complete_pending_decode_jobs(&mut stream);
        assert!(stream.column_is_active(chunk));
        assert_eq!(stream.take_requests().len(), 1);
        assert!(stream.requested_sub_chunks.contains_key(&chunk));

        stream
            .submit(
                2,
                WorldEvent::PublisherUpdate(PublisherUpdateEvent {
                    center: [1_600, 64, 0],
                    radius_blocks: 0,
                }),
            )
            .unwrap();
        assert!(!stream.column_is_active(chunk));
        assert!(stream.store.sub_chunk(key).is_none());

        let resident_before = stream.resident.clone();
        let known_air_before = stream.known_air.clone();
        let loaded_columns_before = stream.loaded_columns.clone();
        let requested_sub_chunks_before = stream.requested_sub_chunks.clone();
        let sub_chunk_deadlines_before = stream.sub_chunk_deadlines.clone();
        let deferred_retries_before = stream.deferred_retries.clone();
        let deferred_retry_set_before = stream.deferred_retry_set.clone();
        let requests_before = stream.requests.len();
        let stats_before = stream.stats();

        apply_sub_chunk_result(&mut stream, key, super::PreparedSubChunkResult::AllAir);

        assert!(stream.store.sub_chunk(key).is_none());
        assert_eq!(stream.resident, resident_before);
        assert_eq!(stream.known_air, known_air_before);
        assert_eq!(stream.loaded_columns, loaded_columns_before);
        assert_eq!(stream.requested_sub_chunks, requested_sub_chunks_before);
        assert_eq!(stream.sub_chunk_deadlines, sub_chunk_deadlines_before);
        assert_eq!(stream.deferred_retries, deferred_retries_before);
        assert_eq!(stream.deferred_retry_set, deferred_retry_set_before);
        assert_eq!(stream.requests.len(), requests_before);
        assert_eq!(stream.stats(), stats_before);
        assert_eq!(stream.stats().normalization_reasons.inactive_sub_chunks, 0);
    }

    #[test]
    fn old_dimension_and_out_of_radius_chunks_are_rejected_and_radii_are_clamped() {
        let mut stream = WorldStream::new(WorldBootstrap {
            dimension: 0,
            local_player_runtime_id: 1,
            player_position: [0.0; 3],
            world_spawn_position: [0; 3],
            air_network_id: 12_530,
            block_network_ids_are_hashes: false,
        });
        stream
            .submit(1, WorldEvent::ChunkRadiusUpdated(999))
            .unwrap();
        stream
            .submit(
                2,
                WorldEvent::PublisherUpdate(PublisherUpdateEvent {
                    center: [0, 64, 0],
                    radius_blocks: u32::MAX,
                }),
            )
            .unwrap();
        assert_eq!(
            stream.chunk_radius,
            Some(super::PHASE0_MAX_VIEW_RADIUS_CHUNKS)
        );
        assert_eq!(
            stream.publisher_radius_chunks,
            Some(super::PHASE0_MAX_VIEW_RADIUS_CHUNKS)
        );
        let stats = format!("{:?}", stream.stats());
        assert!(
            stats.contains("received_radius_chunks: Some(16)"),
            "{stats}"
        );
        assert!(
            stats.contains("publisher_radius_chunks: Some(16)"),
            "{stats}"
        );

        stream
            .submit(
                3,
                WorldEvent::LevelChunk(LevelChunkEvent {
                    dimension: 0,
                    x: super::PHASE0_MAX_VIEW_RADIUS_CHUNKS + 1,
                    z: 0,
                    mode: LevelChunkMode::LimitlessRequests,
                    payload: biome_payload(0, 1),
                }),
            )
            .unwrap();
        complete_pending_decode_jobs(&mut stream);
        assert!(stream.requests.is_empty());

        stream
            .submit(
                4,
                WorldEvent::ChangeDimension(ChangeDimensionEvent {
                    dimension: 1,
                    position: [0.0, 80.0, 0.0],
                }),
            )
            .unwrap();
        stream.submit(5, inline_air_event(0)).unwrap();
        assert_eq!(stream.current_dimension(), 1);
        assert_eq!(stream.stats().queued_decode_jobs, 1);
        complete_pending_decode_jobs(&mut stream);
        assert_eq!(stream.stats().queued_decode_jobs, 0);
    }

    #[test]
    fn subchunk_admission_requires_the_exact_expected_dimension_column_and_y() {
        let mut stream = WorldStream::new(WorldBootstrap {
            dimension: 0,
            local_player_runtime_id: 1,
            player_position: [0.0; 3],
            world_spawn_position: [0; 3],
            air_network_id: 12_530,
            block_network_ids_are_hashes: false,
        });
        stream
            .submit(
                1,
                WorldEvent::LevelChunk(LevelChunkEvent {
                    dimension: 0,
                    x: 0,
                    z: 0,
                    mode: LevelChunkMode::LimitedRequests { highest: 1 },
                    payload: biome_payload(0, 1),
                }),
            )
            .unwrap();
        stream
            .submit(
                2,
                WorldEvent::SubChunks(SubChunkBatchEvent {
                    dimension: 0,
                    entries: vec![SubChunkEntryEvent {
                        position: [0, -3, 0],
                        result: SubChunkResult::AllAir,
                    }],
                }),
            )
            .unwrap();

        assert_eq!(stream.stats().queued_decode_jobs, 2);
        complete_pending_decode_jobs(&mut stream);
        assert_eq!(stream.stats().queued_decode_jobs, 0);
        assert!(!stream.resident.contains(&SubChunkKey::new(0, 0, -3, 0)));
        assert_eq!(
            stream.requested_sub_chunks[&ChunkKey::new(0, 0, 0)]
                .keys()
                .copied()
                .collect::<BTreeSet<_>>(),
            BTreeSet::from([-4])
        );
    }

    #[test]
    fn control_effects_are_exposed_only_after_older_heavy_sequence_commits_in_fifo_order() {
        let mut stream = WorldStream::new(WorldBootstrap {
            dimension: 0,
            local_player_runtime_id: 1,
            player_position: [0.0; 3],
            world_spawn_position: [0; 3],
            air_network_id: 12_530,
            block_network_ids_are_hashes: false,
        });
        let movement = MovePlayerEvent {
            runtime_id: 1,
            position: [4.0, 70.0, 5.0],
            pitch: 7.0,
            yaw: 9.0,
        };
        let change = ChangeDimensionEvent {
            dimension: 1,
            position: [8.0, 80.0, 9.0],
        };
        stream.submit(1, inline_air_event(0)).unwrap();
        stream.submit(2, WorldEvent::MovePlayer(movement)).unwrap();
        stream
            .submit(3, WorldEvent::ChangeDimension(change))
            .unwrap();

        assert_eq!(stream.current_dimension(), 0);
        assert!(stream.take_committed_controls().is_empty());

        let super::DecodeJob::InlineLevelChunk {
            mut event,
            base_sub_chunk_y,
            count,
            ..
        } = stream.pending_decode.pop_front().unwrap()
        else {
            panic!("expected inline decode job")
        };
        let payload = std::mem::take(&mut event.payload);
        let decoded = DecodedLevelChunk::decode(base_sub_chunk_y, count, &payload);
        stream
            .ordered
            .insert(
                1,
                super::PreparedWorldEvent::InlineLevelChunk {
                    event,
                    decoded,
                    duration: std::time::Duration::ZERO,
                },
            )
            .unwrap();
        stream.apply_ready();

        assert_eq!(stream.current_dimension(), 1);
        assert_eq!(
            stream.take_committed_controls(),
            vec![
                super::CommittedControlEvent::MovePlayer {
                    sequence: 2,
                    movement,
                    resolved: crate::server_position::ResolvedServerPosition {
                        position: movement.position,
                        surface_anchor: None,
                    },
                    source_cohort: None,
                },
                super::CommittedControlEvent::ChangeDimension {
                    change,
                    resolved: crate::server_position::ResolvedServerPosition {
                        position: change.position,
                        surface_anchor: None,
                    },
                },
            ]
        );
    }

    #[test]
    fn newer_update_waits_for_older_decode_and_wins() {
        let key = SubChunkKey::new(0, 0, -4, 0);
        let decoded = DecodedLevelChunk::decode(
            -4,
            1,
            include_bytes!("../../crates/world/fixtures/uniform_non_air.bin"),
        )
        .unwrap();
        let mut ordered = SequenceBuffer::new(1);
        ordered.insert(2, Action::Update).unwrap();
        assert!(ordered.pop_next().is_none(), "sequence two must wait");
        ordered.insert(1, Action::Decode(decoded)).unwrap();

        let mut store = ChunkStore::new();
        while let Some(action) = ordered.pop_next() {
            match action {
                Action::Decode(decoded) => {
                    store.commit_level_chunk(ChunkKey::new(0, 0, 0), decoded);
                }
                Action::Update => {
                    store
                        .update_block(key, BlockUpdate::new(0, 0, 0, 0, 99), 12_530)
                        .unwrap();
                }
            }
        }

        assert_eq!(
            store.sub_chunk(key).unwrap().runtime_id(0, 0, 0, 0),
            Some(99)
        );
    }

    #[test]
    fn render_backpressure_retry_preserves_change_order_for_eventual_delivery() {
        let mut stream = WorldStream::new(WorldBootstrap {
            dimension: 0,
            local_player_runtime_id: 1,
            player_position: [0.0; 3],
            world_spawn_position: [0; 3],
            air_network_id: 12_530,
            block_network_ids_are_hashes: false,
        });
        let first = SubChunkKey::new(0, 1, 2, 3);
        let second = SubChunkKey::new(0, 4, 5, 6);
        stream
            .mesh_changes
            .push_back(super::WorldMeshChange::Remove {
                key: first,
                generation: 1,
                dirty_since: Instant::now(),
            });
        stream
            .mesh_changes
            .push_back(super::WorldMeshChange::Remove {
                key: second,
                generation: 2,
                dirty_since: Instant::now(),
            });

        let blocked = stream.pop_mesh_change().unwrap();
        stream.retry_mesh_change_front(blocked).unwrap();

        assert_eq!(stream.pop_mesh_change().unwrap().key(), first);
        assert_eq!(stream.pop_mesh_change().unwrap().key(), second);
        assert!(stream.pop_mesh_change().is_none());
    }

    #[test]
    fn stale_mesh_revision_is_rejected() {
        let key = SubChunkKey::new(0, -1, 2, 3);
        let mut revisions = RevisionTracker::default();
        let old = revisions.mark_dirty(key, Instant::now());
        let current = revisions.mark_dirty(key, Instant::now());

        assert!(!revisions.is_current(key, old));
        assert!(revisions.is_current(key, current));
    }

    #[test]
    fn mesh_completion_carries_current_palette_native_biome_record() {
        let mut stream = WorldStream::new(WorldBootstrap {
            dimension: 0,
            local_player_runtime_id: 1,
            player_position: [0.0; 3],
            world_spawn_position: [0; 3],
            air_network_id: 12_530,
            block_network_ids_are_hashes: false,
        });
        let key = SubChunkKey::new(0, 0, -4, 0);
        let decoded = DecodedLevelChunk::decode(
            -4,
            1,
            include_bytes!("../../crates/world/fixtures/uniform_non_air.bin"),
        )
        .unwrap();
        stream.store.commit_level_chunk(key.chunk(), decoded);
        stream.store.commit_biome_column(
            key.chunk(),
            DecodedBiomeColumn::decode(-4, 1, &[1, 84]).unwrap(),
        );
        let source = stream.store.sub_chunk(key).unwrap();
        let biome_source = stream.store.biome_storage(key).unwrap();
        let generation = stream.revisions.mark_dirty(key, Instant::now());
        stream.in_flight.insert(key, generation);
        let mesh = mesh_sub_chunk(
            &stream.classifier,
            &stream.runtime_assets,
            stream.network_id_mode,
            &Neighbourhood::empty(),
            &source,
        );
        let biome = PackedBiomeRecord::from_storage(&biome_source, |id| id + 1_000);
        let tint_identity = stream.biome_tint_identity();

        stream.accept_mesh_completion(MeshCompletion {
            key,
            revision: generation,
            source,
            biome_source: Some(biome_source),
            biome,
            tint_identity,
            mesh,
            dependency_mask: MeshDependencyMask::default(),
            duration: Duration::ZERO,
        });

        let super::WorldMeshChange::Upsert { biome, .. } = stream.pop_mesh_change().unwrap() else {
            panic!("expected biome-bearing mesh update")
        };
        assert_eq!(biome.tint_index(0, 0, 0), Some(1_042));
    }

    #[test]
    fn stale_biome_snapshot_cannot_publish_an_old_tint_record() {
        let mut stream = WorldStream::new(WorldBootstrap {
            dimension: 0,
            local_player_runtime_id: 1,
            player_position: [0.0; 3],
            world_spawn_position: [0; 3],
            air_network_id: 12_530,
            block_network_ids_are_hashes: false,
        });
        let key = SubChunkKey::new(0, 0, -4, 0);
        stream.store.commit_level_chunk(
            key.chunk(),
            DecodedLevelChunk::decode(
                -4,
                1,
                include_bytes!("../../crates/world/fixtures/uniform_non_air.bin"),
            )
            .unwrap(),
        );
        stream.store.commit_biome_column(
            key.chunk(),
            DecodedBiomeColumn::decode(-4, 1, &[1, 84]).unwrap(),
        );
        let source = stream.store.sub_chunk(key).unwrap();
        let old_biome = stream.store.biome_storage(key).unwrap();
        let generation = stream.revisions.mark_dirty(key, Instant::now());
        stream.in_flight.insert(key, generation);
        let mesh = mesh_sub_chunk(
            &stream.classifier,
            &stream.runtime_assets,
            stream.network_id_mode,
            &Neighbourhood::empty(),
            &source,
        );
        let old_record = PackedBiomeRecord::from_storage(&old_biome, |_| 0);

        stream.store.commit_biome_column(
            key.chunk(),
            DecodedBiomeColumn::decode(-4, 1, &[1, 86]).unwrap(),
        );
        let tint_identity = stream.biome_tint_identity();
        stream.accept_mesh_completion(MeshCompletion {
            key,
            revision: generation,
            source,
            biome_source: Some(old_biome),
            biome: old_record,
            tint_identity,
            mesh,
            dependency_mask: MeshDependencyMask::default(),
            duration: Duration::ZERO,
        });

        assert_eq!(stream.stats().stale_mesh_jobs, 1);
        assert!(stream.pop_mesh_change().is_none());
    }

    #[test]
    fn remesh_latency_closes_only_when_the_exact_generation_is_applied() {
        let mut stream = WorldStream::new(WorldBootstrap {
            dimension: 0,
            local_player_runtime_id: 1,
            player_position: [0.0; 3],
            world_spawn_position: [0; 3],
            air_network_id: 12_530,
            block_network_ids_are_hashes: false,
        });
        let key = SubChunkKey::new(0, 0, -4, 0);
        let decoded = DecodedLevelChunk::decode(
            -4,
            1,
            include_bytes!("../../crates/world/fixtures/uniform_non_air.bin"),
        )
        .unwrap();
        stream
            .store
            .commit_level_chunk(ChunkKey::new(0, 0, 0), decoded);
        let source = stream.store.sub_chunk(key).unwrap();
        let dirty_since = Instant::now();
        let generation = stream.revisions.mark_dirty(key, dirty_since);
        stream.resident.insert(key);
        assert_eq!(stream.unacknowledged_mesh_count(), 1);
        assert!(!stream.is_mesh_clean(key));
        stream
            .requested_sub_chunks
            .insert(key.chunk(), BTreeMap::from([(key.y, Default::default())]));
        assert_eq!(stream.outstanding_sub_chunk_count(), 1);
        stream.requested_sub_chunks.clear();
        stream.in_flight.insert(key, generation);
        let mesh = mesh_sub_chunk(
            &stream.classifier,
            &stream.runtime_assets,
            stream.network_id_mode,
            &Neighbourhood::empty(),
            source.as_ref(),
        );
        let tint_identity = stream.biome_tint_identity();
        stream.accept_mesh_completion(MeshCompletion {
            key,
            revision: generation,
            source,
            biome_source: None,
            biome: PackedBiomeRecord::fallback(),
            tint_identity,
            mesh,
            dependency_mask: MeshDependencyMask::default(),
            duration: std::time::Duration::from_millis(5),
        });

        assert_eq!(
            stream.stats().max_remesh_latency,
            std::time::Duration::ZERO,
            "worker-ready mesh must not close update-to-visible latency"
        );
        let change = stream.pop_mesh_change().unwrap();
        let super::WorldMeshChange::Upsert {
            generation: queued_generation,
            dirty_since: queued_since,
            ..
        } = change
        else {
            panic!("expected queued mesh upload")
        };
        assert_eq!(queued_generation, generation);
        assert_eq!(queued_since, dirty_since);
        assert_eq!(stream.pending_mesh_change_count(), 0);

        let applied_at = dirty_since + std::time::Duration::from_millis(75);

        stream.acknowledge_mesh_upload(key, generation + 1, dirty_since, applied_at);
        assert_eq!(stream.stats().max_remesh_latency, std::time::Duration::ZERO);
        assert!(stream.revisions.is_current(key, generation));

        stream.acknowledge_mesh_upload(key, generation, dirty_since, applied_at);
        assert_eq!(
            stream.stats().max_remesh_latency,
            std::time::Duration::from_millis(75)
        );
        assert!(!stream.revisions.is_current(key, generation));
        assert_eq!(stream.unacknowledged_mesh_count(), 0);
        assert!(stream.is_mesh_clean(key));
    }

    #[test]
    fn timed_session_resets_pre_ready_duration_high_water_marks_only() {
        let mut stream = WorldStream::new(WorldBootstrap {
            dimension: 0,
            local_player_runtime_id: 1,
            player_position: [0.0; 3],
            world_spawn_position: [0; 3],
            air_network_id: 12_530,
            block_network_ids_are_hashes: false,
        });
        stream.stats.max_decode_duration = std::time::Duration::from_secs(3);
        stream.stats.max_mesh_duration = std::time::Duration::from_secs(4);
        stream.stats.max_remesh_latency = std::time::Duration::from_secs(12);
        stream.stats.decode_errors = 7;

        stream.begin_timed_session();

        assert_eq!(
            stream.stats().max_decode_duration,
            std::time::Duration::ZERO
        );
        assert_eq!(stream.stats().max_mesh_duration, std::time::Duration::ZERO);
        assert_eq!(stream.stats().max_remesh_latency, std::time::Duration::ZERO);
        assert_eq!(stream.stats().decode_errors, 7);
    }

    #[test]
    fn mesh_ack_diagnostic_retains_latest_timestamp_when_acks_arrive_out_of_order() {
        let mut stream = WorldStream::new(WorldBootstrap {
            dimension: 0,
            local_player_runtime_id: 1,
            player_position: [0.0; 3],
            world_spawn_position: [0; 3],
            air_network_id: 12_530,
            block_network_ids_are_hashes: false,
        });
        let started = Instant::now();
        let newer_key = SubChunkKey::new(0, 0, 0, 0);
        let older_key = SubChunkKey::new(0, 0, 1, 0);
        let newer_generation = stream.revisions.mark_dirty(newer_key, started);
        let older_generation = stream.revisions.mark_dirty(older_key, started);
        let newest = started + std::time::Duration::from_millis(100);
        let older = started + std::time::Duration::from_millis(50);

        stream.acknowledge_mesh_upload(newer_key, newer_generation, started, newest);
        stream.acknowledge_mesh_upload(older_key, older_generation, started, older);

        assert_eq!(stream.stats().last_mesh_ack_at, Some(newest));
    }

    #[test]
    fn forced_remesh_returns_exact_resident_generation_manifest() {
        let mut stream = WorldStream::new(WorldBootstrap {
            dimension: 0,
            local_player_runtime_id: 1,
            player_position: [0.0; 3],
            world_spawn_position: [0; 3],
            air_network_id: 12_530,
            block_network_ids_are_hashes: false,
        });
        let keys = [
            SubChunkKey::new(0, -1, -4, 2),
            SubChunkKey::new(0, 0, -4, 0),
            SubChunkKey::new(0, 1, -4, -2),
        ];
        for key in keys {
            stream
                .store
                .update_block(key, BlockUpdate::new(0, 0, 0, 0, 99), 12_530)
                .unwrap();
            stream.resident.insert(key);
        }
        let known_air = SubChunkKey::new(0, 2, -4, 3);
        stream.record_known_air(known_air);
        let previously_dirty_at = std::time::Instant::now();
        stream.mark_dirty_exact(keys[0], previously_dirty_at);
        let started = previously_dirty_at + Duration::from_millis(1);

        let manifest = stream.remesh_all_resident(started);

        assert_eq!(manifest.started_at, started);
        assert_eq!(manifest.entries.len(), 4);
        assert_eq!(
            manifest
                .entries
                .iter()
                .map(|(key, _)| *key)
                .collect::<BTreeSet<_>>(),
            keys.into_iter().chain([known_air]).collect()
        );
        assert_eq!(
            manifest
                .entries
                .iter()
                .map(|(_, generation)| *generation)
                .collect::<BTreeSet<_>>()
                .len(),
            manifest.entries.len(),
            "every forced remesh key must receive one unique generation"
        );
        for (key, generation) in manifest.entries.iter().copied() {
            let dirty = stream.revisions.dirty(key).unwrap();
            assert_eq!(dirty.since, started);
            assert_eq!(dirty.revision, generation);
        }

        assert_eq!(stream.dispatch_mesh_jobs([0.0; 3], 3), 3);
        assert!(stream.take_mesh_changes().iter().any(|change| {
            matches!(
                change,
                super::WorldMeshChange::Remove { key, generation, dirty_since }
                    if *key == known_air
                        && manifest.entries.contains(&(*key, *generation))
                        && *dirty_since == started
            )
        }));
    }

    #[test]
    fn eviction_or_superseding_revision_cannot_complete_forced_manifest() {
        let new_stream = || {
            let mut stream = WorldStream::new(WorldBootstrap {
                dimension: 0,
                local_player_runtime_id: 1,
                player_position: [0.0; 3],
                world_spawn_position: [0; 3],
                air_network_id: 12_530,
                block_network_ids_are_hashes: false,
            });
            let key = SubChunkKey::new(0, 0, -4, 0);
            stream.record_known_air(key);
            (stream, key)
        };

        let started = Instant::now();
        let (mut evicted, evicted_key) = new_stream();
        let evicted_manifest = evicted.remesh_all_resident(started);
        evicted.evict_column(evicted_key.chunk());
        assert_eq!(
            evicted.forced_remesh_manifest_state(&evicted_manifest),
            super::ForcedRemeshManifestState::Invalid
        );

        let (mut superseded, superseded_key) = new_stream();
        let superseded_manifest = superseded.remesh_all_resident(started);
        let superseded_at = started + Duration::from_millis(1);
        superseded.mark_dirty_exact(superseded_key, superseded_at);
        let replacement = superseded.revisions.dirty(superseded_key).unwrap();
        superseded.acknowledge_mesh_upload(
            superseded_key,
            replacement.revision,
            superseded_at,
            superseded_at + Duration::from_millis(1),
        );
        assert_eq!(
            superseded.forced_remesh_manifest_state(&superseded_manifest),
            super::ForcedRemeshManifestState::Invalid,
            "applying a replacement revision must not satisfy the forced generation"
        );
    }

    #[test]
    fn negative_absolute_updates_use_euclidean_chunk_coordinates() {
        let event = BlockUpdateEvent {
            dimension: 2,
            position: [-1, -65, 16],
            layer: 1,
            network_id: 0xdead_beef,
        };
        let (key, update) = split_block_update(event).unwrap();

        assert_eq!(key, SubChunkKey::new(2, -1, -5, 1));
        assert_eq!(update, BlockUpdate::new(15, 15, 0, 1, 0xdead_beef));
    }

    #[test]
    fn normalization_breakdown_distinguishes_inactive_and_malformed_world_traffic() {
        let mut stream = WorldStream::new(WorldBootstrap {
            dimension: 0,
            local_player_runtime_id: 1,
            player_position: [0.0; 3],
            world_spawn_position: [0; 3],
            air_network_id: 12_530,
            block_network_ids_are_hashes: false,
        });
        stream.chunk_radius = Some(0);

        let batches = stream.snapshot_block_mutation_batches(vec![
            BlockUpdateEvent {
                dimension: 0,
                position: [16, 0, 0],
                layer: 0,
                network_id: 1,
            },
            BlockUpdateEvent {
                dimension: 0,
                position: [0, 0, 0],
                layer: usize::MAX,
                network_id: 2,
            },
        ]);
        assert!(batches.is_empty());

        stream.apply_prepared(super::PreparedWorldEvent::SubChunks {
            dimension: 0,
            entries: vec![
                super::PreparedSubChunk {
                    position: [0, 0, 0],
                    result: super::PreparedSubChunkResult::AllAir,
                },
                super::PreparedSubChunk {
                    position: [1, 0, 0],
                    result: super::PreparedSubChunkResult::AllAir,
                },
            ],
            duration: std::time::Duration::ZERO,
        });

        let stats = stream.stats();
        assert_eq!(stats.normalization_errors, 3);
        assert_eq!(stats.normalization_reasons.inactive_block_updates, 1);
        assert_eq!(stats.normalization_reasons.malformed_block_updates, 1);
        assert_eq!(stats.normalization_reasons.unexpected_sub_chunks, 1);
        assert_eq!(stats.normalization_reasons.inactive_sub_chunks, 0);
        assert_eq!(stats.normalization_reasons.total(), 3);
    }

    #[test]
    fn max_block_update_batch_prepares_off_thread_and_commits_atomically_in_fifo() {
        let mut stream = WorldStream::new(WorldBootstrap {
            dimension: 0,
            local_player_runtime_id: 1,
            player_position: [0.0; 3],
            world_spawn_position: [0; 3],
            air_network_id: 12_530,
            block_network_ids_are_hashes: false,
        });
        let mut updates = (0..4_095)
            .map(|linear| BlockUpdateEvent {
                dimension: 0,
                position: [linear >> 8, linear & 15, (linear >> 4) & 15],
                layer: 0,
                network_id: linear as u32 + 1,
            })
            .collect::<Vec<_>>();
        updates.push(BlockUpdateEvent {
            dimension: 0,
            position: [0, 0, 0],
            layer: 0,
            network_id: 99_999,
        });
        let movement = MovePlayerEvent {
            runtime_id: 1,
            position: [1.0, 70.0, 2.0],
            pitch: 0.0,
            yaw: 0.0,
        };

        stream.submit(1, WorldEvent::BlockUpdates(updates)).unwrap();
        stream.submit(2, WorldEvent::MovePlayer(movement)).unwrap();

        assert_eq!(stream.stats().queued_decode_jobs, 1);
        assert!(stream.take_committed_controls().is_empty());
        assert!(
            stream
                .store
                .sub_chunk(SubChunkKey::new(0, 0, 0, 0))
                .is_none()
        );

        complete_pending_decode_jobs(&mut stream);

        let committed = stream
            .store
            .sub_chunk(SubChunkKey::new(0, 0, 0, 0))
            .unwrap();
        assert_eq!(committed.runtime_id(0, 0, 0, 0), Some(99_999));
        assert_eq!(committed.runtime_id(0, 15, 14, 15), Some(4_095));
        assert_eq!(
            stream.take_committed_controls(),
            vec![super::CommittedControlEvent::MovePlayer {
                sequence: 2,
                movement,
                resolved: crate::server_position::ResolvedServerPosition {
                    position: movement.position,
                    surface_anchor: None,
                },
                source_cohort: None,
            }]
        );
    }

    #[test]
    fn request_modes_use_vanilla_dimension_base_and_bounded_counts() {
        let mut stream = WorldStream::new(WorldBootstrap {
            dimension: 0,
            local_player_runtime_id: 1,
            player_position: [0.0; 3],
            world_spawn_position: [0; 3],
            air_network_id: 12_530,
            block_network_ids_are_hashes: false,
        });

        stream
            .submit(
                1,
                WorldEvent::LevelChunk(LevelChunkEvent {
                    dimension: 0,
                    x: -2,
                    z: 5,
                    mode: LevelChunkMode::LimitedRequests { highest: u16::MAX },
                    payload: biome_payload(0, 1),
                }),
            )
            .unwrap();
        complete_pending_decode_jobs(&mut stream);
        let overworld_requests = stream.take_requests();
        assert_eq!(overworld_requests.len(), 1);
        assert_eq!(overworld_requests[0].dimension, 0);
        assert_eq!(overworld_requests[0].chunk, ChunkKey::new(0, -2, 5));
        assert_eq!(overworld_requests[0].base_sub_chunk_y, -4);
        assert_eq!(overworld_requests[0].count, 24);
        stream
            .submit(
                2,
                WorldEvent::ChangeDimension(ChangeDimensionEvent {
                    dimension: 1,
                    position: [0.0, 80.0, 0.0],
                }),
            )
            .unwrap();
        stream
            .submit(
                3,
                WorldEvent::LevelChunk(LevelChunkEvent {
                    dimension: 1,
                    x: 7,
                    z: -9,
                    mode: LevelChunkMode::LimitlessRequests,
                    payload: biome_payload(1, 1),
                }),
            )
            .unwrap();
        complete_pending_decode_jobs(&mut stream);

        let requests = stream.take_requests();
        assert_eq!(requests.len(), 1);
        assert_eq!(requests[0].dimension, 1);
        assert_eq!(requests[0].base_sub_chunk_y, 0);
        assert_eq!(requests[0].count, 8);
    }

    #[test]
    fn outbound_request_fifo_has_a_hard_admission_capacity() {
        let mut stream = WorldStream::new(WorldBootstrap {
            dimension: 0,
            local_player_runtime_id: 1,
            player_position: [0.0; 3],
            world_spawn_position: [0; 3],
            air_network_id: 12_530,
            block_network_ids_are_hashes: false,
        });
        for index in 0..super::OUTBOUND_REQUEST_CAPACITY {
            let x = index as i32 % 17 - 8;
            let z = index as i32 / 17 - 8;
            stream
                .submit(
                    index as u64 + 1,
                    WorldEvent::LevelChunk(LevelChunkEvent {
                        dimension: 0,
                        x,
                        z,
                        mode: LevelChunkMode::LimitedRequests { highest: 1 },
                        payload: biome_payload(0, 1),
                    }),
                )
                .unwrap();
            if (index + 1) % super::MAX_ADMITTED_HEAVY_EVENTS == 0 {
                complete_pending_decode_jobs(&mut stream);
            }
        }
        assert_eq!(
            stream.pending_request_count(),
            super::OUTBOUND_REQUEST_CAPACITY
        );

        assert!(matches!(
            stream
                .submit(
                    super::OUTBOUND_REQUEST_CAPACITY as u64 + 1,
                    WorldEvent::LevelChunk(LevelChunkEvent {
                        dimension: 0,
                        x: 9,
                        z: 9,
                        mode: LevelChunkMode::LimitlessRequests,
                        payload: biome_payload(0, 1),
                    }),
                )
                .unwrap_err(),
            super::WorldStreamError::OutboundFull { .. }
        ));
    }

    fn stream_with_one_expected_sub_chunk() -> (WorldStream, SubChunkKey) {
        let mut stream = WorldStream::new(WorldBootstrap {
            dimension: 0,
            local_player_runtime_id: 1,
            player_position: [0.0; 3],
            world_spawn_position: [0; 3],
            air_network_id: 12_530,
            block_network_ids_are_hashes: false,
        });
        let key = SubChunkKey::new(0, 0, -4, 0);
        stream
            .submit(
                1,
                WorldEvent::LevelChunk(LevelChunkEvent {
                    dimension: 0,
                    x: 0,
                    z: 0,
                    mode: LevelChunkMode::LimitedRequests { highest: 1 },
                    payload: biome_payload(0, 1),
                }),
            )
            .unwrap();
        complete_pending_decode_jobs(&mut stream);
        assert_eq!(stream.take_requests().len(), 1);
        (stream, key)
    }

    fn apply_sub_chunk_result(
        stream: &mut WorldStream,
        key: SubChunkKey,
        result: super::PreparedSubChunkResult,
    ) {
        stream.apply_prepared(super::PreparedWorldEvent::SubChunks {
            dimension: key.dimension,
            entries: vec![super::PreparedSubChunk {
                position: [key.x, key.y, key.z],
                result,
            }],
            duration: std::time::Duration::ZERO,
        });
    }

    fn stream_with_unsent_sub_chunks(
        count: u16,
    ) -> (WorldStream, Vec<SubChunkKey>, super::PendingSubChunkRequest) {
        let mut stream = WorldStream::new(WorldBootstrap {
            dimension: 0,
            local_player_runtime_id: 1,
            player_position: [0.0; 3],
            world_spawn_position: [0; 3],
            air_network_id: 12_530,
            block_network_ids_are_hashes: false,
        });
        let chunk = ChunkKey::new(0, 0, 0);
        stream
            .submit(
                1,
                WorldEvent::LevelChunk(LevelChunkEvent {
                    dimension: 0,
                    x: chunk.x,
                    z: chunk.z,
                    mode: LevelChunkMode::LimitedRequests { highest: count },
                    payload: biome_payload(0, 1),
                }),
            )
            .unwrap();
        complete_pending_decode_jobs(&mut stream);
        let request = stream
            .pop_next_request()
            .expect("request-mode LevelChunk should enqueue one request");
        let keys = (0..count)
            .map(|offset| SubChunkKey::from_chunk(chunk, -4 + i32::from(offset)))
            .collect();
        (stream, keys, request)
    }

    fn acknowledge_request_sent(
        stream: &mut WorldStream,
        request: &super::PendingSubChunkRequest,
        sent_at: Instant,
    ) {
        stream.acknowledge_sub_chunk_request_sent(
            request.chunk,
            request.base_sub_chunk_y,
            request.count,
            sent_at,
        );
    }

    #[test]
    fn omitted_sub_chunk_y_retries_at_deadline_then_completes_after_bound() {
        let started = Instant::now();
        let (mut stream, keys, initial) = stream_with_unsent_sub_chunks(1);
        let key = keys[0];
        acknowledge_request_sent(&mut stream, &initial, started);

        for attempt in 1..=super::MAX_SUB_CHUNK_RETRIES {
            let deadline = started + super::SUB_CHUNK_RESPONSE_TIMEOUT * u32::from(attempt);
            stream.expire_sub_chunk_deadlines(deadline);
            let retry = stream
                .pop_next_request()
                .expect("an omitted Y should queue the exact bounded retry");
            assert_eq!(retry.chunk, key.chunk());
            assert_eq!(retry.base_sub_chunk_y, key.y);
            assert_eq!(retry.count, 1);
            acknowledge_request_sent(&mut stream, &retry, deadline);
        }

        let terminal_deadline = started
            + super::SUB_CHUNK_RESPONSE_TIMEOUT
                * u32::from(super::MAX_SUB_CHUNK_RETRIES.saturating_add(1));
        stream.expire_sub_chunk_deadlines(terminal_deadline);

        assert!(stream.loaded_columns.contains(&key.chunk()));
        assert!(!stream.requested_sub_chunks.contains_key(&key.chunk()));
        assert!(!stream.resident.contains(&key));
        assert!(!stream.known_air.contains(&key));
        assert!(stream.sub_chunk_deadlines.is_empty());
        assert_eq!(stream.pending_request_count(), 0);
        let stats = stream.stats();
        assert_eq!(stats.awaiting_sub_chunk_responses, 0);
        assert_eq!(stats.sub_chunk_timeouts, 3);
        assert_eq!(stats.sub_chunk_retries_scheduled, 2);
        assert_eq!(stats.sub_chunk_retry_exhaustions, 1);

        let errors_before = stream.stats().normalization_errors;
        apply_sub_chunk_result(&mut stream, key, super::PreparedSubChunkResult::AllAir);
        assert_eq!(stream.stats().normalization_errors, errors_before + 1);
        assert!(!stream.resident.contains(&key));
        assert!(!stream.known_air.contains(&key));
    }

    #[test]
    fn response_deadline_begins_only_after_successful_send() {
        let started = Instant::now();
        let (mut stream, keys, initial) = stream_with_unsent_sub_chunks(1);
        let key = keys[0];
        assert!(
            stream.retry_request_front(initial).is_ok(),
            "a failed send must restore its unsent request"
        );

        stream.expire_sub_chunk_deadlines(started + Duration::from_secs(100));
        assert_eq!(stream.stats().awaiting_sub_chunk_responses, 0);
        assert_eq!(stream.stats().sub_chunk_timeouts, 0);
        assert!(stream.requested_sub_chunks.contains_key(&key.chunk()));

        let retry = stream.pop_next_request().unwrap();
        let sent_at = started + Duration::from_secs(100);
        acknowledge_request_sent(&mut stream, &retry, sent_at);
        assert_eq!(stream.stats().awaiting_sub_chunk_responses, 1);
        stream.expire_sub_chunk_deadlines(
            sent_at + super::SUB_CHUNK_RESPONSE_TIMEOUT - Duration::from_nanos(1),
        );
        assert_eq!(stream.stats().sub_chunk_timeouts, 0);
        stream.expire_sub_chunk_deadlines(sent_at + super::SUB_CHUNK_RESPONSE_TIMEOUT);
        assert_eq!(stream.stats().sub_chunk_timeouts, 1);
    }

    #[test]
    fn reply_from_already_sent_retry_is_not_unexpected_after_first_attempt_completes() {
        let started = Instant::now();
        let (mut stream, keys, initial) = stream_with_unsent_sub_chunks(1);
        let key = keys[0];
        acknowledge_request_sent(&mut stream, &initial, started);

        let retry_sent_at = started + super::SUB_CHUNK_RESPONSE_TIMEOUT;
        stream.expire_sub_chunk_deadlines(retry_sent_at);
        let retry = stream
            .pop_next_request()
            .expect("the expired initial attempt should queue an exact retry");
        acknowledge_request_sent(&mut stream, &retry, retry_sent_at);

        stream
            .submit(
                2,
                WorldEvent::SubChunks(SubChunkBatchEvent {
                    dimension: key.dimension,
                    entries: vec![SubChunkEntryEvent {
                        position: [key.x, key.y, key.z],
                        result: SubChunkResult::AllAir,
                    }],
                }),
            )
            .unwrap();
        complete_pending_decode_jobs(&mut stream);
        let unexpected_before = stream.stats().normalization_reasons.unexpected_sub_chunks;
        stream
            .submit(
                3,
                WorldEvent::SubChunks(SubChunkBatchEvent {
                    dimension: key.dimension,
                    entries: vec![SubChunkEntryEvent {
                        position: [key.x, key.y, key.z],
                        result: SubChunkResult::AllAir,
                    }],
                }),
            )
            .unwrap();
        complete_pending_decode_jobs(&mut stream);

        assert_eq!(
            stream.stats().normalization_reasons.unexpected_sub_chunks,
            unexpected_before
        );
    }

    #[test]
    fn timely_sub_chunk_admission_disarms_and_cancels_before_decode_or_expiry() {
        let started = Instant::now();
        let (mut stream, keys, initial) = stream_with_unsent_sub_chunks(2);
        acknowledge_request_sent(&mut stream, &initial, started);

        let first_deadline = started + super::SUB_CHUNK_RESPONSE_TIMEOUT;
        stream.expire_sub_chunk_deadlines(first_deadline);
        let sent_retry = stream
            .pop_next_request()
            .expect("the first exact retry should retain FIFO order");
        acknowledge_request_sent(&mut stream, &sent_retry, first_deadline);
        assert_eq!(stream.pending_request_count(), 1);
        assert_eq!(stream.sub_chunk_deadlines.len(), 1);

        stream
            .submit(
                2,
                WorldEvent::SubChunks(SubChunkBatchEvent {
                    dimension: 0,
                    entries: keys
                        .iter()
                        .map(|key| SubChunkEntryEvent {
                            position: [key.x, key.y, key.z],
                            result: SubChunkResult::AllAir,
                        })
                        .collect(),
                }),
            )
            .unwrap();

        assert_eq!(stream.pending_decode.len(), 1);
        assert!(stream.sub_chunk_deadlines.is_empty());
        assert_eq!(stream.pending_request_count(), 0);
        let retry_deadline = first_deadline + super::SUB_CHUNK_RESPONSE_TIMEOUT;
        stream.expire_sub_chunk_deadlines(retry_deadline);
        assert_eq!(stream.stats().sub_chunk_timeouts, 2);
        assert_eq!(stream.outstanding_sub_chunk_count(), 2);

        stream.dispatch_decode_jobs();
        assert!(stream.pending_decode.is_empty());
        assert_eq!(stream.in_flight_decode_jobs, 1);
        stream.expire_sub_chunk_deadlines(retry_deadline);
        assert_eq!(stream.stats().sub_chunk_timeouts, 2);
        assert_eq!(stream.outstanding_sub_chunk_count(), 2);
    }

    #[test]
    fn transport_ack_after_reply_admission_cannot_rearm_expiry_during_decode() {
        let acknowledged_at = Instant::now();
        let (mut stream, keys, initial) = stream_with_unsent_sub_chunks(1);
        let key = keys[0];
        stream.record_sub_chunk_request_transport_pending(
            initial.chunk,
            initial.base_sub_chunk_y,
            initial.count,
        );
        stream
            .submit(
                2,
                WorldEvent::SubChunks(SubChunkBatchEvent {
                    dimension: key.dimension,
                    entries: vec![SubChunkEntryEvent {
                        position: [key.x, key.y, key.z],
                        result: SubChunkResult::AllAir,
                    }],
                }),
            )
            .unwrap();
        assert_eq!(stream.pending_decode.len(), 1);
        assert!(stream.sub_chunk_deadlines.is_empty());

        stream.acknowledge_sub_chunk_request_sent(
            initial.chunk,
            initial.base_sub_chunk_y,
            initial.count,
            acknowledged_at,
        );

        assert!(stream.sub_chunk_deadlines.is_empty());
        stream.expire_sub_chunk_deadlines(acknowledged_at + super::SUB_CHUNK_RESPONSE_TIMEOUT);
        assert_eq!(stream.stats().sub_chunk_timeouts, 0);
        assert_eq!(stream.outstanding_sub_chunk_count(), 1);
    }

    #[test]
    fn explicit_transient_reply_disarms_old_deadline_and_preserves_retry_bound() {
        let started = Instant::now();
        let (mut stream, keys, initial) = stream_with_unsent_sub_chunks(1);
        let key = keys[0];
        acknowledge_request_sent(&mut stream, &initial, started);

        apply_sub_chunk_result(
            &mut stream,
            key,
            super::PreparedSubChunkResult::Unavailable(SubChunkUnavailable::ChunkNotFound),
        );
        assert!(stream.sub_chunk_deadlines.is_empty());
        assert_eq!(stream.stats().awaiting_sub_chunk_responses, 0);
        assert_eq!(stream.stats().sub_chunk_retries_scheduled, 1);
        stream.expire_sub_chunk_deadlines(started + super::SUB_CHUNK_RESPONSE_TIMEOUT);
        assert_eq!(stream.stats().sub_chunk_timeouts, 0);

        let first_retry = stream.pop_next_request().unwrap();
        let first_retry_sent_at = started + Duration::from_secs(1);
        acknowledge_request_sent(&mut stream, &first_retry, first_retry_sent_at);
        stream.expire_sub_chunk_deadlines(first_retry_sent_at + super::SUB_CHUNK_RESPONSE_TIMEOUT);
        let second_retry = stream.pop_next_request().unwrap();
        let second_retry_sent_at = first_retry_sent_at + super::SUB_CHUNK_RESPONSE_TIMEOUT;
        acknowledge_request_sent(&mut stream, &second_retry, second_retry_sent_at);
        stream.expire_sub_chunk_deadlines(second_retry_sent_at + super::SUB_CHUNK_RESPONSE_TIMEOUT);

        assert!(stream.loaded_columns.contains(&key.chunk()));
        assert!(!stream.known_air.contains(&key));
        let stats = stream.stats();
        assert_eq!(stats.sub_chunk_timeouts, 2);
        assert_eq!(stats.sub_chunk_retries_scheduled, 2);
        assert_eq!(stats.sub_chunk_retry_exhaustions, 1);
    }

    #[test]
    fn explicit_transient_retry_preserves_older_deferred_fifo_when_outbound_reopens() {
        let started = Instant::now();
        let (mut stream, keys, initial) = stream_with_unsent_sub_chunks(2);
        acknowledge_request_sent(&mut stream, &initial, started);
        for sequence in 0..super::OUTBOUND_REQUEST_CAPACITY {
            stream
                .requests
                .push_back(super::OutboundRequestSlot::Reserved(sequence as u64 + 10));
        }
        apply_sub_chunk_result(
            &mut stream,
            keys[0],
            super::PreparedSubChunkResult::Unavailable(SubChunkUnavailable::ChunkNotFound),
        );
        assert_eq!(stream.deferred_retries.front(), Some(&keys[0]));
        for index in 1..super::DEFERRED_RETRY_CAPACITY {
            let key = SubChunkKey::new(0, 100 + index as i32, -4, 100);
            stream.deferred_retries.push_back(key);
            stream.deferred_retry_set.insert(key);
        }
        stream.requests.pop_front();
        let normalization_before = stream.stats().normalization_errors;

        apply_sub_chunk_result(
            &mut stream,
            keys[1],
            super::PreparedSubChunkResult::Unavailable(SubChunkUnavailable::PlayerNotFound),
        );

        let outbound_retry_y = stream.requests.iter().find_map(|slot| match slot {
            super::OutboundRequestSlot::Ready(request) => Some(request.base_sub_chunk_y),
            super::OutboundRequestSlot::Reserved(_) => None,
        });
        assert_eq!(outbound_retry_y, Some(keys[0].y));
        assert_eq!(stream.deferred_retries.back(), Some(&keys[1]));
        assert_eq!(
            stream.deferred_retries.len(),
            super::DEFERRED_RETRY_CAPACITY
        );
        assert_eq!(stream.outstanding_sub_chunk_count(), 2);
        assert_eq!(stream.stats().sub_chunk_retries_scheduled, 2);
        assert_eq!(stream.stats().normalization_errors, normalization_before);
        assert!(!stream.loaded_columns.contains(&keys[0].chunk()));
    }

    #[test]
    fn late_success_cancels_queued_timeout_retry() {
        let started = Instant::now();
        let (mut stream, keys, initial) = stream_with_unsent_sub_chunks(2);
        acknowledge_request_sent(&mut stream, &initial, started);
        for sequence in 0..super::OUTBOUND_REQUEST_CAPACITY - 1 {
            stream
                .requests
                .push_back(super::OutboundRequestSlot::Reserved(sequence as u64 + 10));
        }
        stream.expire_sub_chunk_deadlines(started + super::SUB_CHUNK_RESPONSE_TIMEOUT);
        assert_eq!(stream.pending_request_count(), 1);
        assert_eq!(stream.deferred_retries.len(), 1);

        for key in &keys {
            apply_sub_chunk_result(&mut stream, *key, super::PreparedSubChunkResult::AllAir);
        }

        assert!(stream.loaded_columns.contains(&keys[0].chunk()));
        assert!(keys.iter().all(|key| stream.known_air.contains(key)));
        assert_eq!(stream.pending_request_count(), 0);
        assert!(stream.deferred_retries.is_empty());
        assert!(stream.sub_chunk_deadlines.is_empty());
        let stats = stream.stats();
        assert_eq!(stats.sub_chunk_timeouts, 2);
        assert_eq!(stats.sub_chunk_retries_scheduled, 2);
        assert_eq!(stats.sub_chunk_retry_exhaustions, 0);
    }

    #[test]
    fn eviction_purges_deadlines_retries_and_late_reply_state() {
        let started = Instant::now();
        let (mut stream, keys, initial) = stream_with_unsent_sub_chunks(3);
        acknowledge_request_sent(&mut stream, &initial, started);
        for sequence in 0..super::OUTBOUND_REQUEST_CAPACITY - 2 {
            stream
                .requests
                .push_back(super::OutboundRequestSlot::Reserved(sequence as u64 + 10));
        }
        stream.expire_sub_chunk_deadlines(started + super::SUB_CHUNK_RESPONSE_TIMEOUT);
        assert_eq!(stream.pending_request_count(), 2);
        assert_eq!(stream.deferred_retries.len(), 1);
        stream
            .requests
            .retain(|slot| matches!(slot, super::OutboundRequestSlot::Ready(_)));
        let armed_retry = stream.pop_next_request().unwrap();
        acknowledge_request_sent(
            &mut stream,
            &armed_retry,
            started + super::SUB_CHUNK_RESPONSE_TIMEOUT,
        );
        assert!(!stream.sub_chunk_deadlines.is_empty());
        assert_eq!(stream.pending_request_count(), 1);
        assert_eq!(stream.deferred_retries.len(), 1);

        let chunk = keys[0].chunk();
        stream.evict_column(chunk);

        assert!(!stream.requested_sub_chunks.contains_key(&chunk));
        assert!(stream.sub_chunk_deadlines.is_empty());
        assert_eq!(stream.pending_request_count(), 0);
        assert!(stream.deferred_retries.is_empty());
        assert!(stream.deferred_retry_set.is_empty());
        assert_eq!(stream.stats().awaiting_sub_chunk_responses, 0);

        let errors_before = stream.stats().normalization_errors;
        apply_sub_chunk_result(&mut stream, keys[0], super::PreparedSubChunkResult::AllAir);
        assert_eq!(stream.stats().normalization_errors, errors_before + 1);
        assert!(!stream.loaded_columns.contains(&chunk));
        assert!(!stream.resident.contains(&keys[0]));
        assert!(!stream.known_air.contains(&keys[0]));
    }

    #[test]
    fn expired_deadlines_obey_capacity_without_loss_or_overflow() {
        let started = Instant::now();
        let (mut stream, keys, initial) = stream_with_unsent_sub_chunks(3);
        acknowledge_request_sent(&mut stream, &initial, started);
        for sequence in 0..super::OUTBOUND_REQUEST_CAPACITY {
            stream
                .requests
                .push_back(super::OutboundRequestSlot::Reserved(sequence as u64 + 10));
        }
        apply_sub_chunk_result(
            &mut stream,
            keys[0],
            super::PreparedSubChunkResult::Unavailable(SubChunkUnavailable::ChunkNotFound),
        );
        assert_eq!(stream.deferred_retries.front(), Some(&keys[0]));
        for index in 1..super::DEFERRED_RETRY_CAPACITY {
            let key = SubChunkKey::new(0, 100 + index as i32, -4, 100);
            stream.deferred_retries.push_back(key);
            stream.deferred_retry_set.insert(key);
        }
        let normalization_before = stream.stats().normalization_errors;
        let deadline = started + super::SUB_CHUNK_RESPONSE_TIMEOUT;

        stream.expire_sub_chunk_deadlines(deadline);
        assert_eq!(stream.sub_chunk_deadlines.len(), 2);
        assert_eq!(stream.stats().sub_chunk_timeouts, 0);
        assert_eq!(stream.stats().sub_chunk_retries_scheduled, 1);
        assert_eq!(stream.stats().normalization_errors, normalization_before);

        stream.requests.pop_front();
        stream.expire_sub_chunk_deadlines(deadline);

        assert_eq!(stream.requests.len(), super::OUTBOUND_REQUEST_CAPACITY);
        let outbound_retry_y = stream.requests.iter().find_map(|slot| match slot {
            super::OutboundRequestSlot::Ready(request) => Some(request.base_sub_chunk_y),
            super::OutboundRequestSlot::Reserved(_) => None,
        });
        assert_eq!(outbound_retry_y, Some(keys[0].y));
        assert_eq!(
            stream.deferred_retries.len(),
            super::DEFERRED_RETRY_CAPACITY
        );
        assert_eq!(stream.deferred_retries.back(), Some(&keys[1]));
        assert_eq!(stream.sub_chunk_deadlines.len(), 1);
        assert!(stream.sub_chunk_deadlines.contains(&(deadline, keys[2])));
        assert_eq!(stream.outstanding_sub_chunk_count(), 3);
        assert_eq!(stream.stats().sub_chunk_timeouts, 1);
        assert_eq!(stream.stats().sub_chunk_retries_scheduled, 2);
        assert_eq!(stream.stats().normalization_errors, normalization_before);
    }

    #[test]
    fn timeout_progress_stats_are_exact_and_deterministic() {
        let started = Instant::now();
        let (mut stream, keys, initial) = stream_with_unsent_sub_chunks(2);
        acknowledge_request_sent(&mut stream, &initial, started);
        assert_eq!(stream.stats().awaiting_sub_chunk_responses, 2);

        let first_deadline = started + super::SUB_CHUNK_RESPONSE_TIMEOUT;
        stream.expire_sub_chunk_deadlines(first_deadline);
        let stats = stream.stats();
        assert_eq!(stats.awaiting_sub_chunk_responses, 0);
        assert_eq!(stats.sub_chunk_timeouts, 2);
        assert_eq!(stats.sub_chunk_retries_scheduled, 2);
        assert_eq!(stats.sub_chunk_retry_exhaustions, 0);

        let retries = [
            stream.pop_next_request().unwrap(),
            stream.pop_next_request().unwrap(),
        ];
        for retry in &retries {
            acknowledge_request_sent(&mut stream, retry, first_deadline);
        }
        assert_eq!(stream.stats().awaiting_sub_chunk_responses, 2);
        apply_sub_chunk_result(&mut stream, keys[0], super::PreparedSubChunkResult::AllAir);
        assert_eq!(stream.stats().awaiting_sub_chunk_responses, 1);

        let second_deadline = first_deadline + super::SUB_CHUNK_RESPONSE_TIMEOUT;
        stream.expire_sub_chunk_deadlines(second_deadline);
        let final_retry = stream.pop_next_request().unwrap();
        acknowledge_request_sent(&mut stream, &final_retry, second_deadline);
        let third_deadline = second_deadline + super::SUB_CHUNK_RESPONSE_TIMEOUT;
        stream.expire_sub_chunk_deadlines(third_deadline);

        let stats = stream.stats();
        assert_eq!(stats.awaiting_sub_chunk_responses, 0);
        assert_eq!(stats.sub_chunk_timeouts, 4);
        assert_eq!(stats.sub_chunk_retries_scheduled, 3);
        assert_eq!(stats.sub_chunk_retry_exhaustions, 1);
        assert_eq!(stream.outstanding_sub_chunk_count(), 0);
    }

    #[test]
    fn unavailable_value_is_preserved_and_y_out_of_bounds_completes_split_batch_as_air() {
        let prepared = super::prepare_sub_chunks(SubChunkBatchEvent {
            dimension: 0,
            entries: vec![SubChunkEntryEvent {
                position: [0, -4, 0],
                result: SubChunkResult::Unavailable(SubChunkUnavailable::ChunkNotFound),
            }],
        });
        assert!(matches!(
            prepared[0].result,
            super::PreparedSubChunkResult::Unavailable(SubChunkUnavailable::ChunkNotFound)
        ));

        let mut stream = WorldStream::new(WorldBootstrap {
            dimension: 0,
            local_player_runtime_id: 1,
            player_position: [0.0; 3],
            world_spawn_position: [0; 3],
            air_network_id: 12_530,
            block_network_ids_are_hashes: false,
        });
        let chunk = ChunkKey::new(0, 0, 0);
        stream.requested_sub_chunks.insert(
            chunk,
            BTreeMap::from([(-4, Default::default()), (-3, Default::default())]),
        );
        apply_sub_chunk_result(
            &mut stream,
            SubChunkKey::from_chunk(chunk, -4),
            super::PreparedSubChunkResult::AllAir,
        );
        apply_sub_chunk_result(
            &mut stream,
            SubChunkKey::from_chunk(chunk, -3),
            super::PreparedSubChunkResult::Unavailable(SubChunkUnavailable::YIndexOutOfBounds),
        );

        assert!(!stream.requested_sub_chunks.contains_key(&chunk));
        assert!(stream.loaded_columns.contains(&chunk));
        assert!(
            stream
                .known_air
                .contains(&SubChunkKey::from_chunk(chunk, -3))
        );
    }

    #[test]
    fn transient_unavailable_results_retry_boundedly_then_complete_without_wedging() {
        for unavailable in [
            SubChunkUnavailable::ChunkNotFound,
            SubChunkUnavailable::PlayerNotFound,
        ] {
            let (mut stream, key) = stream_with_one_expected_sub_chunk();
            for attempt in 0..=super::MAX_SUB_CHUNK_RETRIES {
                apply_sub_chunk_result(
                    &mut stream,
                    key,
                    super::PreparedSubChunkResult::Unavailable(unavailable),
                );
                if attempt < super::MAX_SUB_CHUNK_RETRIES {
                    assert_eq!(stream.pending_request_count(), 1);
                    assert!(stream.requested_sub_chunks.contains_key(&key.chunk()));
                    stream.take_requests();
                }
            }
            assert!(!stream.requested_sub_chunks.contains_key(&key.chunk()));
            assert!(stream.loaded_columns.contains(&key.chunk()));
            assert_eq!(stream.pending_request_count(), 0);
        }
    }

    #[test]
    fn decode_failures_retry_boundedly_and_invalid_dimension_is_terminal_normalization() {
        let (mut stream, key) = stream_with_one_expected_sub_chunk();
        for attempt in 0..=super::MAX_SUB_CHUNK_RETRIES {
            apply_sub_chunk_result(
                &mut stream,
                key,
                super::PreparedSubChunkResult::Decoded(Err(
                    world::DecodeError::UnsupportedVersion(255),
                )),
            );
            if attempt < super::MAX_SUB_CHUNK_RETRIES {
                assert_eq!(stream.take_requests().len(), 1);
            }
        }
        assert!(stream.loaded_columns.contains(&key.chunk()));
        assert!(!stream.requested_sub_chunks.contains_key(&key.chunk()));

        let (mut stream, key) = stream_with_one_expected_sub_chunk();
        apply_sub_chunk_result(
            &mut stream,
            key,
            super::PreparedSubChunkResult::Unavailable(SubChunkUnavailable::InvalidDimension),
        );
        assert_eq!(stream.stats().normalization_errors, 1);
        assert!(stream.loaded_columns.contains(&key.chunk()));
        assert!(!stream.requested_sub_chunks.contains_key(&key.chunk()));
    }

    #[test]
    fn request_mode_evicts_the_old_column_and_invalidates_its_neighbours() {
        let mut stream = WorldStream::new(WorldBootstrap {
            dimension: 0,
            local_player_runtime_id: 1,
            player_position: [0.0; 3],
            world_spawn_position: [0; 3],
            air_network_id: 12_530,
            block_network_ids_are_hashes: false,
        });
        let key = SubChunkKey::new(0, 3, -4, -2);
        let decoded = DecodedLevelChunk::decode(
            -4,
            1,
            include_bytes!("../../crates/world/fixtures/uniform_non_air.bin"),
        )
        .unwrap();
        stream.store.commit_level_chunk(key.chunk(), decoded);
        stream.resident.insert(key);

        stream
            .submit(
                1,
                WorldEvent::LevelChunk(LevelChunkEvent {
                    dimension: 0,
                    x: key.x,
                    z: key.z,
                    mode: LevelChunkMode::LimitedRequests { highest: 1 },
                    payload: biome_payload(0, 1),
                }),
            )
            .unwrap();
        complete_pending_decode_jobs(&mut stream);

        assert!(stream.store.sub_chunk(key).is_none());
        assert!(!stream.resident.contains(&key));
        assert_eq!(stream.take_requests().len(), 1);
        assert_eq!(
            stream
                .pending_mesh
                .keys()
                .copied()
                .collect::<std::collections::BTreeSet<_>>(),
            key.mesh_dependents().collect()
        );
    }

    #[test]
    fn changed_sub_chunk_dirties_center_and_six_face_neighbours_once() {
        let mut stream = WorldStream::new(WorldBootstrap {
            dimension: 0,
            local_player_runtime_id: 1,
            player_position: [0.0; 3],
            world_spawn_position: [0; 3],
            air_network_id: 12_530,
            block_network_ids_are_hashes: false,
        });
        let key = SubChunkKey::new(0, 4, -2, 9);

        stream.mark_changed(key, Instant::now());
        let expected = key
            .mesh_dependents()
            .collect::<std::collections::BTreeSet<_>>();
        let actual = stream
            .pending_mesh
            .keys()
            .copied()
            .collect::<std::collections::BTreeSet<_>>();

        assert_eq!(actual, expected);
        assert_eq!(stream.pending_mesh.len(), 7);
    }

    #[test]
    fn stale_mesh_completion_cannot_replace_current_revision() {
        let mut stream = WorldStream::new(WorldBootstrap {
            dimension: 0,
            local_player_runtime_id: 1,
            player_position: [0.0; 3],
            world_spawn_position: [0; 3],
            air_network_id: 12_530,
            block_network_ids_are_hashes: false,
        });
        let key = SubChunkKey::new(0, 0, -4, 0);
        let decoded = DecodedLevelChunk::decode(
            -4,
            1,
            include_bytes!("../../crates/world/fixtures/uniform_non_air.bin"),
        )
        .unwrap();
        stream
            .store
            .commit_level_chunk(ChunkKey::new(0, 0, 0), decoded);
        let source = stream.store.sub_chunk(key).unwrap();
        stream.resident.insert(key);
        let old_revision = stream.mark_dirty_exact(key, Instant::now());
        let current_revision = stream.mark_dirty_exact(key, Instant::now());
        stream.in_flight.insert(key, old_revision);
        let classifier = BlockClassifier::new(12_530);
        let mesh = mesh_sub_chunk(
            &classifier,
            &stream.runtime_assets,
            stream.network_id_mode,
            &Neighbourhood::empty(),
            &source,
        );
        let tint_identity = stream.biome_tint_identity();

        stream.accept_mesh_completion(MeshCompletion {
            key,
            revision: old_revision,
            source: Arc::clone(&source),
            biome_source: None,
            biome: PackedBiomeRecord::fallback(),
            tint_identity,
            mesh,
            dependency_mask: MeshDependencyMask::new(false, true),
            duration: std::time::Duration::ZERO,
        });

        assert!(stream.revisions.is_current(key, current_revision));
        assert_eq!(stream.stats().stale_mesh_jobs, 1);
        assert!(stream.take_mesh_changes().is_empty());
        assert_eq!(stream.mesh_dependency_mask(key), None);
        assert_eq!(stream.pending_mesh[&key].revision, current_revision);
    }

    #[test]
    fn mesh_dispatch_never_exceeds_the_bounded_worker_window() {
        let mut stream = WorldStream::new(WorldBootstrap {
            dimension: 0,
            local_player_runtime_id: 1,
            player_position: [0.0; 3],
            world_spawn_position: [0; 3],
            air_network_id: 12_530,
            block_network_ids_are_hashes: false,
        });
        let key = SubChunkKey::new(0, 0, -4, 0);
        let decoded = DecodedLevelChunk::decode(
            -4,
            1,
            include_bytes!("../../crates/world/fixtures/uniform_non_air.bin"),
        )
        .unwrap();
        stream.store.commit_level_chunk(key.chunk(), decoded);
        stream.resident.insert(key);
        stream.mark_changed(key, Instant::now());
        for index in 0..super::WORK_RESULT_CAPACITY {
            stream
                .in_flight
                .insert(SubChunkKey::new(7, index as i32, 0, 0), index as u64 + 1);
        }

        assert_eq!(stream.dispatch_mesh_jobs([0.0; 3], 1), 0);
        assert_eq!(stream.in_flight.len(), super::WORK_RESULT_CAPACITY);
    }

    #[test]
    fn mesh_removals_are_not_blocked_by_a_full_worker_window() {
        let mut stream = WorldStream::new(WorldBootstrap {
            dimension: 0,
            local_player_runtime_id: 1,
            player_position: [0.0; 3],
            world_spawn_position: [0; 3],
            air_network_id: 12_530,
            block_network_ids_are_hashes: false,
        });
        let removed = SubChunkKey::new(0, 0, -4, 0);
        stream.mark_dirty_exact(removed, Instant::now());
        for index in 0..super::WORK_RESULT_CAPACITY {
            stream
                .in_flight
                .insert(SubChunkKey::new(7, index as i32, 0, 0), index as u64 + 1);
        }

        assert_eq!(stream.dispatch_mesh_jobs([0.0; 3], 1), 0);
        let changes = stream.take_mesh_changes();
        assert_eq!(changes.len(), 1);
        assert_eq!(changes[0].key(), removed);
        assert!(matches!(changes[0], super::WorldMeshChange::Remove { .. }));
    }

    #[test]
    fn final_block_removal_latency_waits_for_exact_applied_acknowledgement() {
        let mut stream = WorldStream::new(WorldBootstrap {
            dimension: 0,
            local_player_runtime_id: 1,
            player_position: [0.0; 3],
            world_spawn_position: [0; 3],
            air_network_id: 12_530,
            block_network_ids_are_hashes: false,
        });
        let key = SubChunkKey::new(0, 0, -4, 0);
        stream
            .store
            .update_block(key, BlockUpdate::new(0, 0, 0, 0, 99), 12_530)
            .unwrap();
        stream
            .store
            .update_block(key, BlockUpdate::new(0, 0, 0, 0, 12_530), 12_530)
            .unwrap();
        let dirty_since = Instant::now();
        stream.mark_dirty_exact(key, dirty_since);
        let generation = stream.revisions.dirty(key).unwrap().revision;

        stream.dispatch_mesh_jobs([0.0; 3], 0);

        assert_eq!(stream.stats().max_remesh_latency, std::time::Duration::ZERO);
        assert!(
            stream.revisions.is_current(key, generation),
            "queued removal must retain its dirty revision until render application"
        );
        let change = stream.pop_mesh_change().unwrap();
        assert_eq!(change.key(), key);
        stream.acknowledge_mesh_upload(
            key,
            generation + 1,
            dirty_since,
            dirty_since + std::time::Duration::from_millis(40),
        );
        assert_eq!(stream.stats().max_remesh_latency, std::time::Duration::ZERO);
        stream.acknowledge_mesh_upload(
            key,
            generation,
            dirty_since,
            dirty_since + std::time::Duration::from_millis(40),
        );
        assert_eq!(
            stream.stats().max_remesh_latency,
            std::time::Duration::from_millis(40)
        );
    }

    #[test]
    fn cave_bfs_traverses_a_known_all_air_node_between_rendered_chunks() {
        let mut stream = WorldStream::new(WorldBootstrap {
            dimension: 0,
            local_player_runtime_id: 1,
            player_position: [0.0; 3],
            world_spawn_position: [0; 3],
            air_network_id: 12_530,
            block_network_ids_are_hashes: false,
        });
        let left = SubChunkKey::new(0, -1, 0, 0);
        let air = SubChunkKey::new(0, 0, 0, 0);
        let right = SubChunkKey::new(0, 1, 0, 0);
        stream
            .connectivity
            .insert(left, render::FaceConnectivity::all());
        stream
            .connectivity
            .insert(right, render::FaceConnectivity::all());
        stream.record_known_air(air);
        stream.mark_dirty_exact(air, Instant::now());

        stream.dispatch_mesh_jobs([0.0; 3], 0);

        assert_eq!(
            stream.connectivity(air),
            Some(render::FaceConnectivity::all())
        );
        let visible = stream.cave_visible_sub_chunks(left);
        assert!(visible.contains(&air));
        assert!(visible.contains(&right));
    }

    #[test]
    fn leaf_slab_connectivity_crosses_world_cave_graph_but_opaque_slab_stops_it() {
        let runtime_assets = Arc::new(cave_test_assets());
        let classifier = BlockClassifier::new(0);
        let leaf = cave_test_slab(1);
        let opaque = cave_test_slab(2);
        let leaf_mesh = mesh_sub_chunk(
            &classifier,
            &runtime_assets,
            NetworkIdMode::Sequential,
            &Neighbourhood::empty(),
            &leaf,
        );
        let opaque_mesh = mesh_sub_chunk(
            &classifier,
            &runtime_assets,
            NetworkIdMode::Sequential,
            &Neighbourhood::empty(),
            &opaque,
        );
        assert!(leaf_mesh.connectivity().is_all_connected());
        assert!(
            !opaque_mesh
                .connectivity()
                .is_connected(render::Face::NegativeX, render::Face::PositiveX)
        );

        let mut stream = WorldStream::new_with_assets(
            WorldBootstrap {
                dimension: 0,
                local_player_runtime_id: 1,
                player_position: [0.0; 3],
                world_spawn_position: [0; 3],
                air_network_id: 0,
                block_network_ids_are_hashes: false,
            },
            runtime_assets,
            [0.0; 3],
            None,
        );
        let left = SubChunkKey::new(0, -1, 0, 0);
        let middle = SubChunkKey::new(0, 0, 0, 0);
        let right = SubChunkKey::new(0, 1, 0, 0);
        let beyond_shell = SubChunkKey::new(0, 2, 0, 0);
        stream.set_connectivity(left, Some(render::FaceConnectivity::all()));
        stream.set_connectivity(right, Some(render::FaceConnectivity::all()));
        stream.set_connectivity(beyond_shell, Some(render::FaceConnectivity::all()));
        stream.set_connectivity(middle, Some(leaf_mesh.connectivity()));

        let through_leaf = stream.cave_visible_sub_chunks(left);
        assert!(through_leaf.contains(&middle));
        assert!(through_leaf.contains(&right));
        assert!(through_leaf.contains(&beyond_shell));

        stream.set_connectivity(middle, Some(opaque_mesh.connectivity()));
        let stopped_by_opaque = stream.cave_visible_sub_chunks(left);
        assert!(stopped_by_opaque.contains(&middle));
        assert!(stopped_by_opaque.contains(&right));
        assert!(!stopped_by_opaque.contains(&beyond_shell));
    }

    #[test]
    fn inline_zero_storage_is_a_graph_node_until_column_eviction() {
        let mut stream = WorldStream::new(WorldBootstrap {
            dimension: 0,
            local_player_runtime_id: 1,
            player_position: [0.0; 3],
            world_spawn_position: [0; 3],
            air_network_id: 12_530,
            block_network_ids_are_hashes: false,
        });
        let chunk = ChunkKey::new(0, 2, -3);
        let key = SubChunkKey::from_chunk(chunk, -4);
        let payload = [9, 0, (-4_i8) as u8];
        let decoded = DecodedLevelChunk::decode(-4, 1, &payload).unwrap();
        stream.apply_prepared(super::PreparedWorldEvent::InlineLevelChunk {
            event: LevelChunkEvent {
                dimension: 0,
                x: chunk.x,
                z: chunk.z,
                mode: LevelChunkMode::Inline { count: 1 },
                payload: payload.to_vec(),
            },
            decoded: Ok(decoded),
            duration: std::time::Duration::ZERO,
        });

        assert!(stream.store.sub_chunk(key).is_none());
        assert!(stream.resident.contains(&key));
        assert!(stream.known_air.contains(&key));
        assert_eq!(
            stream.connectivity(key),
            Some(render::FaceConnectivity::all())
        );

        stream.evict_column(chunk);
        assert!(!stream.resident.contains(&key));
        assert!(!stream.known_air.contains(&key));
        assert_eq!(stream.connectivity(key), None);
    }

    #[test]
    fn explicit_all_air_result_is_counted_as_a_resident_graph_node() {
        let mut stream = WorldStream::new(WorldBootstrap {
            dimension: 1,
            local_player_runtime_id: 1,
            player_position: [0.0; 3],
            world_spawn_position: [0; 3],
            air_network_id: 12_530,
            block_network_ids_are_hashes: false,
        });
        let key = SubChunkKey::new(1, -8, 3, 12);
        stream
            .requested_sub_chunks
            .insert(key.chunk(), BTreeMap::from([(key.y, Default::default())]));
        stream.apply_prepared(super::PreparedWorldEvent::SubChunks {
            dimension: key.dimension,
            entries: vec![super::PreparedSubChunk {
                position: [key.x, key.y, key.z],
                result: super::PreparedSubChunkResult::AllAir,
            }],
            duration: std::time::Duration::ZERO,
        });

        assert!(stream.resident.contains(&key));
        assert!(stream.known_air.contains(&key));
        assert_eq!(
            stream.connectivity(key),
            Some(render::FaceConnectivity::all())
        );
        assert_eq!(stream.stats().resident_sub_chunks, 1);
    }

    #[test]
    fn connectivity_generation_changes_only_when_the_graph_changes() {
        let mut stream = WorldStream::new(WorldBootstrap {
            dimension: 0,
            local_player_runtime_id: 1,
            player_position: [0.0; 3],
            world_spawn_position: [0; 3],
            air_network_id: 12_530,
            block_network_ids_are_hashes: false,
        });
        let key = SubChunkKey::new(0, 2, -1, 4);

        assert_eq!(stream.connectivity_generation(), 0);
        stream.record_known_air(key);
        let inserted = stream.connectivity_generation();
        assert_ne!(inserted, 0);

        stream.record_known_air(key);
        assert_eq!(stream.connectivity_generation(), inserted);

        stream.evict_column(key.chunk());
        assert_ne!(stream.connectivity_generation(), inserted);
    }

    #[test]
    fn surface_spawn_waits_for_level_chunk_commit_and_treats_omitted_top_as_air() {
        let mut stream = WorldStream::new(WorldBootstrap {
            dimension: 0,
            local_player_runtime_id: 1,
            player_position: [0.0; 3],
            world_spawn_position: [0; 3],
            air_network_id: 12_530,
            block_network_ids_are_hashes: false,
        });
        let chunk = ChunkKey::new(0, 2, -3);
        let block_x = chunk.x * 16 + 5;
        let block_z = chunk.z * 16 + 7;
        assert_eq!(stream.surface_eye_position(block_x, block_z), None);

        let payload = include_bytes!("../../crates/world/fixtures/uniform_non_air.bin");
        let decoded = DecodedLevelChunk::decode(-4, 1, payload).unwrap();
        stream.apply_prepared(super::PreparedWorldEvent::InlineLevelChunk {
            event: LevelChunkEvent {
                dimension: 0,
                x: chunk.x,
                z: chunk.z,
                mode: LevelChunkMode::Inline { count: 1 },
                payload: payload.to_vec(),
            },
            decoded: Ok(decoded),
            duration: std::time::Duration::ZERO,
        });

        assert_eq!(
            stream.surface_eye_position(block_x, block_z),
            Some([block_x as f32 + 0.5, -46.38, block_z as f32 + 0.5])
        );
    }

    enum Action {
        Decode(DecodedLevelChunk),
        Update,
    }
}
