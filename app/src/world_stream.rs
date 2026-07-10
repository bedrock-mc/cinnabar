use std::{
    collections::{BTreeMap, BTreeSet, HashMap, HashSet, VecDeque},
    sync::Arc,
    time::{Duration, Instant},
};

use bevy::prelude::Resource;
use crossbeam_channel::{Receiver, Sender, bounded};
use protocol::{
    BlockUpdateEvent, ChangeDimensionEvent, LevelChunkEvent, LevelChunkMode, MovePlayerEvent,
    Packet, SubChunkBatchEvent, SubChunkResult, WorldBootstrap, WorldEvent,
    request_sub_chunk_column, vanilla_dimension_range,
};
use render::{BlockClassifier, ChunkMesh, Face, FaceConnectivity, Neighbourhood, mesh_sub_chunk};
use thiserror::Error;
use world::{
    BlockUpdate, ChunkKey, ChunkStore, DecodeError, DecodedLevelChunk, MutationError,
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
pub const COMMITTED_CONTROL_CAPACITY: usize = MAX_ADMITTED_WORLD_EVENTS;
pub const OUTBOUND_REQUEST_CAPACITY: usize = 64;
pub const DEFERRED_RETRY_CAPACITY: usize = 64;
pub const MAX_SUB_CHUNK_RETRIES: u8 = 2;
pub const MAX_PENDING_MESH_CHANGES: usize = 256;

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

/// A current packed mesh update, or removal, ready for `ChunkRenderQueue`.
#[derive(Debug)]
pub enum WorldMeshChange {
    Upsert {
        key: SubChunkKey,
        mesh: ChunkMesh,
        generation: u64,
        dirty_since: Instant,
    },
    Remove {
        key: SubChunkKey,
        generation: u64,
        dirty_since: Instant,
    },
}

#[derive(Debug, Clone, Copy, PartialEq)]
pub enum CommittedControlEvent {
    MovePlayer {
        movement: MovePlayerEvent,
        resolved: ResolvedServerPosition,
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

/// Cumulative diagnostics and current bounded-work gauges.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct WorldStreamStats {
    pub decode_errors: u64,
    pub normalization_errors: u64,
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
    pub max_decode_duration: Duration,
    pub max_mesh_duration: Duration,
    pub max_remesh_latency: Duration,
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
    mesh: ChunkMesh,
    duration: Duration,
}

struct MeshSnapshot {
    center: Arc<SubChunk>,
    negative_x: Option<Arc<SubChunk>>,
    positive_x: Option<Arc<SubChunk>>,
    negative_y: Option<Arc<SubChunk>>,
    positive_y: Option<Arc<SubChunk>>,
    negative_z: Option<Arc<SubChunk>>,
    positive_z: Option<Arc<SubChunk>>,
}

impl MeshSnapshot {
    fn mesh(&self, classifier: BlockClassifier) -> ChunkMesh {
        let mut neighbours = Neighbourhood::empty();
        if let Some(neighbour) = self.negative_x.as_deref() {
            neighbours = neighbours.with_negative_x(neighbour);
        }
        if let Some(neighbour) = self.positive_x.as_deref() {
            neighbours = neighbours.with_positive_x(neighbour);
        }
        if let Some(neighbour) = self.negative_y.as_deref() {
            neighbours = neighbours.with_negative_y(neighbour);
        }
        if let Some(neighbour) = self.positive_y.as_deref() {
            neighbours = neighbours.with_positive_y(neighbour);
        }
        if let Some(neighbour) = self.negative_z.as_deref() {
            neighbours = neighbours.with_negative_z(neighbour);
        }
        if let Some(neighbour) = self.positive_z.as_deref() {
            neighbours = neighbours.with_positive_z(neighbour);
        }
        mesh_sub_chunk(&classifier, &neighbours, &self.center)
    }
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
    current_dimension: i32,
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
    pending_mesh: HashMap<SubChunkKey, PendingMesh>,
    in_flight: HashMap<SubChunkKey, u64>,
    resident: BTreeSet<SubChunkKey>,
    known_air: BTreeSet<SubChunkKey>,
    loaded_columns: BTreeSet<ChunkKey>,
    requested_sub_chunks: HashMap<ChunkKey, BTreeSet<i32>>,
    retry_attempts: HashMap<SubChunkKey, u8>,
    deferred_retries: VecDeque<SubChunkKey>,
    deferred_retry_set: HashSet<SubChunkKey>,
    connectivity: HashMap<SubChunkKey, FaceConnectivity>,
    connectivity_generation: u64,
    requests: VecDeque<OutboundRequestSlot>,
    mesh_changes: VecDeque<WorldMeshChange>,
    committed_controls: VecDeque<CommittedControlEvent>,
    publisher_center: Option<[i32; 3]>,
    publisher_radius_chunks: Option<i32>,
    chunk_radius: Option<i32>,
    resolved_server_position: ResolvedServerPosition,
    stats: WorldStreamStats,
}

impl WorldStream {
    #[cfg(test)]
    #[must_use]
    pub fn new(bootstrap: WorldBootstrap) -> Self {
        Self::with_first_sequence(bootstrap, 1)
    }

    #[must_use]
    pub fn new_with_recovery(
        bootstrap: WorldBootstrap,
        current_position: [f32; 3],
        existing_anchor: Option<[i32; 2]>,
    ) -> Self {
        Self::with_first_sequence_and_recovery(bootstrap, 1, current_position, existing_anchor)
    }

    #[cfg(test)]
    #[must_use]
    pub fn with_first_sequence(bootstrap: WorldBootstrap, first_sequence: u64) -> Self {
        Self::with_first_sequence_and_recovery(
            bootstrap,
            first_sequence,
            [0.0, crate::server_position::SAFE_SERVER_HEIGHT, 0.0],
            None,
        )
    }

    fn with_first_sequence_and_recovery(
        bootstrap: WorldBootstrap,
        first_sequence: u64,
        current_position: [f32; 3],
        existing_anchor: Option<[i32; 2]>,
    ) -> Self {
        let (decode_tx, decode_rx) = bounded(WORK_RESULT_CAPACITY);
        let (mesh_tx, mesh_rx) = bounded(WORK_RESULT_CAPACITY);
        let resolved_server_position =
            resolve_server_position(bootstrap.player_position, current_position, existing_anchor);
        Self {
            store: ChunkStore::new(),
            classifier: BlockClassifier::new(bootstrap.air_network_id),
            current_dimension: bootstrap.dimension,
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
            pending_mesh: HashMap::new(),
            in_flight: HashMap::new(),
            resident: BTreeSet::new(),
            known_air: BTreeSet::new(),
            loaded_columns: BTreeSet::new(),
            requested_sub_chunks: HashMap::new(),
            retry_attempts: HashMap::new(),
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
            WorldEvent::LevelChunk(LevelChunkEvent {
                mode: LevelChunkMode::Inline { .. },
                ..
            }) | WorldEvent::SubChunks(_)
                | WorldEvent::BlockUpdates(_)
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

    #[must_use]
    pub fn pending_request_count(&self) -> usize {
        self.requests
            .iter()
            .filter(|slot| matches!(slot, OutboundRequestSlot::Ready(_)))
            .count()
    }

    #[cfg(test)]
    pub fn take_mesh_changes(&mut self) -> Vec<WorldMeshChange> {
        self.mesh_changes.drain(..).collect()
    }

    pub fn pop_mesh_change(&mut self) -> Option<WorldMeshChange> {
        self.mesh_changes.pop_front()
    }

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
            pending_retry_requests: self.deferred_retries.len(),
            ..self.stats
        }
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
            self.stats.normalization_errors = self.stats.normalization_errors.saturating_add(1);
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
                Ok(_) | Err(_) => {
                    self.stats.normalization_errors =
                        self.stats.normalization_errors.saturating_add(1);
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
                    } => {
                        let payload = std::mem::take(&mut event.payload);
                        let decoded = DecodedLevelChunk::decode(base_sub_chunk_y, count, &payload);
                        DecodeCompletion {
                            sequence,
                            event: PreparedWorldEvent::InlineLevelChunk {
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
                    self.stats.normalization_errors =
                        self.stats.normalization_errors.saturating_add(1);
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
                        self.requested_sub_chunks.remove(&key);
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
                        for dirty in applied.dirty {
                            self.mark_dirty_exact(dirty, now);
                        }
                        for removed in old_keys.difference(&new_keys) {
                            self.mark_changed(*removed, now);
                        }
                    }
                    Err(_) => self.stats.decode_errors = self.stats.decode_errors.saturating_add(1),
                }
            }
            PreparedWorldEvent::SubChunks {
                dimension,
                entries,
                duration,
            } => {
                self.stats.max_decode_duration = self.stats.max_decode_duration.max(duration);
                for entry in entries {
                    let key = SubChunkKey::new(
                        dimension,
                        entry.position[0],
                        entry.position[1],
                        entry.position[2],
                    );
                    if !self.is_expected_sub_chunk(key) || !self.column_is_active(key.chunk()) {
                        self.stats.normalization_errors =
                            self.stats.normalization_errors.saturating_add(1);
                        continue;
                    }
                    let completed = match entry.result {
                        PreparedSubChunkResult::Decoded(Ok(decoded)) => {
                            let decoded_air = decoded.has_no_storages();
                            match self.store.commit_sub_chunk(key, decoded) {
                                Ok(Some(changed)) => {
                                    if decoded_air {
                                        self.record_known_air(changed);
                                    } else {
                                        self.sync_resident(changed);
                                    }
                                    self.mark_changed(changed, Instant::now());
                                }
                                Ok(None) => {
                                    if decoded_air {
                                        self.record_known_air(key);
                                    }
                                }
                                Err(_) => {
                                    self.stats.decode_errors =
                                        self.stats.decode_errors.saturating_add(1);
                                }
                            }
                            true
                        }
                        PreparedSubChunkResult::Decoded(Err(_)) => {
                            self.stats.decode_errors = self.stats.decode_errors.saturating_add(1);
                            self.retry_or_complete_sub_chunk(key)
                        }
                        PreparedSubChunkResult::AllAir => {
                            let changed = self.store.apply_all_air(key);
                            self.record_known_air(key);
                            if let Some(changed) = changed {
                                self.mark_changed(changed, Instant::now());
                            }
                            true
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
                                    true
                                }
                                protocol::SubChunkUnavailable::InvalidDimension => {
                                    self.stats.normalization_errors =
                                        self.stats.normalization_errors.saturating_add(1);
                                    true
                                }
                                protocol::SubChunkUnavailable::ChunkNotFound
                                | protocol::SubChunkUnavailable::PlayerNotFound => {
                                    self.retry_or_complete_sub_chunk(key)
                                }
                                protocol::SubChunkUnavailable::Undefined
                                | protocol::SubChunkUnavailable::Unknown(_) => true,
                            }
                        }
                    };
                    if completed {
                        self.complete_requested_sub_chunk(key);
                    }
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
                        self.stats.normalization_errors =
                            self.stats.normalization_errors.saturating_add(1);
                    }
                }
            }
            PreparedWorldEvent::Immediate(event) => self.apply_immediate(event, sequence),
            PreparedWorldEvent::NormalizationFailure => {
                self.stats.normalization_errors = self.stats.normalization_errors.saturating_add(1);
            }
        }
    }

    fn apply_immediate(&mut self, event: WorldEvent, sequence: Option<u64>) {
        match event {
            WorldEvent::LevelChunk(event) => self.apply_request_level_chunk(event, sequence),
            WorldEvent::BlockUpdates(_) => {
                unreachable!("block-update batches are prepared on workers")
            }
            WorldEvent::ChunkRadiusUpdated(radius) => {
                if radius < 0 {
                    self.stats.normalization_errors =
                        self.stats.normalization_errors.saturating_add(1);
                    return;
                }
                self.chunk_radius = Some(radius.min(PHASE0_MAX_VIEW_RADIUS_CHUNKS));
                self.evict_outside_active_radius();
            }
            WorldEvent::PublisherUpdate(update) => {
                self.publisher_center = Some(update.center);
                let chunks = update.radius_blocks.saturating_add(15) / 16;
                self.publisher_radius_chunks = Some(
                    i32::try_from(chunks)
                        .unwrap_or(i32::MAX)
                        .min(PHASE0_MAX_VIEW_RADIUS_CHUNKS),
                );
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
                self.push_committed_control(CommittedControlEvent::ChangeDimension {
                    change,
                    resolved,
                });
            }
            WorldEvent::MovePlayer(movement) => {
                let resolved = resolve_server_position(
                    movement.position,
                    self.resolved_server_position.position,
                    self.resolved_server_position.surface_anchor,
                );
                self.resolved_server_position = resolved;
                self.push_committed_control(CommittedControlEvent::MovePlayer {
                    movement,
                    resolved,
                });
            }
            WorldEvent::SubChunks(_) => unreachable!("sub-chunk batches are prepared on workers"),
        }
    }

    fn apply_request_level_chunk(&mut self, event: LevelChunkEvent, sequence: Option<u64>) {
        let key = ChunkKey::new(event.dimension, event.x, event.z);
        if !self.column_is_active(key) {
            self.stats.normalization_errors = self.stats.normalization_errors.saturating_add(1);
            return;
        }
        match event.mode {
            LevelChunkMode::LimitedRequests { highest } => {
                let Some(range) = vanilla_dimension_range(event.dimension) else {
                    self.stats.normalization_errors =
                        self.stats.normalization_errors.saturating_add(1);
                    return;
                };
                self.evict_column(key);
                let count = usize::from(highest).min(range.sub_chunk_count);
                self.enqueue_request(key, range.base_sub_chunk_y, count, sequence);
            }
            LevelChunkMode::LimitlessRequests => {
                let Some(range) = vanilla_dimension_range(event.dimension) else {
                    self.stats.normalization_errors =
                        self.stats.normalization_errors.saturating_add(1);
                    return;
                };
                self.evict_column(key);
                self.enqueue_request(key, range.base_sub_chunk_y, range.sub_chunk_count, sequence);
            }
            LevelChunkMode::Inline { .. } => {
                unreachable!("inline LevelChunk packets are prepared on workers")
            }
        }
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
                    self.stats.normalization_errors =
                        self.stats.normalization_errors.saturating_add(1);
                    return;
                }
                let expected = (0..count)
                    .map(|offset| base_sub_chunk_y.saturating_add(offset as i32))
                    .collect::<BTreeSet<_>>();
                if expected.is_empty() {
                    self.loaded_columns.insert(key);
                } else {
                    self.requested_sub_chunks.insert(key, expected);
                }
            }
            Err(_) => {
                self.stats.normalization_errors = self.stats.normalization_errors.saturating_add(1)
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
        self.known_air.insert(key);
        self.set_connectivity(key, Some(FaceConnectivity::all()));
    }

    fn mark_changed(&mut self, key: SubChunkKey, now: Instant) {
        for dependent in key.mesh_dependents() {
            self.mark_dirty_exact(dependent, now);
        }
    }

    fn mark_dirty_exact(&mut self, key: SubChunkKey, now: Instant) {
        let revision = self.revisions.mark_dirty(key, now);
        let since = self.revisions.dirty(key).map_or(now, |dirty| dirty.since);
        self.pending_mesh
            .insert(key, PendingMesh { revision, since });
    }

    fn evict_column(&mut self, key: ChunkKey) {
        self.loaded_columns.remove(&key);
        self.requested_sub_chunks.remove(&key);
        self.requests.retain(|slot| match slot {
            OutboundRequestSlot::Reserved(_) => true,
            OutboundRequestSlot::Ready(request) => request.chunk != key,
        });
        self.retry_attempts
            .retain(|sub_chunk, _| sub_chunk.chunk() != key);
        self.deferred_retries
            .retain(|sub_chunk| sub_chunk.chunk() != key);
        self.deferred_retry_set
            .retain(|sub_chunk| sub_chunk.chunk() != key);
        let mut changed = self
            .resident
            .iter()
            .copied()
            .filter(|resident| resident.chunk() == key)
            .collect::<BTreeSet<_>>();
        changed.extend(self.store.evict_chunk(key));
        self.resident.retain(|resident| resident.chunk() != key);
        self.known_air.retain(|resident| resident.chunk() != key);
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
            .is_some_and(|expected| expected.contains(&key.y))
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
                } else {
                    self.set_connectivity(key, None);
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
            rayon::spawn(move || {
                let started = Instant::now();
                let source = Arc::clone(&snapshot.center);
                let mesh = snapshot.mesh(classifier);
                let _ = tx.send(MeshCompletion {
                    key,
                    revision,
                    source,
                    mesh,
                    duration: started.elapsed(),
                });
            });
            dispatched += 1;
        }
        dispatched
    }

    fn mesh_snapshot(&self, key: SubChunkKey, center: Arc<SubChunk>) -> MeshSnapshot {
        MeshSnapshot {
            center,
            negative_x: adjacent_key(key, Face::NegativeX)
                .and_then(|key| self.store.sub_chunk(key)),
            positive_x: adjacent_key(key, Face::PositiveX)
                .and_then(|key| self.store.sub_chunk(key)),
            negative_y: adjacent_key(key, Face::NegativeY)
                .and_then(|key| self.store.sub_chunk(key)),
            positive_y: adjacent_key(key, Face::PositiveY)
                .and_then(|key| self.store.sub_chunk(key)),
            negative_z: adjacent_key(key, Face::NegativeZ)
                .and_then(|key| self.store.sub_chunk(key)),
            positive_z: adjacent_key(key, Face::PositiveZ)
                .and_then(|key| self.store.sub_chunk(key)),
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
        if !self
            .revisions
            .is_current(completion.key, completion.revision)
            || !source_is_current
        {
            self.stats.stale_mesh_jobs = self.stats.stale_mesh_jobs.saturating_add(1);
            return;
        }
        self.stats.max_mesh_duration = self.stats.max_mesh_duration.max(completion.duration);
        let dirty = self
            .revisions
            .dirty(completion.key)
            .expect("current mesh completion has a dirty revision");
        self.set_connectivity(completion.key, Some(completion.mesh.connectivity()));
        self.mesh_changes.push_back(WorldMeshChange::Upsert {
            key: completion.key,
            mesh: completion.mesh,
            generation: completion.revision,
            dirty_since: dirty.since,
        });
    }

    fn complete_requested_sub_chunk(&mut self, key: SubChunkKey) {
        self.cancel_sub_chunk_retry(key);
        let chunk = key.chunk();
        let completed = self
            .requested_sub_chunks
            .get_mut(&chunk)
            .is_some_and(|expected| {
                expected.remove(&key.y);
                expected.is_empty()
            });
        if completed {
            self.requested_sub_chunks.remove(&chunk);
            self.loaded_columns.insert(chunk);
        }
    }

    fn retry_or_complete_sub_chunk(&mut self, key: SubChunkKey) -> bool {
        if self.retry_is_queued(key) {
            return false;
        }
        let attempts = self.retry_attempts.entry(key).or_default();
        if *attempts >= MAX_SUB_CHUNK_RETRIES {
            return true;
        }
        *attempts += 1;
        if self.requests.len() < OUTBOUND_REQUEST_CAPACITY {
            return !self.enqueue_exact_retry(key);
        }
        if self.deferred_retries.len() < DEFERRED_RETRY_CAPACITY {
            self.deferred_retries.push_back(key);
            self.deferred_retry_set.insert(key);
            return false;
        }
        self.stats.normalization_errors = self.stats.normalization_errors.saturating_add(1);
        true
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
            self.stats.normalization_errors = self.stats.normalization_errors.saturating_add(1);
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
        self.retry_attempts.remove(&key);
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

fn adjacent_key(key: SubChunkKey, face: Face) -> Option<SubChunkKey> {
    match face {
        Face::NegativeX => key
            .x
            .checked_sub(1)
            .map(|x| SubChunkKey::new(key.dimension, x, key.y, key.z)),
        Face::PositiveX => key
            .x
            .checked_add(1)
            .map(|x| SubChunkKey::new(key.dimension, x, key.y, key.z)),
        Face::NegativeY => key
            .y
            .checked_sub(1)
            .map(|y| SubChunkKey::new(key.dimension, key.x, y, key.z)),
        Face::PositiveY => key
            .y
            .checked_add(1)
            .map(|y| SubChunkKey::new(key.dimension, key.x, y, key.z)),
        Face::NegativeZ => key
            .z
            .checked_sub(1)
            .map(|z| SubChunkKey::new(key.dimension, key.x, key.y, z)),
        Face::PositiveZ => key
            .z
            .checked_add(1)
            .map(|z| SubChunkKey::new(key.dimension, key.x, key.y, z)),
    }
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
    use std::{collections::BTreeSet, sync::Arc, time::Instant};

    use protocol::{
        BlockUpdateEvent, ChangeDimensionEvent, LevelChunkEvent, LevelChunkMode, MovePlayerEvent,
        PublisherUpdateEvent, SubChunkBatchEvent, SubChunkEntryEvent, SubChunkResult,
        SubChunkUnavailable, WorldBootstrap, WorldEvent,
    };
    use render::{BlockClassifier, Neighbourhood, mesh_sub_chunk};
    use world::{BlockUpdate, ChunkKey, ChunkStore, DecodedLevelChunk, SubChunkKey};

    use super::{MeshCompletion, RevisionTracker, SequenceBuffer, WorldStream, split_block_update};

    fn inline_air_event(x: i32) -> WorldEvent {
        WorldEvent::LevelChunk(LevelChunkEvent {
            dimension: 0,
            x,
            z: 0,
            mode: LevelChunkMode::Inline { count: 1 },
            payload: vec![9, 0, (-4_i8) as u8],
        })
    }

    fn complete_pending_decode_jobs(stream: &mut WorldStream) {
        while let Some(job) = stream.pending_decode.pop_front() {
            let (sequence, event) = match job {
                super::DecodeJob::InlineLevelChunk {
                    sequence,
                    mut event,
                    base_sub_chunk_y,
                    count,
                } => {
                    let payload = std::mem::take(&mut event.payload);
                    (
                        sequence,
                        super::PreparedWorldEvent::InlineLevelChunk {
                            event,
                            decoded: DecodedLevelChunk::decode(base_sub_chunk_y, count, &payload),
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

    #[test]
    fn bootstrap_non_finite_horizontal_position_uses_the_shared_finite_scope_anchor() {
        let bootstrap = WorldBootstrap {
            dimension: 0,
            local_player_runtime_id: 1,
            player_position: [f32::NAN, 80.0, f32::INFINITY],
            world_spawn_position: [0; 3],
            air_network_id: 12_530,
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
    fn newer_subchunk_is_validated_after_fifo_blocked_dimension_change_commits() {
        let mut stream = WorldStream::new(WorldBootstrap {
            dimension: 0,
            local_player_runtime_id: 1,
            player_position: [0.0; 3],
            world_spawn_position: [0; 3],
            air_network_id: 12_530,
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
                    payload: Vec::new(),
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

        assert_eq!(stream.pending_decode.len(), 2);
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
                        payload: Vec::new(),
                    }),
                )
                .unwrap();
        }
        stream.submit(63, inline_air_event(0)).unwrap();
        for (sequence, x) in [(64, 10), (65, 11)] {
            stream
                .submit(
                    sequence,
                    WorldEvent::LevelChunk(LevelChunkEvent {
                        dimension: 0,
                        x,
                        z: 1,
                        mode: LevelChunkMode::LimitedRequests { highest: 1 },
                        payload: Vec::new(),
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
                    payload: Vec::new(),
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
                    payload: Vec::new(),
                }),
            )
            .unwrap();
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
    fn old_dimension_and_out_of_radius_chunks_are_rejected_and_radii_are_clamped() {
        let mut stream = WorldStream::new(WorldBootstrap {
            dimension: 0,
            local_player_runtime_id: 1,
            player_position: [0.0; 3],
            world_spawn_position: [0; 3],
            air_network_id: 12_530,
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
                    payload: Vec::new(),
                }),
            )
            .unwrap();
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
        });
        stream
            .submit(
                1,
                WorldEvent::LevelChunk(LevelChunkEvent {
                    dimension: 0,
                    x: 0,
                    z: 0,
                    mode: LevelChunkMode::LimitedRequests { highest: 1 },
                    payload: Vec::new(),
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

        assert_eq!(stream.stats().queued_decode_jobs, 1);
        complete_pending_decode_jobs(&mut stream);
        assert_eq!(stream.stats().queued_decode_jobs, 0);
        assert!(!stream.resident.contains(&SubChunkKey::new(0, 0, -3, 0)));
        assert_eq!(
            stream.requested_sub_chunks[&ChunkKey::new(0, 0, 0)],
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
                    movement,
                    resolved: crate::server_position::ResolvedServerPosition {
                        position: movement.position,
                        surface_anchor: None,
                    },
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
    fn remesh_latency_closes_only_when_the_exact_generation_is_applied() {
        let mut stream = WorldStream::new(WorldBootstrap {
            dimension: 0,
            local_player_runtime_id: 1,
            player_position: [0.0; 3],
            world_spawn_position: [0; 3],
            air_network_id: 12_530,
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
        stream.in_flight.insert(key, generation);
        let mesh = mesh_sub_chunk(&stream.classifier, &Neighbourhood::empty(), source.as_ref());
        stream.accept_mesh_completion(MeshCompletion {
            key,
            revision: generation,
            source,
            mesh,
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
    fn max_block_update_batch_prepares_off_thread_and_commits_atomically_in_fifo() {
        let mut stream = WorldStream::new(WorldBootstrap {
            dimension: 0,
            local_player_runtime_id: 1,
            player_position: [0.0; 3],
            world_spawn_position: [0; 3],
            air_network_id: 12_530,
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
                movement,
                resolved: crate::server_position::ResolvedServerPosition {
                    position: movement.position,
                    surface_anchor: None,
                },
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
        });

        stream
            .submit(
                1,
                WorldEvent::LevelChunk(LevelChunkEvent {
                    dimension: 0,
                    x: -2,
                    z: 5,
                    mode: LevelChunkMode::LimitedRequests { highest: u16::MAX },
                    payload: Vec::new(),
                }),
            )
            .unwrap();
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
                    payload: Vec::new(),
                }),
            )
            .unwrap();

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
                        payload: Vec::new(),
                    }),
                )
                .unwrap();
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
                        payload: Vec::new(),
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
                    payload: Vec::new(),
                }),
            )
            .unwrap();
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
        });
        let chunk = ChunkKey::new(0, 0, 0);
        stream
            .requested_sub_chunks
            .insert(chunk, BTreeSet::from([-4, -3]));
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
                    payload: Vec::new(),
                }),
            )
            .unwrap();

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
        let old_revision = stream.revisions.mark_dirty(key, Instant::now());
        let current_revision = stream.revisions.mark_dirty(key, Instant::now());
        stream.in_flight.insert(key, old_revision);
        let classifier = BlockClassifier::new(12_530);
        let mesh = mesh_sub_chunk(&classifier, &Neighbourhood::empty(), &source);

        stream.accept_mesh_completion(MeshCompletion {
            key,
            revision: old_revision,
            source: Arc::clone(&source),
            mesh,
            duration: std::time::Duration::ZERO,
        });

        assert!(stream.revisions.is_current(key, current_revision));
        assert_eq!(stream.stats().stale_mesh_jobs, 1);
        assert!(stream.take_mesh_changes().is_empty());
    }

    #[test]
    fn mesh_dispatch_never_exceeds_the_bounded_worker_window() {
        let mut stream = WorldStream::new(WorldBootstrap {
            dimension: 0,
            local_player_runtime_id: 1,
            player_position: [0.0; 3],
            world_spawn_position: [0; 3],
            air_network_id: 12_530,
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
    fn inline_zero_storage_is_a_graph_node_until_column_eviction() {
        let mut stream = WorldStream::new(WorldBootstrap {
            dimension: 0,
            local_player_runtime_id: 1,
            player_position: [0.0; 3],
            world_spawn_position: [0; 3],
            air_network_id: 12_530,
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
        });
        let key = SubChunkKey::new(1, -8, 3, 12);
        stream
            .requested_sub_chunks
            .insert(key.chunk(), BTreeSet::from([key.y]));
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
