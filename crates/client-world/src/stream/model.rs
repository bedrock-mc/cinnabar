use super::*;

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub(super) struct PendingSubChunk {
    pub(super) retry_attempts: u8,
    pub(super) pending_transport_attempts: u8,
    pub(super) confirmed_attempts: u8,
    pub(super) response_deadline: Option<Instant>,
}

pub(super) type PendingSubChunkColumn = BTreeMap<i32, PendingSubChunk>;

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub(super) struct CorrelatedSubChunkAttempts {
    pub(super) pending_transport_attempts: u8,
    pub(super) confirmed_attempts: u8,
}

/// Raw block-space witness for a server publisher view.
///
/// The containing chunk and ceiling chunk radius remain on [`ViewCohort`] for
/// bounded retention. This identity preserves values that would otherwise be
/// lost when an unaligned block centre or radius is converted to chunks.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct PublisherViewGeometry {
    pub center_blocks: [i32; 2],
    pub radius_blocks: u32,
}

/// One horizontal view cohort with separate required and retention geometry.
///
/// `center` and `radius` define the enclosing Chebyshev retention square in
/// chunk columns. Publisher-created cohorts additionally retain the raw wire
/// witness, but the protocol does not define an enumerable universal column
/// set from that witness. Required membership is recorded separately from
/// unique request-mode `LevelChunk` announcements in the publisher epoch.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ViewCohort {
    pub dimension: i32,
    pub center: [i32; 2],
    pub radius: i32,
    pub publisher_geometry: Option<PublisherViewGeometry>,
}

impl ViewCohort {
    #[must_use]
    pub fn from_publisher(dimension: i32, center: [i32; 3], radius_blocks: u32) -> Self {
        let chunks = radius_blocks.saturating_add(15) / 16;
        Self {
            dimension,
            center: [center[0].div_euclid(16), center[2].div_euclid(16)],
            radius: i32::try_from(chunks).unwrap_or(i32::MAX),
            publisher_geometry: Some(PublisherViewGeometry {
                center_blocks: [center[0], center[2]],
                radius_blocks,
            }),
        }
    }

    #[must_use]
    pub fn contains_column(self, dimension: i32, column: [i32; 2]) -> bool {
        dimension == self.dimension
            && i64::from(column[0]).abs_diff(i64::from(self.center[0])) <= self.radius.max(0) as u64
            && i64::from(column[1]).abs_diff(i64::from(self.center[1])) <= self.radius.max(0) as u64
    }

    /// Returns a static diagnostic classifier, never publisher readiness.
    ///
    /// Manually constructed cohorts use the legacy Euclidean chunk disk.
    /// Publisher witnesses use Dragonfly's attributable `distance < r - 0.5`
    /// policy; other servers may announce a different set.
    #[must_use]
    pub fn classifier_columns(self) -> BTreeSet<ChunkKey> {
        let radius = self.radius.max(0);
        let doubled_limit = self.publisher_geometry.map_or_else(
            || i64::from(radius).saturating_mul(2),
            |_| i64::from(radius).saturating_mul(2).saturating_sub(1),
        );
        (-radius..=radius)
            .flat_map(|x_offset| {
                (-radius..=radius)
                    .filter(move |z_offset| {
                        let x = i64::from(x_offset).unsigned_abs().saturating_mul(2);
                        let z = i64::from(*z_offset).unsigned_abs().saturating_mul(2);
                        x.saturating_mul(x).saturating_add(z.saturating_mul(z))
                            <= doubled_limit
                                .unsigned_abs()
                                .saturating_mul(doubled_limit.unsigned_abs())
                    })
                    .map(move |z_offset| {
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
    pub publisher_epoch: u64,
    pub expected: usize,
    pub required_hash: u64,
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
            && self.expected != 0
            && self.loaded_target == self.expected
            && self.missing_target == 0
            && self.foreign_loaded == 0
            && self.foreign_requested == 0
            && self.foreign_resident == 0
            && self.source_leftover == 0
    }
}

#[derive(Debug)]
pub(super) struct SequenceBuffer<T> {
    pub(super) next: u64,
    pub(super) ready: BTreeMap<u64, T>,
}

impl<T> SequenceBuffer<T> {
    pub(super) fn new(first_sequence: u64) -> Self {
        Self {
            next: first_sequence,
            ready: BTreeMap::new(),
        }
    }

    pub(super) fn insert(&mut self, sequence: u64, value: T) -> Result<(), SequenceError> {
        if sequence < self.next || self.ready.contains_key(&sequence) {
            return Err(SequenceError::DuplicateOrPast {
                sequence,
                next: self.next,
            });
        }
        self.ready.insert(sequence, value);
        Ok(())
    }

    pub(super) fn pop_next(&mut self) -> Option<T> {
        let value = self.ready.remove(&self.next)?;
        self.next = self.next.saturating_add(1);
        Some(value)
    }

    pub(super) const fn next_sequence(&self) -> u64 {
        self.next
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Error)]
pub(super) enum SequenceError {
    #[error("world sequence {sequence} is duplicate or older than next sequence {next}")]
    DuplicateOrPast { sequence: u64, next: u64 },
}

#[derive(Debug, Clone, Copy)]
pub(super) struct DirtyRevision {
    pub(super) revision: u64,
    pub(super) since: Instant,
}

#[derive(Debug, Default)]
pub(super) struct RevisionTracker {
    pub(super) entries: HashMap<SubChunkKey, DirtyRevision>,
    pub(super) next_revision: u64,
}

impl RevisionTracker {
    pub(super) fn mark_dirty(&mut self, key: SubChunkKey, now: Instant) -> u64 {
        self.next_revision = self.next_revision.wrapping_add(1).max(1);
        let revision = self.next_revision;
        let entry = self.entries.entry(key).or_insert(DirtyRevision {
            revision,
            since: now,
        });
        entry.revision = revision;
        entry.revision
    }

    pub(super) fn is_current(&self, key: SubChunkKey, revision: u64) -> bool {
        self.entries
            .get(&key)
            .is_some_and(|entry| entry.revision == revision)
    }

    pub(super) fn force_dirty_since(&mut self, key: SubChunkKey, now: Instant) -> u64 {
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

    pub(super) fn dirty(&self, key: SubChunkKey) -> Option<DirtyRevision> {
        self.entries.get(&key).copied()
    }

    pub(super) fn clear_if_current(&mut self, key: SubChunkKey, revision: u64) {
        if self.is_current(key, revision) {
            self.entries.remove(&key);
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Error)]
pub(super) enum BlockUpdateConversionError {
    #[error("block update layer {0} cannot fit the world mutation layer type")]
    LayerOverflow(usize),
}

pub(super) fn split_block_update(
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

pub(super) enum OutboundRequestSlot {
    Reserved(u64),
    Ready(PendingSubChunkRequest),
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) enum RetrySchedule {
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
        permit: Option<PublicationPermit>,
    },
    Remove {
        key: SubChunkKey,
        generation: u64,
        dirty_since: Instant,
        permit: Option<PublicationPermit>,
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
    PlayerMovementCorrection {
        sequence: u64,
        correction: PlayerMovementCorrectionEvent,
        resolved: ResolvedServerPosition,
    },
    ChangeDimension {
        change: ChangeDimensionEvent,
        resolved: ResolvedServerPosition,
    },
    SetTime {
        sequence: u64,
        update: SetTimeEvent,
    },
    DaylightCycle {
        sequence: u64,
        update: DaylightCycleUpdateEvent,
    },
    Weather {
        sequence: u64,
        update: WeatherUpdateEvent,
    },
}

#[derive(Debug, Clone, PartialEq)]
pub enum CommittedUiEvent {
    Ui {
        sequence: u64,
        event: UiEvent,
    },
    BlockCrack {
        sequence: u64,
        dimension: i32,
        event: BlockCrackEvent,
    },
    LocalAttributes {
        sequence: u64,
        server_tick: u64,
        attributes: Arc<[ActorAttribute]>,
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
    pub inactive_block_entity_updates: u64,
    pub invalid_block_entity_positions: u64,
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
            self.inactive_block_entity_updates,
            self.invalid_block_entity_positions,
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

pub(super) enum NormalizationErrorReason {
    OrderedCompletionRejection,
    InactiveBlockUpdate,
    InactiveBlockEntityUpdate,
    InvalidBlockEntityPosition,
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
    pub(super) fn record(&mut self, reason: NormalizationErrorReason) {
        let counter = match reason {
            NormalizationErrorReason::OrderedCompletionRejection => {
                &mut self.ordered_completion_rejections
            }
            NormalizationErrorReason::InactiveBlockUpdate => &mut self.inactive_block_updates,
            NormalizationErrorReason::InactiveBlockEntityUpdate => {
                &mut self.inactive_block_entity_updates
            }
            NormalizationErrorReason::InvalidBlockEntityPosition => {
                &mut self.invalid_block_entity_positions
            }
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
    pub phase2_stages: PublicationStageCounters,
    pub phase2_outcomes: SubChunkOutcomeCounters,
    pub decode_errors: u64,
    pub normalization_errors: u64,
    pub normalization_reasons: WorldStreamNormalizationStats,
    pub unavailable_sub_chunks: u64,
    pub stale_mesh_jobs: u64,
    pub stale_light_jobs: u64,
    pub light_solve_failures: u64,
    pub light_uniform_fast_path_jobs: u64,
    pub accepted_light_jobs: u64,
    pub noop_light_jobs: u64,
    pub value_changed_light_jobs: u64,
    pub provenance_only_light_jobs: u64,
    pub light_mesh_invalidations: u64,
    pub received_radius_chunks: Option<i32>,
    pub publisher_radius_chunks: Option<i32>,
    pub resident_sub_chunks: usize,
    pub adjudicated_static_block_entities: usize,
    pub adjudicated_logical_block_entities: usize,
    pub deferred_block_entities: usize,
    pub unknown_block_entities: usize,
    pub pending_mesh_jobs: usize,
    pub in_flight_mesh_jobs: usize,
    pub pending_light_jobs: usize,
    pub in_flight_light_jobs: usize,
    pub terminal_light_failures: usize,
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
    pub max_decode_queue_wait: Duration,
    pub max_light_queue_wait: Duration,
    pub max_mesh_queue_wait: Duration,
    pub max_decode_duration: Duration,
    pub max_mesh_duration: Duration,
    pub max_light_duration: Duration,
    pub max_remesh_latency: Duration,
    pub last_chunk_commit_at: Option<Instant>,
    pub last_mesh_dispatch_at: Option<Instant>,
    pub last_mesh_completion_at: Option<Instant>,
    pub last_mesh_ack_at: Option<Instant>,
}

impl WorldStreamStats {
    pub(super) fn observe_decode_queue_wait(&mut self, queue_wait: Duration) {
        self.max_decode_queue_wait = self.max_decode_queue_wait.max(queue_wait);
    }

    pub(super) fn observe_light_queue_wait(&mut self, queue_wait: Duration) {
        self.max_light_queue_wait = self.max_light_queue_wait.max(queue_wait);
    }

    pub(super) fn observe_mesh_queue_wait(&mut self, queue_wait: Duration) {
        self.max_mesh_queue_wait = self.max_mesh_queue_wait.max(queue_wait);
    }
}

pub(super) fn queue_wait(queued_at: Instant, started_at: Instant) -> Duration {
    started_at.saturating_duration_since(queued_at)
}

/// Work performed by one call to [`WorldStream::poll`].
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct WorldStreamPoll {
    pub decoded_results: usize,
    pub light_results: usize,
    pub light_jobs_dispatched: usize,
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

#[derive(Debug, Clone, Copy, PartialEq, Eq, Error)]
pub enum WorldStreamFatalError {
    #[error("light solve failed for {key:?}: {error}")]
    LightSolve {
        key: SubChunkKey,
        error: LightSolveError,
    },
    #[error("light solve for {key:?} returned no target output")]
    MissingLightTarget { key: SubChunkKey },
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
pub(super) enum PreparedWorldEvent {
    InlineLevelChunk {
        event: LevelChunkEvent,
        decoded: Result<DecodedLevelChunk, DecodeError>,
        duration: Duration,
    },
    RequestLevelChunk {
        event: LevelChunkEvent,
        decoded: Result<(DecodedBiomeColumn, DecodedBlockEntities), DecodeError>,
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
    BlockEntityUpdate {
        key: BlockEntityKey,
        decoded: Result<BlockEntityNbt, BlockEntityError>,
        duration: Duration,
    },
    Immediate(WorldEvent),
    SameLocationReset,
    CommitOnly,
    NormalizationFailure,
}

#[derive(Debug)]
pub(super) struct PreparedSubChunk {
    pub(super) position: [i32; 3],
    pub(super) result: PreparedSubChunkResult,
}

#[derive(Debug)]
pub(super) enum PreparedSubChunkResult {
    Decoded(Result<DecodedSubChunk, DecodeError>),
    AllAir,
    Unavailable(protocol::SubChunkUnavailable),
}

#[derive(Debug)]
pub(super) struct DecodeCompletion {
    pub(super) sequence: u64,
    pub(super) event: PreparedWorldEvent,
    pub(super) queue_wait: Duration,
}

#[derive(Debug)]
pub(super) struct QueuedDecodeJob {
    pub(super) queued_at: Instant,
    pub(super) job: DecodeJob,
}

#[derive(Debug)]
pub(super) enum DecodeJob {
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
    BlockEntityUpdate {
        sequence: u64,
        event: BlockEntityUpdateEvent,
    },
}

#[derive(Debug)]
pub(super) struct BlockMutationBatch {
    pub(super) key: SubChunkKey,
    pub(super) previous: Option<Arc<SubChunk>>,
    pub(super) updates: Vec<BlockUpdate>,
}

#[derive(Debug, Clone, Copy)]
pub(super) struct PendingMesh {
    pub(super) revision: u64,
    pub(super) since: Instant,
    pub(super) queued_at: Instant,
}

#[derive(Debug)]
pub(super) struct MeshCompletion {
    pub(super) key: SubChunkKey,
    pub(super) revision: u64,
    pub(super) source: Arc<SubChunk>,
    pub(super) biome_sources: BiomeNeighbourhood,
    pub(super) biome: PackedBiomeRecord,
    pub(super) tint_identity: ChunkBiomeTintIdentity,
    pub(super) mesh: ChunkMesh,
    pub(super) dependency_mask: MeshDependencyMask,
    pub(super) light_halo: MeshLightHalo,
    pub(super) queue_wait: Duration,
    pub(super) duration: Duration,
}
