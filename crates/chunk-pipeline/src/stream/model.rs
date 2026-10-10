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

/// How much of one view's required terrain has loaded: the readiness half of
/// [`ViewCohortStatus`], without its diagnostic scan of resident sub-chunks.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct CohortProgress {
    pub target: ViewCohort,
    pub committed: Option<ViewCohort>,
    pub expected: usize,
    pub loaded_target: usize,
}

impl CohortProgress {
    /// Whether the committed view is `target` and every one of its required columns loaded,
    /// exactly as [`ViewCohortStatus::target_is_complete`] decides it.
    #[must_use]
    pub fn target_is_complete(self) -> bool {
        self.committed == Some(self.target)
            && self.expected != 0
            && self.loaded_target == self.expected
    }
}

impl From<ViewCohortStatus> for CohortProgress {
    /// Keeps the readiness fields without retaining the full diagnostic witness.
    fn from(status: ViewCohortStatus) -> Self {
        Self {
            target: status.target,
            committed: status.committed,
            expected: status.expected,
            loaded_target: status.loaded_target,
        }
    }
}

impl ViewCohortStatus {
    /// Requires the target view to be committed with every required column loaded.
    #[must_use]
    pub fn target_is_complete(self) -> bool {
        self.committed == Some(self.target)
            && self.expected != 0
            && self.loaded_target == self.expected
            && self.missing_target == 0
    }

    #[must_use]
    pub fn is_exact(self) -> bool {
        self.target_is_complete()
            && self.foreign_loaded == 0
            && self.foreign_requested == 0
            && self.foreign_resident == 0
            && self.source_leftover == 0
    }
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
        output_permit: Option<super::meshing::memory::MeshMemoryPermit>,
        key: SubChunkKey,
        mesh: ChunkMesh,
        biome: PackedBiomeRecord,
        tint_identity: ChunkBiomeTintIdentity,
        generation: u64,
        dirty_since: Instant,
        urgent: bool,
        permit: Option<PublicationPermit>,
    },
    Remove {
        key: SubChunkKey,
        generation: u64,
        dirty_since: Instant,
        urgent: bool,
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

impl WorldMeshChange {
    #[must_use]
    pub const fn key(&self) -> SubChunkKey {
        match self {
            Self::Upsert { key, .. } | Self::Remove { key, .. } => *key,
        }
    }

    #[must_use]
    pub const fn generation(&self) -> u64 {
        match self {
            Self::Upsert { generation, .. } | Self::Remove { generation, .. } => *generation,
        }
    }

    const fn is_urgent(&self) -> bool {
        match self {
            Self::Upsert { urgent, .. } | Self::Remove { urgent, .. } => *urgent,
        }
    }
}

/// Publication FIFO holding at most one change per key, so the renderer's last-write-wins
/// queue can never apply an older change after a newer one that jumped ahead.
#[derive(Debug)]
pub(super) struct MeshChangeQueue {
    changes: VecDeque<WorldMeshChange>,
    keys: HashSet<SubChunkKey>,
}

impl Default for MeshChangeQueue {
    fn default() -> Self {
        Self {
            changes: VecDeque::with_capacity(MAX_PENDING_MESH_CHANGES),
            keys: HashSet::with_capacity(MAX_PENDING_MESH_CHANGES),
        }
    }
}

impl MeshChangeQueue {
    /// Queues urgent changes first and non-urgent ones last; true when a change for the same
    /// key was superseded and dropped undelivered.
    pub(super) fn push(&mut self, change: WorldMeshChange) -> bool {
        let front = change.is_urgent();
        self.insert(change, front)
    }

    /// Re-queues a change the renderer rejected at the head of the FIFO.
    pub(super) fn push_front(&mut self, change: WorldMeshChange) -> bool {
        self.insert(change, true)
    }

    /// Dropping the older change for a key releases its permits; a newer queued change wins.
    fn insert(&mut self, change: WorldMeshChange, front: bool) -> bool {
        let key = change.key();
        let superseded = !self.keys.insert(key);
        if superseded {
            let index = self
                .changes
                .iter()
                .position(|queued| queued.key() == key)
                .expect("every tracked key has one queued change");
            if self.changes[index].generation() > change.generation() {
                return true;
            }
            self.changes.remove(index);
        }
        if front {
            self.changes.push_front(change);
        } else {
            self.changes.push_back(change);
        }
        superseded
    }

    pub(super) fn pop_front(&mut self) -> Option<WorldMeshChange> {
        let change = self.changes.pop_front()?;
        self.keys.remove(&change.key());
        Some(change)
    }

    pub(super) fn drain(&mut self) -> std::collections::vec_deque::Drain<'_, WorldMeshChange> {
        self.keys.clear();
        self.changes.drain(..)
    }

    /// Returns how many changes were dropped undelivered.
    pub(super) fn retain(&mut self, mut keep: impl FnMut(&WorldMeshChange) -> bool) -> usize {
        let before = self.changes.len();
        let keys = &mut self.keys;
        self.changes.retain(|change| {
            let kept = keep(change);
            if !kept {
                keys.remove(&change.key());
            }
            kept
        });
        before - self.changes.len()
    }

    pub(super) fn iter(&self) -> std::collections::vec_deque::Iter<'_, WorldMeshChange> {
        self.changes.iter()
    }

    pub(super) fn len(&self) -> usize {
        self.changes.len()
    }

    pub(super) fn is_empty(&self) -> bool {
        self.changes.is_empty()
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
    pub invalid_actor_block_syncs: u64,
    pub actor_block_sync_capacity_failures: u64,
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
            self.invalid_actor_block_syncs,
            self.actor_block_sync_capacity_failures,
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
    InvalidActorBlockSync,
    ActorBlockSyncCapacity,
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
            NormalizationErrorReason::InvalidActorBlockSync => &mut self.invalid_actor_block_syncs,
            NormalizationErrorReason::ActorBlockSyncCapacity => {
                &mut self.actor_block_sync_capacity_failures
            }
        };
        *counter = counter.saturating_add(1);
    }
}

/// Cumulative diagnostics and current bounded-work gauges.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct WorldStreamStats {
    /// Monotonic lifetime fact, unaffected by diagnostic drains or timing resets.
    pub audio_nondefault_camera_observed: bool,
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
    pub committed_audio_events: usize,
    pub committed_camera_events: usize,
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
    /// Worker-pool share of the mesh queue wait: dispatch to worker start.
    pub max_mesh_dispatch_wait: Duration,
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

    pub(super) fn observe_mesh_queue_wait(
        &mut self,
        queue_wait: Duration,
        dispatch_wait: Duration,
    ) {
        self.max_mesh_queue_wait = self.max_mesh_queue_wait.max(queue_wait);
        self.max_mesh_dispatch_wait = self.max_mesh_dispatch_wait.max(dispatch_wait);
    }
}

impl super::WorldStream {
    /// Any committed camera command conservatively disables the ordinary-listener
    /// audio lane for this entire stream. Only fresh bootstrap re-enables it.
    #[must_use]
    pub const fn audio_default_camera_eligible(&self) -> bool {
        !self.authority.audio_nondefault_camera_observed()
    }
}

pub(super) fn queue_wait(queued_at: Instant, started_at: Instant) -> Duration {
    started_at.saturating_duration_since(queued_at)
}

/// Work performed by one call to [`WorldStream::poll`].
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct WorldStreamPoll {
    /// Ordered commit steps applied, counting each partial sub-chunk batch entry.
    pub commit_steps: usize,
    pub decoded_results: usize,
    pub light_results: usize,
    pub light_jobs_dispatched: usize,
    pub mesh_results: usize,
    pub mesh_jobs_dispatched: usize,
    /// Mesh updates or removals queued, including replacements of pending changes.
    pub mesh_changes_queued: usize,
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

#[derive(Debug, Clone, Copy)]
pub(super) struct PendingMesh {
    pub(super) revision: u64,
    pub(super) since: Instant,
    pub(super) queued_at: Instant,
    pub(super) urgent: bool,
}

#[derive(Debug)]
pub(super) struct MeshCompletion {
    pub(super) output_permit: Option<super::meshing::memory::MeshMemoryPermit>,
    pub(super) _job_permit: Option<super::meshing::admission::MeshJobPermit>,
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
    pub(super) dispatch_wait: Duration,
    pub(super) duration: Duration,
    pub(super) urgent: bool,
}

#[cfg(test)]
mod mesh_change_queue_tests {
    use super::*;

    fn upsert(key: SubChunkKey, generation: u64, urgent: bool) -> WorldMeshChange {
        WorldMeshChange::Upsert {
            output_permit: None,
            key,
            mesh: ChunkMesh::default(),
            biome: PackedBiomeRecord::fallback(),
            tint_identity: ChunkBiomeTintIdentity::default(),
            generation,
            dirty_since: Instant::now(),
            urgent,
            permit: None,
        }
    }

    fn remove(key: SubChunkKey, generation: u64, urgent: bool) -> WorldMeshChange {
        WorldMeshChange::Remove {
            key,
            generation,
            dirty_since: Instant::now(),
            urgent,
            permit: None,
        }
    }

    fn order(queue: &mut MeshChangeQueue) -> Vec<(SubChunkKey, u64)> {
        std::iter::from_fn(|| queue.pop_front())
            .map(|change| (change.key(), change.generation()))
            .collect()
    }

    /// Each key keeps only its newest change, wherever urgency or a retry would place it.
    #[test]
    fn a_key_never_holds_an_older_change_behind_a_newer_one() {
        let [a, b] = [SubChunkKey::new(0, 0, 0, 0), SubChunkKey::new(0, 1, 0, 0)];
        let mut queue = MeshChangeQueue::default();
        assert!(!queue.push(upsert(a, 1, false)));
        assert!(!queue.push(upsert(b, 2, false)));
        assert!(
            queue.push(remove(a, 3, true)),
            "the older upsert is superseded"
        );
        assert!(
            queue.push_front(upsert(b, 1, true)),
            "the stale retry is dropped"
        );
        assert_eq!(queue.len(), 2);
        assert_eq!(order(&mut queue), [(a, 3), (b, 2)]);

        queue.push(remove(a, 4, false));
        queue.push(upsert(a, 5, true));
        assert_eq!(
            queue.retain(|change| matches!(change, WorldMeshChange::Remove { .. })),
            1
        );
        assert!(queue.is_empty());
        assert!(!queue.push(remove(a, 6, false)), "retain released the key");
        assert_eq!(order(&mut queue), [(a, 6)]);
    }
}
