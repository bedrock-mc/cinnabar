mod args;
mod asset_startup;
mod camera;
mod culling;
mod metrics;
mod model_witness;
mod network;
mod server_position;
mod transparent_witness;
mod world_stream;

use std::{
    collections::{BTreeSet, HashSet, VecDeque},
    io::Write,
    path::{Path, PathBuf},
    sync::Arc,
    time::{Duration, Instant},
};

use anyhow::{Context, Result, bail};
use asset_startup::{LoadedAssetKind, load_runtime_assets, select_asset_path_from_environment};
use assets::{DIAGNOSTIC_MATERIAL, RuntimeAssets};
use bevy::{
    app::AppExit,
    prelude::*,
    window::{CursorOptions, PresentMode, PrimaryWindow, WindowPlugin},
    winit::{UpdateMode, WinitSettings},
};
use camera::{FlyCamera, FlyCameraPlugin};
use metrics::{
    DiagnosticQuadTracker, ExactFullViewProof, MetricsCollector, PipelineMetricsSnapshot,
    TeleportProof, TransparentSortMetricsSnapshot, deterministic_manifest_hash,
};
use model_witness::{ModelWitnessFileSource, poll_model_witness_request};
use network::{NetworkConfig, NetworkControlEvent, NetworkHandle, spawn_network};
use render::{
    ChunkBiomeTints, ChunkRenderInstance, ChunkRenderQueue, ChunkTextureAssets,
    ChunkUploadAcknowledgements, ChunkUploadPriority, ChunkUploadToken, DebugWorldPlugin,
    ModelWitnessEvidence, ModelWitnessManifestRecord, ModelWitnessRequest, PresentedFrameAck,
    PresentedFrameGate, RenderViewCohort, TargetRenderExpectation, TransparentSortMetrics,
    TransparentWitnessEvidence,
};
use server_position::SAFE_SERVER_HEIGHT;
use sha2::{Digest, Sha256};
use transparent_witness::{TransparentWitnessFileSource, poll_transparent_witness_request};
use world::SubChunkKey;
use world_stream::{
    CommittedControlEvent, ViewCohort, ViewCohortStatus, WorldMeshChange, WorldStream,
};

const MESH_JOB_BUDGET_PER_FRAME: usize = 128;
const GPU_UPLOAD_BUDGET_PER_FRAME: usize = 128;
const NETWORK_INGRESS_BUDGET_PER_FRAME: usize = 8;
const OUTBOUND_SEND_BUDGET_PER_FRAME: usize = 16;
const TITLE_REFRESH_INTERVAL: Duration = Duration::from_millis(250);
const WORLD_READY_QUIET_INTERVAL: Duration = Duration::from_secs(2);
const TRANSPARENT_PRESENTATION_EXIT_GRACE: Duration = Duration::from_secs(2);
const TELEPORT_COHORT_PROGRESS_INTERVAL: Duration = Duration::from_secs(1);
const PHASE0_REQUESTED_RADIUS_CHUNKS: i32 = 16;
const MUTATION_X_OFFSET_BLOCKS: i32 = 4;
const LEAF_FOREST_FAR_OFFSET_CHUNKS: i32 = 65;
const LEAF_FOREST_FAR_OFFSET_BLOCKS: i32 = LEAF_FOREST_FAR_OFFSET_CHUNKS * 16;
const LEAF_FOREST_MUTATION_Z_OFFSET_BLOCKS: i32 = 12;
const FULL_VIEW_TELEPORT_MIN_CHUNK_DELTA: u64 = (PHASE0_REQUESTED_RADIUS_CHUNKS as u64) * 2 + 1;

#[derive(Resource)]
struct ClientWorld {
    stream: Option<WorldStream>,
    runtime_assets: Arc<RuntimeAssets>,
    pending_surface_spawn: Option<[i32; 2]>,
    fatal_error: Option<String>,
    network_decode_errors: u64,
    reported_decode_errors: u64,
}

impl Default for ClientWorld {
    fn default() -> Self {
        Self::new(Arc::new(RuntimeAssets::diagnostic()))
    }
}

impl ClientWorld {
    fn new(runtime_assets: Arc<RuntimeAssets>) -> Self {
        Self {
            stream: None,
            runtime_assets,
            pending_surface_spawn: None,
            fatal_error: None,
            network_decode_errors: 0,
            reported_decode_errors: 0,
        }
    }
}

fn startup_biome_tints(runtime_assets: &RuntimeAssets) -> ChunkBiomeTints {
    let resolved = runtime_assets
        .biome_assets()
        .resolve_live(&[])
        .expect("validated startup biome assets resolve without live definitions");
    ChunkBiomeTints::from_resolved(&resolved, 0)
}

fn synchronize_biome_tints(stream: &WorldStream, active: &mut ChunkBiomeTints) -> bool {
    let identity = stream.biome_tint_identity();
    if active.table_identity() == identity {
        return false;
    }
    let resolved = stream.resolved_biome_tints_snapshot();
    *active = ChunkBiomeTints::from_resolved_with_identity(&resolved, identity);
    true
}

#[derive(Resource, Default)]
struct CaveVisibilityCache {
    camera: Option<SubChunkKey>,
    graph_generation: Option<u64>,
    visible: BTreeSet<SubChunkKey>,
    rendered: HashSet<SubChunkKey>,
    visible_rendered: usize,
    initialized: bool,
}

impl CaveVisibilityCache {
    fn is_visible(&self, key: SubChunkKey) -> bool {
        !self.initialized || self.visible.contains(&key)
    }
}

#[derive(Resource)]
struct AppMetrics(MetricsCollector);

#[derive(Resource, Default)]
struct DiagnosticQuads(DiagnosticQuadTracker);

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
struct WorldReadyWork {
    network_events: usize,
    network_commands: usize,
    admitted_world_events: usize,
    queued_decode_jobs: usize,
    in_flight_decode_jobs: usize,
    completed_decode_results: usize,
    pending_mesh_jobs: usize,
    in_flight_mesh_jobs: usize,
    pending_mesh_changes: usize,
    outbound_requests: usize,
    outstanding_sub_chunks: usize,
    pending_retry_requests: usize,
    render_queue_items: usize,
    pending_gpu_acknowledgements: usize,
    unacknowledged_meshes: usize,
}

impl WorldReadyWork {
    fn is_empty(self) -> bool {
        self == Self::default()
    }
}

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
struct SubChunkTimeoutProgress {
    awaiting_responses: usize,
    timeouts: u64,
    retries_scheduled: u64,
    retry_exhaustions: u64,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
struct WorldReadySnapshot {
    mutation_coordinate: Option<[i32; 3]>,
    received_radius_chunks: Option<i32>,
    publisher_radius_chunks: Option<i32>,
    rendered_sub_chunks: usize,
    resident_sub_chunks: usize,
    visible_sub_chunks: usize,
    mutation_target_rendered: bool,
    mutation_target_visible: bool,
    mutation_target_clean: bool,
    work: WorldReadyWork,
}

#[derive(Debug, Default)]
struct WorldReadySettler {
    candidate: Option<(WorldReadySnapshot, Instant)>,
}

impl WorldReadySettler {
    fn observe(&mut self, snapshot: WorldReadySnapshot, now: Instant) -> Option<[String; 2]> {
        let markers = world_ready_markers(snapshot);
        if markers.is_none() {
            self.candidate = None;
            return None;
        }
        match self.candidate {
            Some((stable, since)) if stable == snapshot => (now.saturating_duration_since(since)
                >= WORLD_READY_QUIET_INTERVAL)
                .then_some(markers.expect("settled snapshots have markers")),
            _ => {
                self.candidate = Some((snapshot, now));
                None
            }
        }
    }
}

#[derive(Debug, Default)]
struct GalleryAnchorEmitter {
    emitted: bool,
}

impl GalleryAnchorEmitter {
    fn observe(&mut self, enabled: bool, snapshot: WorldReadySnapshot) -> Option<String> {
        if self.emitted
            || !enabled
            || !snapshot.mutation_target_rendered
            || !snapshot.mutation_target_clean
        {
            return None;
        }
        let coordinate = snapshot.mutation_coordinate?;
        self.emitted = true;
        Some(format!(
            "RUST_MCBE_GALLERY_ANCHOR_READY coordinate={},{},{} rendered=true visible={} clean=true",
            coordinate[0], coordinate[1], coordinate[2], snapshot.mutation_target_visible
        ))
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
struct TeleportReadySnapshot {
    received_radius_chunks: Option<i32>,
    publisher_radius_chunks: Option<i32>,
    rendered_sub_chunks: usize,
    resident_sub_chunks: usize,
    visible_sub_chunks: usize,
    loaded_columns: usize,
    cohort: Option<ViewCohortStatus>,
    last_chunk_commit_at: Option<Instant>,
    last_mesh_dispatch_at: Option<Instant>,
    last_mesh_completion_at: Option<Instant>,
    last_mesh_ack_at: Option<Instant>,
    work: WorldReadyWork,
}

impl TeleportReadySnapshot {
    fn is_binding_ready(self) -> bool {
        self.received_radius_chunks == Some(PHASE0_REQUESTED_RADIUS_CHUNKS)
            && self.publisher_radius_chunks == Some(PHASE0_REQUESTED_RADIUS_CHUNKS)
            && self.cohort.is_some_and(ViewCohortStatus::is_exact)
            && self.work.is_empty()
    }

    fn is_ready(self) -> bool {
        self.is_binding_ready()
            && self.rendered_sub_chunks != 0
            && self.resident_sub_chunks != 0
            && self.visible_sub_chunks != 0
    }
}

#[derive(Debug, Clone)]
struct TeleportPresentedCandidate {
    snapshot: TeleportReadySnapshot,
    status: ViewCohortStatus,
    expectation: TargetRenderExpectation,
    first_frame: Option<PresentedFrameAck>,
}

#[derive(Debug, Clone)]
struct PendingFullViewTeleport {
    started: Instant,
    started_frame_count: u64,
    move_sequence: u64,
    target: ViewCohort,
    source: ViewCohort,
    target_mutation_coordinate: [i32; 3],
    publisher_seen: bool,
    publisher_latency: Option<Duration>,
    first_level_chunk_latency: Option<Duration>,
    last_level_chunk_latency: Option<Duration>,
    level_chunk_events: u64,
    first_sub_chunk_latency: Option<Duration>,
    last_sub_chunk_latency: Option<Duration>,
    sub_chunk_events: u64,
    peak_network_events: usize,
    presented_candidate: Option<TeleportPresentedCandidate>,
    last_progress_at: Option<Instant>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
struct FullViewTeleportCompletion {
    settle_latency: Duration,
    render_ready_latency: Duration,
    first_present_return_latency: Duration,
    first_gpu_completion_latency: Duration,
    stable_present_return_latency: Duration,
    stable_gpu_completion_latency: Duration,
    view_generation: u64,
    target_mutation_coordinate: [i32; 3],
    publisher_latency: Option<Duration>,
    first_level_chunk_latency: Option<Duration>,
    last_level_chunk_latency: Option<Duration>,
    level_chunk_events: u64,
    first_sub_chunk_latency: Option<Duration>,
    last_sub_chunk_latency: Option<Duration>,
    sub_chunk_events: u64,
    last_chunk_commit_latency: Option<Duration>,
    last_mesh_dispatch_latency: Option<Duration>,
    last_mesh_completion_latency: Option<Duration>,
    last_mesh_ack_latency: Option<Duration>,
    peak_network_events: usize,
    expectation: TargetRenderExpectation,
    first_frame: PresentedFrameAck,
    stable_frame: PresentedFrameAck,
    frame_count: u64,
}

#[derive(Debug)]
struct FullViewTeleportTracker {
    enabled: bool,
    origin_chunk: Option<[i32; 2]>,
    source_mutation_coordinate: Option<[i32; 3]>,
    local_player_runtime_id: Option<u64>,
    latest_publisher_ingress: Option<(u64, ViewCohort, Instant)>,
    pending_move_ingress: Option<(u64, Instant, u64)>,
    pending: Option<PendingFullViewTeleport>,
    completed: Option<Duration>,
    completed_target: Option<ViewCohort>,
    completed_target_mutation: Option<[i32; 3]>,
    next_view_generation: u64,
    current_frame_count: u64,
    #[cfg(test)]
    next_test_sequence: u64,
}

impl FullViewTeleportTracker {
    const fn new(enabled: bool) -> Self {
        Self {
            enabled,
            origin_chunk: None,
            source_mutation_coordinate: None,
            local_player_runtime_id: None,
            latest_publisher_ingress: None,
            pending_move_ingress: None,
            pending: None,
            completed: None,
            completed_target: None,
            completed_target_mutation: None,
            next_view_generation: 0,
            current_frame_count: 0,
            #[cfg(test)]
            next_test_sequence: 0,
        }
    }

    fn begin_world_ready(&mut self, position: [f32; 3], local_player_runtime_id: u64) {
        if self.enabled {
            self.origin_chunk = horizontal_chunk(position);
            self.local_player_runtime_id = Some(local_player_runtime_id);
        }
    }

    fn set_source_mutation_coordinate(&mut self, coordinate: [i32; 3]) {
        self.source_mutation_coordinate = Some(coordinate);
    }

    fn observe_ingress(
        &mut self,
        event: &protocol::WorldEvent,
        sequence: u64,
        observed_at: Instant,
        current_dimension: i32,
        frame_count: u64,
    ) -> bool {
        if !self.enabled || self.completed.is_some() {
            return false;
        }
        match event {
            protocol::WorldEvent::PublisherUpdate(update) => {
                let publisher = ViewCohort::from_publisher(
                    current_dimension,
                    update.center,
                    update.radius_blocks,
                );
                if self
                    .latest_publisher_ingress
                    .is_none_or(|(latest, _, _)| sequence >= latest)
                {
                    self.latest_publisher_ingress = Some((sequence, publisher, observed_at));
                }
                if let Some(pending) = &mut self.pending
                    && sequence > pending.move_sequence
                    && publisher == pending.target
                    && !pending.publisher_seen
                {
                    pending.publisher_seen = true;
                    pending.publisher_latency = observed_at.checked_duration_since(pending.started);
                }
                false
            }
            protocol::WorldEvent::MovePlayer(movement) if self.pending.is_none() => {
                if self.local_player_runtime_id != Some(movement.runtime_id) {
                    return false;
                }
                let (Some(origin), Some(target)) =
                    (self.origin_chunk, horizontal_chunk(movement.position))
                else {
                    return false;
                };
                let far_enough = i64::from(origin[0]).abs_diff(i64::from(target[0]))
                    >= FULL_VIEW_TELEPORT_MIN_CHUNK_DELTA
                    || i64::from(origin[1]).abs_diff(i64::from(target[1]))
                        >= FULL_VIEW_TELEPORT_MIN_CHUNK_DELTA;
                if !far_enough {
                    return false;
                }
                if self
                    .pending_move_ingress
                    .is_none_or(|(pending_sequence, _, _)| sequence < pending_sequence)
                {
                    self.pending_move_ingress = Some((sequence, observed_at, frame_count));
                    return true;
                }
                false
            }
            protocol::WorldEvent::LevelChunk(event) => {
                if let Some(pending) = &mut self.pending
                    && sequence > pending.move_sequence
                    && pending
                        .target
                        .contains_column(event.dimension, [event.x, event.z])
                {
                    let latency = observed_at.saturating_duration_since(pending.started);
                    pending.first_level_chunk_latency.get_or_insert(latency);
                    pending.last_level_chunk_latency = Some(latency);
                    pending.level_chunk_events = pending.level_chunk_events.saturating_add(1);
                }
                false
            }
            protocol::WorldEvent::SubChunks(batch) => {
                if let Some(pending) = &mut self.pending
                    && sequence > pending.move_sequence
                {
                    let target_entries = batch
                        .entries
                        .iter()
                        .filter(|entry| {
                            pending.target.contains_column(
                                batch.dimension,
                                [entry.position[0], entry.position[2]],
                            )
                        })
                        .count();
                    if target_entries == 0 {
                        return false;
                    }
                    let latency = observed_at.saturating_duration_since(pending.started);
                    pending.first_sub_chunk_latency.get_or_insert(latency);
                    pending.last_sub_chunk_latency = Some(latency);
                    pending.sub_chunk_events = pending
                        .sub_chunk_events
                        .saturating_add(u64::try_from(target_entries).unwrap_or(u64::MAX));
                }
                false
            }
            _ => false,
        }
    }

    fn observe_committed_control(&mut self, control: &CommittedControlEvent) -> bool {
        let CommittedControlEvent::MovePlayer {
            sequence,
            movement,
            source_cohort,
            ..
        } = control
        else {
            return false;
        };
        self.commit_move(*sequence, *movement, *source_cohort)
    }

    fn commit_move(
        &mut self,
        sequence: u64,
        movement: protocol::MovePlayerEvent,
        source_cohort: Option<ViewCohort>,
    ) -> bool {
        if !self.enabled || self.completed.is_some() || self.pending.is_some() {
            return false;
        }
        let Some((pending_sequence, started, started_frame_count)) = self.pending_move_ingress
        else {
            return false;
        };
        if pending_sequence != sequence {
            return false;
        }
        self.pending_move_ingress = None;
        if self.local_player_runtime_id != Some(movement.runtime_id) {
            return false;
        }
        let (Some(origin), Some(target_center)) =
            (self.origin_chunk, horizontal_chunk(movement.position))
        else {
            return false;
        };
        let Some(target_mutation_coordinate) = self
            .source_mutation_coordinate
            .and_then(|source| leaf_forest_target_mutation_coordinate(movement.position, source))
        else {
            return false;
        };
        let far_enough = i64::from(origin[0]).abs_diff(i64::from(target_center[0]))
            >= FULL_VIEW_TELEPORT_MIN_CHUNK_DELTA
            || i64::from(origin[1]).abs_diff(i64::from(target_center[1]))
                >= FULL_VIEW_TELEPORT_MIN_CHUNK_DELTA;
        if !far_enough {
            return false;
        }
        let Some(source) =
            source_cohort.filter(|source| source.radius == PHASE0_REQUESTED_RADIUS_CHUNKS)
        else {
            return false;
        };
        let target = ViewCohort {
            dimension: source.dimension,
            center: target_center,
            radius: PHASE0_REQUESTED_RADIUS_CHUNKS,
        };
        if source == target {
            return false;
        }
        let matching_publisher =
            self.latest_publisher_ingress
                .filter(|(publisher_sequence, publisher, _)| {
                    *publisher_sequence > sequence && *publisher == target
                });
        self.pending = Some(PendingFullViewTeleport {
            started,
            started_frame_count,
            move_sequence: sequence,
            target,
            source,
            target_mutation_coordinate,
            publisher_seen: matching_publisher.is_some(),
            publisher_latency: matching_publisher
                .and_then(|(_, _, observed_at)| observed_at.checked_duration_since(started)),
            first_level_chunk_latency: None,
            last_level_chunk_latency: None,
            level_chunk_events: 0,
            first_sub_chunk_latency: None,
            last_sub_chunk_latency: None,
            sub_chunk_events: 0,
            peak_network_events: 0,
            presented_candidate: None,
            last_progress_at: None,
        });
        true
    }

    #[cfg(test)]
    fn observe(
        &mut self,
        event: &protocol::WorldEvent,
        observed_at: Instant,
        current_dimension: i32,
    ) -> bool {
        self.next_test_sequence = self.next_test_sequence.saturating_add(1);
        let sequence = self.next_test_sequence;
        let source = self.origin_chunk.map(|center| ViewCohort {
            dimension: current_dimension,
            center,
            radius: PHASE0_REQUESTED_RADIUS_CHUNKS,
        });
        let capture = self.observe_ingress(
            event,
            sequence,
            observed_at,
            current_dimension,
            self.next_test_sequence,
        );
        if capture && let protocol::WorldEvent::MovePlayer(movement) = event {
            return self.commit_move(sequence, *movement, source);
        }
        false
    }

    fn reconcile_presented_expectation(
        &mut self,
        snapshot: TeleportReadySnapshot,
        mut proposed: TargetRenderExpectation,
        now: Instant,
    ) -> Option<TargetRenderExpectation> {
        let pending = self.pending.as_mut()?;
        pending.peak_network_events = pending
            .peak_network_events
            .max(snapshot.work.network_events);
        let Some(status) = snapshot.cohort else {
            pending.presented_candidate = None;
            return None;
        };
        let target = render_view_cohort(pending.target);
        let source = render_view_cohort(pending.source);
        if !pending.publisher_seen
            || !snapshot.is_binding_ready()
            || status.target != pending.target
            || proposed.cohort != target
            || proposed.source_cohort != Some(source)
            || proposed.manifest.is_empty()
        {
            pending.presented_candidate = None;
            return None;
        }

        if let Some(candidate) = &mut pending.presented_candidate
            && candidate.status == status
            && candidate.expectation.cohort == proposed.cohort
            && candidate.expectation.source_cohort == proposed.source_cohort
            && candidate.expectation.manifest == proposed.manifest
        {
            candidate.snapshot = snapshot;
            return Some(candidate.expectation.clone());
        }

        self.next_view_generation = self.next_view_generation.wrapping_add(1).max(1);
        proposed.view_generation = self.next_view_generation;
        proposed.render_ready_at = now;
        pending.presented_candidate = Some(TeleportPresentedCandidate {
            snapshot,
            status,
            expectation: proposed.clone(),
            first_frame: None,
        });
        Some(proposed)
    }

    fn observe_presented_frame(
        &mut self,
        acknowledgement: PresentedFrameAck,
    ) -> Option<FullViewTeleportCompletion> {
        let completion = {
            let pending = self.pending.as_mut()?;
            let candidate = pending.presented_candidate.as_mut()?;
            if !presented_ack_matches(pending.started, &candidate.expectation, &acknowledgement) {
                candidate.first_frame = None;
                return None;
            }
            let Some(first) = candidate.first_frame.take() else {
                candidate.first_frame = Some(acknowledgement);
                return None;
            };
            if !first.forms_stable_exact_pair_with(&acknowledgement)
                || first.present_returned_at > acknowledgement.present_returned_at
            {
                candidate.first_frame = Some(acknowledgement);
                return None;
            }

            let started = pending.started;
            Some(FullViewTeleportCompletion {
                settle_latency: acknowledgement
                    .gpu_completed_at
                    .checked_duration_since(started)?,
                render_ready_latency: candidate
                    .expectation
                    .render_ready_at
                    .checked_duration_since(started)?,
                first_present_return_latency: first
                    .present_returned_at
                    .checked_duration_since(started)?,
                first_gpu_completion_latency: first
                    .gpu_completed_at
                    .checked_duration_since(started)?,
                stable_present_return_latency: acknowledgement
                    .present_returned_at
                    .checked_duration_since(started)?,
                stable_gpu_completion_latency: acknowledgement
                    .gpu_completed_at
                    .checked_duration_since(started)?,
                view_generation: candidate.expectation.view_generation,
                target_mutation_coordinate: pending.target_mutation_coordinate,
                publisher_latency: pending.publisher_latency,
                first_level_chunk_latency: pending.first_level_chunk_latency,
                last_level_chunk_latency: pending.last_level_chunk_latency,
                level_chunk_events: pending.level_chunk_events,
                first_sub_chunk_latency: pending.first_sub_chunk_latency,
                last_sub_chunk_latency: pending.last_sub_chunk_latency,
                sub_chunk_events: pending.sub_chunk_events,
                last_chunk_commit_latency: latency_after(
                    started,
                    candidate.snapshot.last_chunk_commit_at,
                ),
                last_mesh_dispatch_latency: latency_after(
                    started,
                    candidate.snapshot.last_mesh_dispatch_at,
                ),
                last_mesh_completion_latency: latency_after(
                    started,
                    candidate.snapshot.last_mesh_completion_at,
                ),
                last_mesh_ack_latency: latency_after(started, candidate.snapshot.last_mesh_ack_at),
                peak_network_events: pending.peak_network_events,
                expectation: candidate.expectation.clone(),
                first_frame: first,
                stable_frame: acknowledgement,
                frame_count: self
                    .current_frame_count
                    .saturating_sub(pending.started_frame_count)
                    .saturating_add(1)
                    .max(2),
            })
        };
        if let Some(completion) = &completion {
            self.completed_target = self.pending.as_ref().map(|pending| pending.target);
            self.completed_target_mutation = Some(completion.target_mutation_coordinate);
            self.pending = None;
            self.completed = Some(completion.settle_latency);
        }
        completion
    }

    #[cfg(test)]
    fn observe_snapshot(
        &mut self,
        snapshot: TeleportReadySnapshot,
        now: Instant,
    ) -> Option<FullViewTeleportCompletion> {
        let pending = self.pending.as_ref()?;
        let proposed = TargetRenderExpectation {
            cohort: render_view_cohort(pending.target),
            source_cohort: Some(render_view_cohort(pending.source)),
            manifest: Arc::from([(
                SubChunkKey::new(
                    pending.target.dimension,
                    0,
                    pending.target.center[0],
                    pending.target.center[1],
                ),
                1,
            )]),
            view_generation: 0,
            render_ready_at: now,
        };
        let _ = self.reconcile_presented_expectation(snapshot, proposed, now);
        None
    }

    fn target_cohort(&self) -> Option<ViewCohort> {
        self.pending
            .as_ref()
            .map(|pending| pending.target)
            .or(self.completed_target)
    }

    fn note_frame(&mut self, frame_count: u64) {
        self.current_frame_count = frame_count;
    }

    fn cohort_progress_line(
        &mut self,
        status: ViewCohortStatus,
        work: WorldReadyWork,
        timeout_progress: SubChunkTimeoutProgress,
        now: Instant,
    ) -> Option<String> {
        let pending = self.pending.as_mut()?;
        if pending.last_progress_at.is_some_and(|previous| {
            now.saturating_duration_since(previous) < TELEPORT_COHORT_PROGRESS_INTERVAL
        }) {
            return None;
        }
        pending.last_progress_at = Some(now);
        let committed = status
            .committed
            .map_or_else(|| "none".to_owned(), cohort_tag);
        Some(format!(
            "RUST_MCBE_TELEPORT_COHORT target={} committed={} exact={} expected={} loaded_target={} missing_target={} foreign_loaded={} foreign_requested={} foreign_resident={} source_leftover={} resident_count={} resident_hash={:016x} known_air_count={} known_air_hash={:016x} network_events={} network_commands={} admitted_world_events={} queued_decode_jobs={} in_flight_decode_jobs={} completed_decode_results={} pending_mesh_jobs={} in_flight_mesh_jobs={} pending_mesh_changes={} outbound_requests={} outstanding_sub_chunks={} pending_retry_requests={} awaiting_sub_chunk_responses={} sub_chunk_timeouts={} sub_chunk_retries_scheduled={} sub_chunk_retry_exhaustions={} render_queue_items={} pending_gpu_acknowledgements={} unacknowledged_meshes={}",
            cohort_tag(pending.target),
            committed,
            status.is_exact(),
            status.expected,
            status.loaded_target,
            status.missing_target,
            status.foreign_loaded,
            status.foreign_requested,
            status.foreign_resident,
            status.source_leftover,
            status.resident_count,
            status.resident_hash,
            status.known_air_count,
            status.known_air_hash,
            work.network_events,
            work.network_commands,
            work.admitted_world_events,
            work.queued_decode_jobs,
            work.in_flight_decode_jobs,
            work.completed_decode_results,
            work.pending_mesh_jobs,
            work.in_flight_mesh_jobs,
            work.pending_mesh_changes,
            work.outbound_requests,
            work.outstanding_sub_chunks,
            work.pending_retry_requests,
            timeout_progress.awaiting_responses,
            timeout_progress.timeouts,
            timeout_progress.retries_scheduled,
            timeout_progress.retry_exhaustions,
            work.render_queue_items,
            work.pending_gpu_acknowledgements,
            work.unacknowledged_meshes,
        ))
    }

    #[cfg(test)]
    const fn is_pending(&self) -> bool {
        self.pending.is_some()
    }

    #[cfg(test)]
    fn has_clean_candidate(&self) -> bool {
        self.pending
            .as_ref()
            .is_some_and(|pending| pending.presented_candidate.is_some())
    }
}

const fn render_view_cohort(cohort: ViewCohort) -> RenderViewCohort {
    RenderViewCohort::new(cohort.dimension, cohort.center, cohort.radius)
}

fn presented_ack_matches(
    started: Instant,
    expectation: &TargetRenderExpectation,
    acknowledgement: &PresentedFrameAck,
) -> bool {
    acknowledgement.cohort == expectation.cohort
        && acknowledgement.view_generation == expectation.view_generation
        && acknowledgement.render_ready_at == expectation.render_ready_at
        && acknowledgement.allocation_manifest == expectation.manifest
        && acknowledgement.is_exact()
        && acknowledgement
            .render_ready_at
            .checked_duration_since(started)
            .is_some()
        && acknowledgement
            .present_returned_at
            .checked_duration_since(acknowledgement.render_ready_at)
            .is_some()
        && acknowledgement
            .gpu_completed_at
            .checked_duration_since(acknowledgement.present_returned_at)
            .is_some()
}

fn cohort_tag(cohort: ViewCohort) -> String {
    format!(
        "{}:{}:{}:{}",
        cohort.dimension, cohort.center[0], cohort.center[1], cohort.radius
    )
}

fn teleport_global_stage_diagnostic_marker(
    target: ViewCohort,
    completion: &FullViewTeleportCompletion,
) -> String {
    format!(
        "RUST_MCBE_TELEPORT_GLOBAL_STAGE_DIAGNOSTIC target={} global_commit_ms={} global_mesh_dispatch_ms={} global_mesh_complete_ms={} global_mesh_ack_ms={}",
        cohort_tag(target),
        optional_milliseconds_token(optional_duration_milliseconds(
            completion.last_chunk_commit_latency,
        )),
        optional_milliseconds_token(optional_duration_milliseconds(
            completion.last_mesh_dispatch_latency,
        )),
        optional_milliseconds_token(optional_duration_milliseconds(
            completion.last_mesh_completion_latency,
        )),
        optional_milliseconds_token(optional_duration_milliseconds(
            completion.last_mesh_ack_latency,
        )),
    )
}

fn latency_after(started: Instant, observed: Option<Instant>) -> Option<Duration> {
    observed.and_then(|observed| observed.checked_duration_since(started))
}

#[derive(Debug, Clone)]
struct FullViewRemeshPresentedCandidate {
    expectation: TargetRenderExpectation,
    first_frame: Option<PresentedFrameAck>,
}

#[derive(Debug, Clone)]
struct PendingFullViewRemesh {
    manifest: world_stream::ForcedRemeshManifest,
    cohort: ViewCohortStatus,
    source_cohort: Option<RenderViewCohort>,
    binding_manifest: Arc<[(SubChunkKey, u64)]>,
    binding_view_generation: u64,
    started_frame_count: u64,
    candidate: Option<FullViewRemeshPresentedCandidate>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
struct FullViewRemeshCompletion {
    settle_latency: Duration,
    render_ready_latency: Duration,
    first_present_return_latency: Duration,
    first_gpu_completion_latency: Duration,
    stable_present_return_latency: Duration,
    stable_gpu_completion_latency: Duration,
    view_generation: u64,
    expectation: TargetRenderExpectation,
    first_frame: PresentedFrameAck,
    stable_frame: PresentedFrameAck,
    frame_count: u64,
}

#[derive(Debug, Default)]
struct FullViewRemeshTracker {
    pending: Option<PendingFullViewRemesh>,
    completed: Option<FullViewRemeshCompletion>,
    invalidated: bool,
}

impl FullViewRemeshTracker {
    fn start(
        &mut self,
        binding: Option<&FullViewTeleportCompletion>,
        cohort: ViewCohortStatus,
        manifest: world_stream::ForcedRemeshManifest,
        frame_count: u64,
    ) -> bool {
        let Some(binding) = binding else {
            return false;
        };
        let manifest_keys = manifest
            .entries
            .iter()
            .map(|(key, _)| *key)
            .collect::<BTreeSet<_>>();
        if manifest.is_empty()
            || binding.expectation.manifest.is_empty()
            || manifest.started_at < binding.stable_frame.gpu_completed_at
            || !cohort.is_exact()
            || render_view_cohort(cohort.target) != binding.expectation.cohort
            || !binding
                .expectation
                .manifest
                .iter()
                .all(|(key, _)| manifest_keys.contains(key))
            || self.pending.is_some()
            || self.completed.is_some()
            || self.invalidated
        {
            return false;
        }
        self.pending = Some(PendingFullViewRemesh {
            manifest,
            cohort,
            source_cohort: binding.expectation.source_cohort,
            binding_manifest: Arc::clone(&binding.expectation.manifest),
            binding_view_generation: binding.view_generation,
            started_frame_count: frame_count,
            candidate: None,
        });
        true
    }

    fn reconcile_presented_expectation(
        &mut self,
        snapshot: TeleportReadySnapshot,
        manifest_state: world_stream::ForcedRemeshManifestState,
        proposed: Option<TargetRenderExpectation>,
        now: Instant,
        _frame_count: u64,
    ) -> Option<TargetRenderExpectation> {
        if manifest_state == world_stream::ForcedRemeshManifestState::Invalid {
            self.invalidate();
            return None;
        }
        let pending = self.pending.as_ref()?;
        if snapshot.received_radius_chunks != Some(PHASE0_REQUESTED_RADIUS_CHUNKS)
            || snapshot.publisher_radius_chunks != Some(PHASE0_REQUESTED_RADIUS_CHUNKS)
            || snapshot.cohort != Some(pending.cohort)
        {
            self.invalidate();
            return None;
        }
        if manifest_state == world_stream::ForcedRemeshManifestState::Pending
            || !snapshot.is_ready()
        {
            self.pending
                .as_mut()
                .expect("validated forced remesh remains pending")
                .candidate = None;
            return None;
        }
        let Some(mut proposed) = proposed else {
            self.invalidate();
            return None;
        };

        if let Some(candidate) = &pending.candidate {
            if proposed.cohort != candidate.expectation.cohort
                || proposed.source_cohort != candidate.expectation.source_cohort
                || proposed.manifest != candidate.expectation.manifest
            {
                self.invalidate();
                return None;
            }
            return Some(candidate.expectation.clone());
        }

        let binding_keys = pending
            .binding_manifest
            .iter()
            .map(|(key, _)| *key)
            .collect::<BTreeSet<_>>();
        let proposed_keys = proposed
            .manifest
            .iter()
            .map(|(key, _)| *key)
            .collect::<BTreeSet<_>>();
        let proposal_is_forced = !proposed.manifest.is_empty()
            && proposed.cohort == render_view_cohort(pending.cohort.target)
            && proposed.source_cohort == pending.source_cohort
            && proposed.manifest.len() == pending.binding_manifest.len()
            && proposed_keys == binding_keys
            && proposed.manifest != pending.binding_manifest
            && proposed
                .manifest
                .iter()
                .all(|entry| pending.manifest.entries.contains(entry));
        if !proposal_is_forced {
            self.invalidate();
            return None;
        }

        proposed.view_generation = pending.binding_view_generation.wrapping_add(1).max(1);
        proposed.render_ready_at = now;
        let expectation = proposed.clone();
        self.pending
            .as_mut()
            .expect("validated forced remesh remains pending")
            .candidate = Some(FullViewRemeshPresentedCandidate {
            expectation,
            first_frame: None,
        });
        Some(proposed)
    }

    fn observe_presented_frame(
        &mut self,
        acknowledgement: PresentedFrameAck,
        frame_count: u64,
    ) -> Option<FullViewRemeshCompletion> {
        let invalid_current_evidence = {
            let pending = self.pending.as_ref()?;
            let candidate = pending.candidate.as_ref()?;
            acknowledgement.cohort == candidate.expectation.cohort
                && acknowledgement.view_generation == candidate.expectation.view_generation
                && acknowledgement.render_ready_at == candidate.expectation.render_ready_at
                && !presented_ack_matches(
                    pending.manifest.started_at,
                    &candidate.expectation,
                    &acknowledgement,
                )
        };
        if invalid_current_evidence {
            self.invalidate();
            return None;
        }
        let completion = {
            let pending = self.pending.as_mut()?;
            let candidate = pending.candidate.as_mut()?;
            if !presented_ack_matches(
                pending.manifest.started_at,
                &candidate.expectation,
                &acknowledgement,
            ) {
                candidate.first_frame = None;
                return None;
            }
            let Some(first) = candidate.first_frame.take() else {
                candidate.first_frame = Some(acknowledgement);
                return None;
            };
            if !first.forms_stable_exact_pair_with(&acknowledgement)
                || first.present_returned_at > acknowledgement.present_returned_at
            {
                candidate.first_frame = Some(acknowledgement);
                return None;
            }

            let started = pending.manifest.started_at;
            Some(FullViewRemeshCompletion {
                settle_latency: acknowledgement
                    .gpu_completed_at
                    .checked_duration_since(started)?,
                render_ready_latency: candidate
                    .expectation
                    .render_ready_at
                    .checked_duration_since(started)?,
                first_present_return_latency: first
                    .present_returned_at
                    .checked_duration_since(started)?,
                first_gpu_completion_latency: first
                    .gpu_completed_at
                    .checked_duration_since(started)?,
                stable_present_return_latency: acknowledgement
                    .present_returned_at
                    .checked_duration_since(started)?,
                stable_gpu_completion_latency: acknowledgement
                    .gpu_completed_at
                    .checked_duration_since(started)?,
                view_generation: candidate.expectation.view_generation,
                expectation: candidate.expectation.clone(),
                first_frame: first,
                stable_frame: acknowledgement,
                frame_count: frame_count
                    .saturating_sub(pending.started_frame_count)
                    .saturating_add(1)
                    .max(2),
            })
        };
        if let Some(completion) = &completion {
            self.completed = Some(completion.clone());
            self.pending = None;
        }
        completion
    }

    fn invalidate(&mut self) {
        self.pending = None;
        self.invalidated = true;
    }

    #[cfg(test)]
    fn is_pending(&self) -> bool {
        self.pending.is_some()
    }

    #[cfg(test)]
    const fn is_invalidated(&self) -> bool {
        self.invalidated
    }
}

struct FullViewCompletionEvidence<'a> {
    settle_latency: Duration,
    render_ready_latency: Duration,
    first_present_return_latency: Duration,
    first_gpu_completion_latency: Duration,
    stable_present_return_latency: Duration,
    stable_gpu_completion_latency: Duration,
    view_generation: u64,
    expectation: &'a TargetRenderExpectation,
    first_frame: &'a PresentedFrameAck,
    stable_frame: &'a PresentedFrameAck,
    frame_count: u64,
}

fn exact_full_view_proof(
    status: ViewCohortStatus,
    evidence: FullViewCompletionEvidence<'_>,
) -> ExactFullViewProof {
    let manifest_evidence = |manifest: &[(SubChunkKey, u64)]| {
        (
            manifest.len(),
            format!("{:016x}", deterministic_manifest_hash(manifest)),
        )
    };
    let (expected_manifest_count, expected_manifest_hash) =
        manifest_evidence(&evidence.expectation.manifest);
    let (first_presented_manifest_count, first_presented_manifest_hash) =
        manifest_evidence(&evidence.first_frame.drawn_manifest);
    let (stable_presented_manifest_count, stable_presented_manifest_hash) =
        manifest_evidence(&evidence.stable_frame.drawn_manifest);
    ExactFullViewProof {
        target: cohort_tag(status.target),
        committed: status
            .committed
            .map_or_else(|| "none".to_owned(), cohort_tag),
        ms: duration_milliseconds(evidence.settle_latency),
        view_generation: evidence.view_generation,
        transparent_sort_generation: evidence.stable_frame.transparent_sort_generation,
        render_ready_ms: duration_milliseconds(evidence.render_ready_latency),
        first_frame_sequence: evidence.first_frame.frame_sequence,
        stable_frame_sequence: evidence.stable_frame.frame_sequence,
        first_present_ms: duration_milliseconds(evidence.first_present_return_latency),
        first_gpu_ms: duration_milliseconds(evidence.first_gpu_completion_latency),
        stable_present_ms: duration_milliseconds(evidence.stable_present_return_latency),
        stable_gpu_ms: duration_milliseconds(evidence.stable_gpu_completion_latency),
        frame_count: evidence.frame_count,
        expected_manifest_count,
        expected_manifest_hash,
        first_presented_manifest_count,
        first_presented_manifest_hash,
        stable_presented_manifest_count,
        stable_presented_manifest_hash,
        expected: status.expected,
        loaded_target: status.loaded_target,
        missing_target: status.missing_target,
        foreign_loaded: status.foreign_loaded,
        foreign_requested: status.foreign_requested,
        foreign_resident: status.foreign_resident,
        source_leftover: status.source_leftover,
        resident_count: status.resident_count,
        resident_hash: format!("{:016x}", status.resident_hash),
        known_air_count: status.known_air_count,
        known_air_hash: format!("{:016x}", status.known_air_hash),
        missing_target_instances: evidence.stable_frame.missing_target_instances,
        unexpected_target_instances: evidence.stable_frame.unexpected_target_instances,
        source_instances: evidence.stable_frame.source_instances,
        foreign_instances: evidence.stable_frame.foreign_instances,
        stale_generation_instances: evidence.stable_frame.stale_generation_instances,
        orphan_allocations: evidence.stable_frame.orphan_allocations,
    }
}

fn teleport_proof(
    status: ViewCohortStatus,
    completion: &FullViewTeleportCompletion,
) -> TeleportProof {
    TeleportProof {
        exact: exact_full_view_proof(
            status,
            FullViewCompletionEvidence {
                settle_latency: completion.settle_latency,
                render_ready_latency: completion.render_ready_latency,
                first_present_return_latency: completion.first_present_return_latency,
                first_gpu_completion_latency: completion.first_gpu_completion_latency,
                stable_present_return_latency: completion.stable_present_return_latency,
                stable_gpu_completion_latency: completion.stable_gpu_completion_latency,
                view_generation: completion.view_generation,
                expectation: &completion.expectation,
                first_frame: &completion.first_frame,
                stable_frame: &completion.stable_frame,
                frame_count: completion.frame_count,
            },
        ),
        publisher_ms: optional_duration_milliseconds(completion.publisher_latency),
        first_level_ms: optional_duration_milliseconds(completion.first_level_chunk_latency),
        last_level_ms: optional_duration_milliseconds(completion.last_level_chunk_latency),
        level_events: completion.level_chunk_events,
        first_sub_ms: optional_duration_milliseconds(completion.first_sub_chunk_latency),
        last_sub_ms: optional_duration_milliseconds(completion.last_sub_chunk_latency),
        sub_events: completion.sub_chunk_events,
    }
}

fn forced_remesh_proof(
    status: ViewCohortStatus,
    completion: &FullViewRemeshCompletion,
) -> ExactFullViewProof {
    exact_full_view_proof(
        status,
        FullViewCompletionEvidence {
            settle_latency: completion.settle_latency,
            render_ready_latency: completion.render_ready_latency,
            first_present_return_latency: completion.first_present_return_latency,
            first_gpu_completion_latency: completion.first_gpu_completion_latency,
            stable_present_return_latency: completion.stable_present_return_latency,
            stable_gpu_completion_latency: completion.stable_gpu_completion_latency,
            view_generation: completion.view_generation,
            expectation: &completion.expectation,
            first_frame: &completion.first_frame,
            stable_frame: &completion.stable_frame,
            frame_count: completion.frame_count,
        },
    )
}

fn exact_full_view_proof_marker_fields(proof: &ExactFullViewProof) -> String {
    format!(
        "target={} committed={} ms={:.4} view_generation={} transparent_sort_generation={} render_ready_ms={:.4} first_frame_sequence={} stable_frame_sequence={} first_present_ms={:.4} first_gpu_ms={:.4} stable_present_ms={:.4} stable_gpu_ms={:.4} frame_count={} expected_manifest_count={} expected_manifest_hash={} first_presented_manifest_count={} first_presented_manifest_hash={} stable_presented_manifest_count={} stable_presented_manifest_hash={} expected={} loaded_target={} missing_target={} foreign_loaded={} foreign_requested={} foreign_resident={} source_leftover={} resident_count={} resident_hash={} known_air_count={} known_air_hash={} missing_target_instances={} unexpected_target_instances={} source_instances={} foreign_instances={} stale_generation_instances={} orphan_allocations={}",
        proof.target,
        proof.committed,
        proof.ms,
        proof.view_generation,
        proof.transparent_sort_generation,
        proof.render_ready_ms,
        proof.first_frame_sequence,
        proof.stable_frame_sequence,
        proof.first_present_ms,
        proof.first_gpu_ms,
        proof.stable_present_ms,
        proof.stable_gpu_ms,
        proof.frame_count,
        proof.expected_manifest_count,
        proof.expected_manifest_hash,
        proof.first_presented_manifest_count,
        proof.first_presented_manifest_hash,
        proof.stable_presented_manifest_count,
        proof.stable_presented_manifest_hash,
        proof.expected,
        proof.loaded_target,
        proof.missing_target,
        proof.foreign_loaded,
        proof.foreign_requested,
        proof.foreign_resident,
        proof.source_leftover,
        proof.resident_count,
        proof.resident_hash,
        proof.known_air_count,
        proof.known_air_hash,
        proof.missing_target_instances,
        proof.unexpected_target_instances,
        proof.source_instances,
        proof.foreign_instances,
        proof.stale_generation_instances,
        proof.orphan_allocations,
    )
}

fn teleport_settled_marker(proof: &TeleportProof) -> String {
    format!(
        "RUST_MCBE_TELEPORT_SETTLED {} publisher_ms={} first_level_ms={} last_level_ms={} level_events={} first_sub_ms={} last_sub_ms={} sub_events={}",
        exact_full_view_proof_marker_fields(&proof.exact),
        optional_milliseconds_token(proof.publisher_ms),
        optional_milliseconds_token(proof.first_level_ms),
        optional_milliseconds_token(proof.last_level_ms),
        proof.level_events,
        optional_milliseconds_token(proof.first_sub_ms),
        optional_milliseconds_token(proof.last_sub_ms),
        proof.sub_events,
    )
}

fn forced_remesh_settled_marker(proof: &ExactFullViewProof) -> String {
    format!(
        "RUST_MCBE_FORCED_FULL_VIEW_REMESH_SETTLED {}",
        exact_full_view_proof_marker_fields(proof)
    )
}

fn duration_milliseconds(duration: Duration) -> f64 {
    duration.as_secs_f64() * 1_000.0
}

fn optional_duration_milliseconds(duration: Option<Duration>) -> Option<f64> {
    duration.map(duration_milliseconds)
}

fn optional_milliseconds_token(milliseconds: Option<f64>) -> String {
    milliseconds.map_or_else(|| "null".to_owned(), |value| format!("{value:.4}"))
}

fn horizontal_chunk(position: [f32; 3]) -> Option<[i32; 2]> {
    if !position[0].is_finite() || !position[2].is_finite() {
        return None;
    }
    Some([
        (position[0].floor() as i32).div_euclid(16),
        (position[2].floor() as i32).div_euclid(16),
    ])
}

#[derive(Resource)]
struct AcceptanceRun {
    duration: Option<Duration>,
    deadline: Option<Instant>,
    metrics_out: Option<PathBuf>,
    mutation_surface_anchor: Option<[i32; 2]>,
    source_mutation_coordinate: Option<[i32; 3]>,
    mutation: Option<MutationTracker>,
    gallery_anchor: GalleryAnchorEmitter,
    world_ready_settler: WorldReadySettler,
    full_view_teleport: FullViewTeleportTracker,
    full_view_remesh: FullViewRemeshTracker,
    world_ready: bool,
    require_transparent_presentation: bool,
    finished: bool,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum AcceptanceExitDecision {
    Continue,
    WaitForTransparentPresentation,
    Complete,
    Fatal,
    TransparentPresentationTimedOut,
}

impl AcceptanceExitDecision {
    const fn is_error(self) -> bool {
        matches!(self, Self::Fatal | Self::TransparentPresentationTimedOut)
    }
}

impl AcceptanceRun {
    fn new(
        seconds: Option<u64>,
        metrics_out: Option<PathBuf>,
        full_view_teleport_gate: bool,
        require_transparent_presentation: bool,
    ) -> Self {
        Self {
            duration: seconds.map(Duration::from_secs),
            deadline: None,
            metrics_out,
            mutation_surface_anchor: None,
            source_mutation_coordinate: None,
            mutation: None,
            gallery_anchor: GalleryAnchorEmitter::default(),
            world_ready_settler: WorldReadySettler::default(),
            full_view_teleport: FullViewTeleportTracker::new(full_view_teleport_gate),
            full_view_remesh: FullViewRemeshTracker::default(),
            world_ready: false,
            require_transparent_presentation,
            finished: false,
        }
    }

    fn exit_decision(
        &self,
        now: Instant,
        fatal: bool,
        transparent: TransparentSortMetricsSnapshot,
    ) -> AcceptanceExitDecision {
        if fatal {
            return AcceptanceExitDecision::Fatal;
        }
        let Some(deadline) = self.deadline else {
            return AcceptanceExitDecision::Continue;
        };
        if now < deadline {
            return AcceptanceExitDecision::Continue;
        }
        if !self.require_transparent_presentation {
            return AcceptanceExitDecision::Complete;
        }
        if transparent.ref_count > 0
            && transparent.committed_generation != 0
            && transparent.committed_generation == transparent.encoded_generation
            && transparent.committed_generation == transparent.presented_generation
        {
            return AcceptanceExitDecision::Complete;
        }
        let grace_deadline = deadline
            .checked_add(TRANSPARENT_PRESENTATION_EXIT_GRACE)
            .expect("transparent presentation grace deadline overflowed");
        if now < grace_deadline {
            AcceptanceExitDecision::WaitForTransparentPresentation
        } else {
            AcceptanceExitDecision::TransparentPresentationTimedOut
        }
    }

    fn enabled(&self) -> bool {
        self.duration.is_some()
    }

    fn begin_world_ready(
        &mut self,
        ready_at: Instant,
        position: [f32; 3],
        local_player_runtime_id: u64,
    ) {
        self.deadline = self.duration.map(|duration| ready_at + duration);
        self.world_ready = true;
        self.full_view_teleport
            .begin_world_ready(position, local_player_runtime_id);
    }

    fn set_mutation_surface_anchor(&mut self, anchor: [i32; 2]) {
        self.mutation_surface_anchor = Some(anchor);
    }

    fn mutation_surface_anchor(&self) -> Option<[i32; 2]> {
        self.mutation_surface_anchor
    }

    fn set_mutation_coordinate(&mut self, coordinate: [i32; 3]) {
        self.mutation_surface_anchor = None;
        self.source_mutation_coordinate = Some(coordinate);
        self.full_view_teleport
            .set_source_mutation_coordinate(coordinate);
        if !self.full_view_teleport.enabled {
            self.mutation = Some(MutationTracker::new(coordinate));
        }
    }

    fn source_mutation_coordinate(&self) -> Option<[i32; 3]> {
        self.source_mutation_coordinate
    }

    fn retarget_mutation(&mut self, coordinate: [i32; 3], armed_at: Instant) -> bool {
        if self.full_view_teleport.completed_target_mutation != Some(coordinate)
            || self.full_view_remesh.completed.is_none()
            || self
                .mutation
                .as_ref()
                .is_some_and(|mutation| mutation.coordinate() == coordinate)
        {
            return false;
        }
        self.mutation_surface_anchor = None;
        self.mutation = Some(MutationTracker::armed(coordinate, armed_at));
        true
    }

    fn target_mutation_marker(&self) -> Option<String> {
        let source = self.source_mutation_coordinate()?;
        let target = self.mutation.as_ref()?.coordinate();
        if self.full_view_teleport.completed_target_mutation != Some(target) {
            return None;
        }
        let view_generation = self.full_view_remesh.completed.as_ref()?.view_generation;
        Some(target_mutation_armed_marker(
            source,
            target,
            view_generation,
        ))
    }

    fn observe_mutation(&mut self, event: &protocol::WorldEvent, observed_at: Instant) {
        if let Some(mutation) = &mut self.mutation {
            mutation.observe(event, observed_at);
        }
    }

    fn observe_full_view_teleport_ingress(
        &mut self,
        event: &protocol::WorldEvent,
        sequence: u64,
        observed_at: Instant,
        current_dimension: i32,
        frame_count: u64,
    ) -> bool {
        self.world_ready
            && self.full_view_teleport.observe_ingress(
                event,
                sequence,
                observed_at,
                current_dimension,
                frame_count,
            )
    }

    fn observe_committed_full_view_control(&mut self, control: &CommittedControlEvent) -> bool {
        self.world_ready && self.full_view_teleport.observe_committed_control(control)
    }

    fn acknowledge_mutation(
        &mut self,
        key: SubChunkKey,
        generation: u64,
        dirty_since: Instant,
        applied_at: Instant,
    ) -> Option<Duration> {
        let requires_presented_frame = self.full_view_teleport.enabled;
        self.mutation.as_mut().and_then(|mutation| {
            mutation.acknowledge_upload(
                key,
                generation,
                dirty_since,
                applied_at,
                requires_presented_frame,
            )
        })
    }

    fn mutation_coordinate(&self) -> Option<[i32; 3]> {
        self.mutation
            .as_ref()
            .map(MutationTracker::coordinate)
            .or(self.source_mutation_coordinate)
    }

    fn visible_mutation_count(&self) -> u64 {
        self.mutation
            .as_ref()
            .map_or(0, MutationTracker::visible_count)
    }

    fn reconcile_mutation_presented_expectation(
        &mut self,
        proposed: TargetRenderExpectation,
        now: Instant,
    ) -> Option<TargetRenderExpectation> {
        let minimum_view_generation = self.full_view_remesh.completed.as_ref()?.view_generation;
        self.mutation.as_mut()?.reconcile_presented_expectation(
            proposed,
            minimum_view_generation,
            now,
        )
    }

    fn observe_presented_mutation(
        &mut self,
        acknowledgement: PresentedFrameAck,
    ) -> Option<Duration> {
        self.mutation
            .as_mut()?
            .observe_presented_frame(acknowledgement)
    }
}

#[derive(Debug, Clone)]
struct PendingMutation {
    key: SubChunkKey,
    observed_at: Instant,
    uploaded_generation: Option<u64>,
    expectation: Option<TargetRenderExpectation>,
}

#[derive(Debug)]
struct MutationTracker {
    coordinate: [i32; 3],
    armed_at: Instant,
    pending: Option<PendingMutation>,
    visible_count: u64,
    next_view_generation: u64,
}

impl MutationTracker {
    fn new(coordinate: [i32; 3]) -> Self {
        Self::armed(coordinate, Instant::now())
    }

    const fn armed(coordinate: [i32; 3], armed_at: Instant) -> Self {
        Self {
            coordinate,
            armed_at,
            pending: None,
            visible_count: 0,
            next_view_generation: 0,
        }
    }

    const fn coordinate(&self) -> [i32; 3] {
        self.coordinate
    }

    fn observe(&mut self, event: &protocol::WorldEvent, observed_at: Instant) -> bool {
        if observed_at < self.armed_at {
            return false;
        }
        let protocol::WorldEvent::BlockUpdates(updates) = event else {
            return false;
        };
        let Some(update) = updates
            .iter()
            .find(|update| update.position == self.coordinate)
        else {
            return false;
        };
        self.pending = Some(PendingMutation {
            key: SubChunkKey::new(
                update.dimension,
                update.position[0].div_euclid(16),
                update.position[1].div_euclid(16),
                update.position[2].div_euclid(16),
            ),
            observed_at,
            uploaded_generation: None,
            expectation: None,
        });
        true
    }

    #[cfg(test)]
    fn acknowledge(
        &mut self,
        key: SubChunkKey,
        dirty_since: Instant,
        applied_at: Instant,
    ) -> Option<Duration> {
        self.acknowledge_upload(key, 0, dirty_since, applied_at, false)
    }

    fn acknowledge_upload(
        &mut self,
        key: SubChunkKey,
        generation: u64,
        dirty_since: Instant,
        applied_at: Instant,
        requires_presented_frame: bool,
    ) -> Option<Duration> {
        let pending = self.pending.as_mut()?;
        if pending.key != key
            || dirty_since < pending.observed_at
            || applied_at < pending.observed_at
        {
            return None;
        }
        if requires_presented_frame {
            pending.uploaded_generation = Some(generation);
            pending.expectation = None;
            return None;
        }
        let observed_at = pending.observed_at;
        self.pending = None;
        self.visible_count = self.visible_count.saturating_add(1);
        Some(applied_at.saturating_duration_since(observed_at))
    }

    fn reconcile_presented_expectation(
        &mut self,
        mut proposed: TargetRenderExpectation,
        minimum_view_generation: u64,
        now: Instant,
    ) -> Option<TargetRenderExpectation> {
        let pending = self.pending.as_ref()?;
        let generation = pending.uploaded_generation?;
        let expected_entry = (pending.key, generation);
        if proposed.manifest.is_empty() || !proposed.manifest.contains(&expected_entry) {
            self.pending
                .as_mut()
                .expect("the mutation pending state was just observed")
                .expectation = None;
            return None;
        }
        if let Some(expectation) = &pending.expectation
            && expectation.cohort == proposed.cohort
            && expectation.source_cohort == proposed.source_cohort
            && expectation.manifest == proposed.manifest
        {
            return Some(expectation.clone());
        }

        self.next_view_generation = self
            .next_view_generation
            .max(minimum_view_generation)
            .wrapping_add(1)
            .max(1);
        proposed.view_generation = self.next_view_generation;
        proposed.render_ready_at = now;
        self.pending
            .as_mut()
            .expect("the mutation pending state was just observed")
            .expectation = Some(proposed.clone());
        Some(proposed)
    }

    fn observe_presented_frame(&mut self, acknowledgement: PresentedFrameAck) -> Option<Duration> {
        let pending = self.pending.as_ref()?;
        let expectation = pending.expectation.as_ref()?;
        let generation = pending.uploaded_generation?;
        if !presented_ack_matches(pending.observed_at, expectation, &acknowledgement)
            || !acknowledgement
                .drawn_manifest
                .contains(&(pending.key, generation))
        {
            return None;
        }
        let latency = acknowledgement
            .gpu_completed_at
            .checked_duration_since(pending.observed_at)?;
        self.pending = None;
        self.visible_count = self.visible_count.saturating_add(1);
        Some(latency)
    }

    const fn visible_count(&self) -> u64 {
        self.visible_count
    }
}

fn deterministic_mutation_coordinate(
    surface_eye_position: [f32; 3],
    surface_anchor: [i32; 2],
) -> [i32; 3] {
    let surface_y = surface_eye_position[1]
        .floor()
        .clamp(i32::MIN as f32, i32::MAX as f32) as i32;
    [
        surface_anchor[0].saturating_add(MUTATION_X_OFFSET_BLOCKS),
        surface_y.saturating_sub(1),
        surface_anchor[1],
    ]
}

fn leaf_forest_target_mutation_coordinate(
    position: [f32; 3],
    source: [i32; 3],
) -> Option<[i32; 3]> {
    let [x, _, z] = position;
    if !x.is_finite() || !z.is_finite() {
        return None;
    }
    let floor_to_i32 = |value: f32| value.floor().clamp(i32::MIN as f32, i32::MAX as f32) as i32;
    let target_x = floor_to_i32(x);
    let target_z = floor_to_i32(z);
    if target_x != source[0].saturating_add(LEAF_FOREST_FAR_OFFSET_BLOCKS)
        || target_z != source[2].saturating_add(LEAF_FOREST_FAR_OFFSET_BLOCKS)
    {
        return None;
    }
    Some([
        target_x,
        source[1],
        target_z.saturating_add(LEAF_FOREST_MUTATION_Z_OFFSET_BLOCKS),
    ])
}

fn move_player_ingress_marker(sequence: u64, position: [f32; 3]) -> Option<String> {
    let [x, y, z] = position;
    if !x.is_finite() || !y.is_finite() || !z.is_finite() {
        return None;
    }
    Some(format!(
        "RUST_MCBE_MOVE_PLAYER_INGRESS sequence={sequence} position={x},{y},{z}"
    ))
}

fn accepted_move_player_ingress_marker(
    accepted: bool,
    sequence: u64,
    event: &protocol::WorldEvent,
) -> Option<String> {
    if !accepted {
        return None;
    }
    let protocol::WorldEvent::MovePlayer(movement) = event else {
        return None;
    };
    move_player_ingress_marker(sequence, movement.position)
}

fn write_move_player_ingress_before_source_capture(
    writer: &mut impl Write,
    marker: &str,
    source_capture: impl FnOnce(),
) {
    write_stdout_marker(writer, marker);
    source_capture();
}

fn write_stdout_marker(writer: &mut impl Write, marker: &str) {
    let _ = writeln!(writer, "{marker}");
    let _ = writer.flush();
}

fn target_mutation_armed_marker(
    source: [i32; 3],
    target: [i32; 3],
    view_generation: u64,
) -> String {
    format!(
        "RUST_MCBE_TARGET_MUTATION_ARMED source={},{},{} target={},{},{} view_generation={view_generation}",
        source[0], source[1], source[2], target[0], target[1], target[2]
    )
}

fn world_ready_markers(snapshot: WorldReadySnapshot) -> Option<[String; 2]> {
    let coordinate = snapshot.mutation_coordinate?;
    if snapshot.received_radius_chunks != Some(PHASE0_REQUESTED_RADIUS_CHUNKS)
        || snapshot.publisher_radius_chunks != Some(PHASE0_REQUESTED_RADIUS_CHUNKS)
        || snapshot.rendered_sub_chunks == 0
        || snapshot.resident_sub_chunks == 0
        || snapshot.visible_sub_chunks == 0
        || !snapshot.mutation_target_rendered
        || !snapshot.mutation_target_visible
        || !snapshot.mutation_target_clean
        || !snapshot.work.is_empty()
    {
        return None;
    }
    Some([
        format!(
            "RUST_MCBE_MUTATION_COORDINATE={},{},{}",
            coordinate[0], coordinate[1], coordinate[2]
        ),
        format!(
            "RUST_MCBE_WORLD_READY radius={} rendered={} resident={} visible={}",
            PHASE0_REQUESTED_RADIUS_CHUNKS,
            snapshot.rendered_sub_chunks,
            snapshot.resident_sub_chunks,
            snapshot.visible_sub_chunks,
        ),
    ])
}

fn camera_sub_chunk_key(dimension: i32, position: Vec3) -> SubChunkKey {
    SubChunkKey::new(
        dimension,
        (position.x.floor() as i32).div_euclid(16),
        (position.y.floor() as i32).div_euclid(16),
        (position.z.floor() as i32).div_euclid(16),
    )
}

fn frame_limited_winit_settings(frame_cap: Option<u32>) -> WinitSettings {
    let Some(frame_cap) = frame_cap else {
        return WinitSettings::continuous();
    };
    let mode = UpdateMode::Reactive {
        wait: Duration::from_secs_f64(1.0 / f64::from(frame_cap)),
        react_to_device_events: false,
        react_to_user_events: false,
        react_to_window_events: false,
    };
    WinitSettings {
        focused_mode: mode,
        unfocused_mode: mode,
    }
}

#[derive(Default)]
struct RollingFps {
    frame_times: VecDeque<Duration>,
    elapsed: Duration,
}

impl RollingFps {
    fn record(&mut self, frame_time: Duration) {
        if frame_time.is_zero() {
            return;
        }
        self.frame_times.push_back(frame_time);
        self.elapsed += frame_time;
        while self.elapsed > Duration::from_secs(1) {
            let Some(oldest) = self.frame_times.pop_front() else {
                break;
            };
            self.elapsed = self.elapsed.saturating_sub(oldest);
        }
    }

    fn value(&self) -> f64 {
        if self.elapsed.is_zero() {
            return 0.0;
        }
        self.frame_times.len() as f64 / self.elapsed.as_secs_f64()
    }
}

fn status_title(
    camera: &Transform,
    resident_sub_chunks: usize,
    visible_sub_chunks: usize,
    captured: bool,
    fps: f64,
) -> String {
    let (yaw, pitch, _) = camera.rotation.to_euler(EulerRot::YXZ);
    format!(
        "Rust MCBE | {fps:.1} FPS | pos {:.2} {:.2} {:.2} | yaw {yaw:.2} pitch {pitch:.2} | chunks {visible_sub_chunks}/{resident_sub_chunks} | {}",
        camera.translation.x,
        camera.translation.y,
        camera.translation.z,
        if captured { "captured" } else { "released" },
    )
}

fn bedrock_camera_rotation(yaw_degrees: f32, pitch_degrees: f32) -> Quat {
    Quat::from_euler(
        EulerRot::YXZ,
        (180.0 - yaw_degrees).to_radians(),
        -pitch_degrees.to_radians(),
        0.0,
    )
}

fn main() {
    match args::ClientArgs::parse_env() {
        Ok(args::ParseOutcome::Help) => print!("{}", args::HELP),
        Ok(args::ParseOutcome::Run(args)) => {
            if let Err(error) = run(*args) {
                eprintln!("bedrock-client failed: {error:#}");
                std::process::exit(1);
            }
        }
        Err(error) => {
            eprintln!("{error}");
            std::process::exit(2);
        }
    }
}

fn run(args: args::ClientArgs) -> Result<()> {
    let selected_assets = select_asset_path_from_environment(args.assets.as_deref());
    let loaded_assets =
        load_runtime_assets(selected_assets).context("load startup block assets")?;
    if let Some(notice) = &loaded_assets.notice {
        eprintln!("{notice}");
    } else if loaded_assets.kind == LoadedAssetKind::CompiledBlob {
        eprintln!(
            "loaded compiled block assets from {} (sha256 {})",
            loaded_assets.selected_path.display(),
            loaded_assets.metrics.blob_sha256
        );
    }
    let runtime_assets = loaded_assets.runtime;
    let asset_metrics = loaded_assets.metrics;

    let network = spawn_network(NetworkConfig {
        socket_dir: resolve_socket_dir(&args.socket_dir),
        display_name: args.display_name.clone(),
    })
    .context("spawn Bedrock network worker")?;
    let present_mode = if args.no_vsync {
        PresentMode::AutoNoVsync
    } else {
        PresentMode::AutoVsync
    };

    let mut app = App::new();
    app.add_plugins(DefaultPlugins.set(WindowPlugin {
        primary_window: Some(Window {
            title: "Rust MCBE | connecting".to_owned(),
            present_mode,
            ..default()
        }),
        ..default()
    }))
    .insert_resource(frame_limited_winit_settings(args.frame_cap))
    .insert_resource(ClearColor(Color::srgb(0.46, 0.70, 0.92)))
    .insert_resource(network)
    .insert_resource(ClientWorld::new(Arc::clone(&runtime_assets)))
    .insert_resource(startup_biome_tints(&runtime_assets))
    .insert_resource(ChunkTextureAssets::new(runtime_assets))
    .insert_resource(CaveVisibilityCache::default())
    .insert_resource(AppMetrics(MetricsCollector::with_asset_metrics(
        asset_metrics,
    )))
    .insert_resource(DiagnosticQuads::default())
    .insert_resource(TransparentWitnessFileSource::new(
        args.transparent_witness_request,
    ))
    .insert_resource(ModelWitnessFileSource::new(args.model_witness_request))
    .insert_resource(AcceptanceRun::new(
        args.acceptance_seconds,
        args.metrics_out,
        args.full_view_teleport_gate,
        args.require_transparent_presentation,
    ))
    .add_plugins((
        DebugWorldPlugin::new(GPU_UPLOAD_BUDGET_PER_FRAME),
        FlyCameraPlugin::new(args.auto_fly),
    ))
    .add_observer(apply_added_chunk_visibility)
    .add_observer(remove_chunk_visibility)
    .add_systems(
        Update,
        (
            receive_network_events,
            poll_transparent_witness_request,
            poll_model_witness_request,
            drive_world_stream,
            refresh_cave_visibility,
            emit_world_ready,
            drive_model_witness,
            record_metrics_and_title,
            finish_acceptance_run,
        )
            .chain(),
    );

    let exit = app.run();
    if let Some(mut network) = app.world_mut().remove_resource::<NetworkHandle>() {
        network.shutdown();
    }
    if exit.is_error() {
        bail!("Bevy app exited after a fatal runtime error");
    }
    Ok(())
}

fn resolve_socket_dir(path: &Path) -> PathBuf {
    let current_dir = std::env::current_dir().unwrap_or_else(|_| PathBuf::from("."));
    let executable = std::env::current_exe().unwrap_or_default();
    resolve_socket_dir_from(path, &current_dir, &executable)
}

fn resolve_socket_dir_from(path: &Path, current_dir: &Path, executable: &Path) -> PathBuf {
    if path.is_absolute() {
        return path.to_owned();
    }
    let current_candidate = current_dir.join(path);
    if bridge_endpoint_exists(&current_candidate) {
        return current_candidate;
    }
    let development_candidate = executable
        .parent()
        .and_then(Path::parent)
        .and_then(Path::parent)
        .map(|project_root| project_root.join(path));
    if let Some(candidate) = development_candidate
        && bridge_endpoint_exists(&candidate)
    {
        return candidate;
    }
    current_candidate
}

fn bridge_endpoint_exists(directory: &Path) -> bool {
    directory.join("game.addr").is_file() || directory.join("game.sock").exists()
}

fn record_fatal_error(fatal_error: &mut Option<String>, error: String) {
    if fatal_error.is_none() {
        *fatal_error = Some(error);
    }
}

fn receive_network_events(
    mut network: ResMut<NetworkHandle>,
    mut client_world: ResMut<ClientWorld>,
    mut acceptance: ResMut<AcceptanceRun>,
    metrics: Res<AppMetrics>,
    acknowledgements: Res<ChunkUploadAcknowledgements>,
    mut cameras: Query<&mut Transform, With<FlyCamera>>,
) {
    let controls =
        drain_network_controls(network.control_events_mut(), OUTBOUND_SEND_BUDGET_PER_FRAME);
    for control in controls {
        match control {
            NetworkControlEvent::Bootstrap(bootstrap) => {
                acknowledgements.clear();
                info!(
                    runtime_id = bootstrap.local_player_runtime_id,
                    position = ?bootstrap.player_position,
                    world_spawn = ?bootstrap.world_spawn_position,
                    "received StartGame bootstrap"
                );
                if acceptance.enabled() {
                    acceptance.set_mutation_surface_anchor([
                        bootstrap.world_spawn_position[0],
                        bootstrap.world_spawn_position[2],
                    ]);
                }
                let current = cameras
                    .single()
                    .map(|camera| camera.translation.to_array())
                    .unwrap_or([
                        bootstrap.world_spawn_position[0] as f32 + 0.5,
                        SAFE_SERVER_HEIGHT,
                        bootstrap.world_spawn_position[2] as f32 + 0.5,
                    ]);
                let stream = WorldStream::new_with_assets(
                    bootstrap,
                    Arc::clone(&client_world.runtime_assets),
                    current,
                    client_world.pending_surface_spawn,
                );
                let resolved = stream.resolved_server_position();
                if let Ok(mut camera) = cameras.single_mut() {
                    camera.translation = Vec3::from_array(resolved.position);
                }
                client_world.pending_surface_spawn = resolved.surface_anchor;
                client_world.stream = Some(stream);
            }
            NetworkControlEvent::SubChunkRequestSent {
                chunk,
                base_sub_chunk_y,
                count,
                sent_at,
            } => {
                if let Some(stream) = client_world.stream.as_mut() {
                    stream.acknowledge_sub_chunk_request_sent(
                        chunk,
                        base_sub_chunk_y,
                        count,
                        sent_at,
                    );
                }
            }
            NetworkControlEvent::Failed {
                message,
                decode_error_count,
            } => {
                error!(decode_error_count, "network session failed: {message}");
                client_world.network_decode_errors = decode_error_count;
                record_fatal_error(
                    &mut client_world.fatal_error,
                    format!("network session failed: {message}"),
                );
            }
            NetworkControlEvent::Stopped { decode_error_count } => {
                client_world.network_decode_errors = decode_error_count;
                if client_world.fatal_error.is_none() {
                    client_world.fatal_error = Some("network session stopped unexpectedly".into());
                }
            }
        }
    }

    let admission_capacity = client_world.stream.as_ref().map_or(
        NETWORK_INGRESS_BUDGET_PER_FRAME,
        WorldStream::remaining_admission_capacity,
    );
    let events = drain_network_ingress(
        network.world_events_mut(),
        NETWORK_INGRESS_BUDGET_PER_FRAME.min(admission_capacity),
    );
    for sequenced in events {
        let Some(stream) = client_world.stream.as_mut() else {
            client_world.fatal_error =
                Some("received world data before StartGame bootstrap".to_owned());
            continue;
        };
        let observed_at = Instant::now();
        acceptance.observe_mutation(&sequenced.event, observed_at);
        let accepted_binding_ingress = acceptance.observe_full_view_teleport_ingress(
            &sequenced.event,
            sequenced.sequence,
            observed_at,
            stream.current_dimension(),
            metrics.0.frame_count(),
        );
        if accepted_binding_ingress {
            if let Some(ingress_marker) = accepted_move_player_ingress_marker(
                accepted_binding_ingress,
                sequenced.sequence,
                &sequenced.event,
            ) {
                let mut stdout = std::io::stdout().lock();
                write_move_player_ingress_before_source_capture(
                    &mut stdout,
                    &ingress_marker,
                    || stream.schedule_source_capture(sequenced.sequence),
                );
            } else {
                stream.schedule_source_capture(sequenced.sequence);
            }
        }
        if let Err(error) = stream.submit(sequenced.sequence, sequenced.event) {
            client_world.fatal_error = Some(format!("world FIFO rejected data: {error}"));
        }
    }
}

fn drain_network_controls<T>(
    receiver: &mut tokio::sync::mpsc::Receiver<T>,
    budget: usize,
) -> Vec<T> {
    drain_network_ingress(receiver, budget)
}

fn drain_network_ingress<T>(
    receiver: &mut tokio::sync::mpsc::Receiver<T>,
    budget: usize,
) -> Vec<T> {
    std::iter::from_fn(|| receiver.try_recv().ok())
        .take(budget)
        .collect()
}

#[allow(clippy::too_many_arguments)]
fn drive_world_stream(
    network: Res<NetworkHandle>,
    mut client_world: ResMut<ClientWorld>,
    mut acceptance: ResMut<AcceptanceRun>,
    mut metrics: ResMut<AppMetrics>,
    mut render_queue: ResMut<ChunkRenderQueue>,
    mut biome_tints: ResMut<ChunkBiomeTints>,
    mut diagnostic_quads: ResMut<DiagnosticQuads>,
    acknowledgements: Res<ChunkUploadAcknowledgements>,
    mut camera: Query<&mut Transform, With<FlyCamera>>,
) {
    let Some(stream) = client_world.stream.as_mut() else {
        return;
    };
    synchronize_biome_tints(stream, &mut biome_tints);
    for acknowledgement in acknowledgements.drain() {
        render_queue.record_gpu_upload_bytes(acknowledgement.uploaded_bytes);
        if let Some(latency) = acceptance.acknowledge_mutation(
            acknowledgement.key,
            acknowledgement.token.generation,
            acknowledgement.token.dirty_since,
            acknowledgement.applied_at,
        ) {
            metrics.0.record_mutation_to_visible(latency);
        }
        stream.acknowledge_mesh_upload(
            acknowledgement.key,
            acknowledgement.token.generation,
            acknowledgement.token.dirty_since,
            acknowledgement.applied_at,
        );
    }
    let Ok(mut camera) = camera.single_mut() else {
        return;
    };
    let controls = {
        let stream = client_world
            .stream
            .as_mut()
            .expect("stream presence was checked before camera access");
        stream.poll(camera.translation.to_array(), MESH_JOB_BUDGET_PER_FRAME);
        stream.take_committed_controls()
    };
    for control in controls {
        let _ = acceptance.observe_committed_full_view_control(&control);
        apply_committed_control(
            control,
            &mut camera,
            &mut client_world.pending_surface_spawn,
        );
    }
    let camera_position = camera.translation;
    let resolved_surface_spawn = client_world.pending_surface_spawn.and_then(|anchor| {
        client_world
            .stream
            .as_ref()
            .and_then(|stream| stream.surface_eye_position(anchor[0], anchor[1]))
    });
    let resolved_mutation_coordinate = acceptance.mutation_surface_anchor().and_then(|anchor| {
        client_world.stream.as_ref().and_then(|stream| {
            stream
                .surface_eye_position(anchor[0], anchor[1])
                .map(|position| deterministic_mutation_coordinate(position, anchor))
        })
    });

    let send_error = client_world.stream.as_mut().and_then(|stream| {
        flush_sub_chunk_requests(
            stream,
            OUTBOUND_SEND_BUDGET_PER_FRAME,
            |chunk, base_sub_chunk_y, count, packet| {
                network.send_sub_chunk_request(chunk, base_sub_chunk_y, count, packet)
            },
        )
        .err()
    });
    if let Some(stream) = client_world.stream.as_mut() {
        while let Some(change) = stream.pop_mesh_change() {
            let retry = match change {
                WorldMeshChange::Upsert {
                    key,
                    mesh,
                    biome,
                    tint_identity,
                    generation,
                    dirty_since,
                } => {
                    let diagnostic_count = u64::try_from(
                        mesh.quads()
                            .iter()
                            .filter(|quad| quad.material_id() == DIAGNOSTIC_MATERIAL)
                            .count(),
                    )
                    .unwrap_or(u64::MAX);
                    match render_queue.try_update_tracked_with_biome_identity(
                        key,
                        mesh,
                        biome,
                        tint_identity,
                        ChunkUploadPriority::from_camera(key, camera_position),
                        ChunkUploadToken {
                            generation,
                            dirty_since,
                        },
                    ) {
                        Ok(()) => {
                            diagnostic_quads.0.upsert(key, diagnostic_count);
                            None
                        }
                        Err((mesh, biome)) => Some(WorldMeshChange::Upsert {
                            key,
                            mesh,
                            biome,
                            tint_identity,
                            generation,
                            dirty_since,
                        }),
                    }
                }
                WorldMeshChange::Remove {
                    key,
                    generation,
                    dirty_since,
                } => match render_queue.try_remove_tracked(
                    key,
                    ChunkUploadPriority::from_camera(key, camera_position),
                    ChunkUploadToken {
                        generation,
                        dirty_since,
                    },
                ) {
                    Ok(()) => {
                        diagnostic_quads.0.remove(key);
                        None
                    }
                    Err(key) => Some(WorldMeshChange::Remove {
                        key,
                        generation,
                        dirty_since,
                    }),
                },
            };
            let Some(retry) = retry else {
                continue;
            };
            if stream.retry_mesh_change_front(retry).is_err() {
                client_world.fatal_error = Some(
                    "failed to restore a render update to the bounded world retry FIFO".to_owned(),
                );
            }
            break;
        }
    }
    if let Some(error) = send_error {
        record_fatal_error(&mut client_world.fatal_error, error);
    }
    if let Some(position) = resolved_surface_spawn {
        camera.translation = Vec3::from_array(position);
        client_world.pending_surface_spawn = None;
        info!(position = ?position, "resolved temporary Bedrock spawn from packed terrain");
    }
    if let Some(coordinate) = resolved_mutation_coordinate {
        acceptance.set_mutation_coordinate(coordinate);
    }
}

#[derive(Default)]
struct ModelWitnessExpectationState {
    request: ModelWitnessRequest,
    expectation: Option<TargetRenderExpectation>,
    next_view_generation: u64,
}

fn drive_model_witness(
    client_world: Res<ClientWorld>,
    render_queue: Res<ChunkRenderQueue>,
    presented_frames: Res<PresentedFrameGate>,
    request: Res<ModelWitnessRequest>,
    evidence: Res<ModelWitnessEvidence>,
    mut state: Local<ModelWitnessExpectationState>,
) {
    if !request.enabled() {
        if state.request.enabled() {
            presented_frames.clear();
        }
        *state = ModelWitnessExpectationState::default();
        return;
    }
    if state.request != *request {
        presented_frames.clear();
        state.request = (*request).clone();
        state.expectation = None;
        state.next_view_generation = 0;
    }
    let Some(cohort) = client_world
        .stream
        .as_ref()
        .and_then(WorldStream::committed_view_cohort)
        .map(render_view_cohort)
    else {
        return;
    };
    let now = Instant::now();
    let Some(proposed) = render_queue.freeze_target_expectation_for_keys(
        cohort,
        None,
        request.keys().iter().copied(),
        0,
        now,
    ) else {
        if state.expectation.take().is_some() {
            presented_frames.clear();
        }
        return;
    };
    let expectation = if let Some(current) = state.expectation.as_ref().filter(|current| {
        current.cohort == proposed.cohort
            && current.source_cohort == proposed.source_cohort
            && current.manifest == proposed.manifest
    }) {
        current.clone()
    } else {
        state.next_view_generation = state.next_view_generation.wrapping_add(1).max(1);
        let mut next = proposed;
        next.view_generation = state.next_view_generation;
        state.expectation = Some(next.clone());
        next
    };
    presented_frames.set_expectation(expectation);
    for acknowledgement in presented_frames.drain() {
        evidence.observe_presented_frame(&request, &acknowledgement);
    }
}

#[allow(clippy::too_many_arguments)]
fn emit_world_ready(
    network: Res<NetworkHandle>,
    mut client_world: ResMut<ClientWorld>,
    cache: Res<CaveVisibilityCache>,
    diagnostic_quads: Res<DiagnosticQuads>,
    render_queue: Res<ChunkRenderQueue>,
    model_witness_source: Res<ModelWitnessFileSource>,
    acknowledgements: Res<ChunkUploadAcknowledgements>,
    presented_frames: Res<PresentedFrameGate>,
    mut acceptance: ResMut<AcceptanceRun>,
    mut auto_fly: ResMut<camera::AutoFly>,
    mut metrics: ResMut<AppMetrics>,
) {
    let missing_mapping_count = client_world.runtime_assets.missing_count();
    let Some(stream) = client_world.stream.as_mut() else {
        return;
    };
    let stats = stream.stats();
    let timeout_progress = SubChunkTimeoutProgress {
        awaiting_responses: stats.awaiting_sub_chunk_responses,
        timeouts: stats.sub_chunk_timeouts,
        retries_scheduled: stats.sub_chunk_retries_scheduled,
        retry_exhaustions: stats.sub_chunk_retry_exhaustions,
    };
    let work = WorldReadyWork {
        network_events: network.pending_event_count(),
        network_commands: network.pending_command_count(),
        admitted_world_events: stats.admitted_world_events,
        queued_decode_jobs: stats.queued_decode_jobs,
        in_flight_decode_jobs: stats.in_flight_decode_jobs,
        completed_decode_results: stats.completed_decode_results,
        pending_mesh_jobs: stats.pending_mesh_jobs,
        in_flight_mesh_jobs: stats.in_flight_mesh_jobs,
        pending_mesh_changes: stream.pending_mesh_change_count(),
        outbound_requests: stream.pending_request_work_count(),
        outstanding_sub_chunks: stream.outstanding_sub_chunk_count(),
        pending_retry_requests: stats.pending_retry_requests,
        render_queue_items: render_queue.retained_len(),
        pending_gpu_acknowledgements: usize::from(!acknowledgements.is_empty()),
        unacknowledged_meshes: stream.unacknowledged_mesh_count(),
    };
    if acceptance.world_ready {
        let cohort = acceptance
            .full_view_teleport
            .target_cohort()
            .map(|target| stream.cohort_status(target));
        if let Some(status) = cohort {
            debug_assert_eq!(stream.committed_view_cohort(), status.committed);
        }
        let snapshot = TeleportReadySnapshot {
            received_radius_chunks: stats.received_radius_chunks,
            publisher_radius_chunks: stats.publisher_radius_chunks,
            rendered_sub_chunks: cache.rendered.len(),
            resident_sub_chunks: stats.resident_sub_chunks,
            visible_sub_chunks: cache.visible_rendered,
            loaded_columns: stream.loaded_column_count(),
            cohort,
            last_chunk_commit_at: stats.last_chunk_commit_at,
            last_mesh_dispatch_at: stats.last_mesh_dispatch_at,
            last_mesh_completion_at: stats.last_mesh_completion_at,
            last_mesh_ack_at: stats.last_mesh_ack_at,
            work,
        };
        let observed_at = Instant::now();
        let frame_count = metrics.0.frame_count();
        acceptance.full_view_teleport.note_frame(frame_count);
        let teleport = if let Some(pending) = acceptance.full_view_teleport.pending.as_ref() {
            let proposed = render_queue.freeze_target_expectation(
                render_view_cohort(pending.target),
                Some(render_view_cohort(pending.source)),
                0,
                observed_at,
            );
            let expectation = acceptance
                .full_view_teleport
                .reconcile_presented_expectation(snapshot, proposed, observed_at);
            if let Some(expectation) = expectation {
                presented_frames.set_expectation(expectation);
            } else {
                presented_frames.clear();
            }
            presented_frames
                .drain()
                .into_iter()
                .find_map(|acknowledgement| {
                    acceptance
                        .full_view_teleport
                        .observe_presented_frame(acknowledgement)
                })
        } else {
            None
        };
        if let Some(teleport) = teleport {
            presented_frames.clear();
            let cohort = snapshot
                .cohort
                .expect("teleport completion requires an exact cohort");
            let remesh_manifest = stream.remesh_all_resident(Instant::now());
            if !acceptance.full_view_remesh.start(
                Some(&teleport),
                cohort,
                remesh_manifest,
                frame_count,
            ) {
                error!("could not start exact full-view remesh gate after binding teleport");
                return;
            }
            let proof = teleport_proof(cohort, &teleport);
            metrics.0.record_teleport_proof(proof.clone());
            let mut stdout = std::io::stdout().lock();
            let _ = writeln!(stdout, "{}", teleport_settled_marker(&proof));
            let _ = writeln!(
                stdout,
                "{}",
                teleport_global_stage_diagnostic_marker(cohort.target, &teleport)
            );
            let _ = stdout.flush();
            return;
        }

        if let Some(status) = snapshot.cohort
            && let Some(marker) = acceptance.full_view_teleport.cohort_progress_line(
                status,
                snapshot.work,
                timeout_progress,
                observed_at,
            )
        {
            let mut stdout = std::io::stdout().lock();
            let _ = writeln!(stdout, "{marker}");
            let _ = stdout.flush();
        }

        let remesh_manifest = acceptance
            .full_view_remesh
            .pending
            .as_ref()
            .map(|pending| pending.manifest.clone());
        if let Some(remesh_manifest) = remesh_manifest {
            let manifest_state = stream.forced_remesh_manifest_state(&remesh_manifest);
            let proposal_target = acceptance.full_view_remesh.pending.as_ref().map(|pending| {
                (
                    render_view_cohort(pending.cohort.target),
                    pending.source_cohort,
                )
            });
            let proposed = if manifest_state == world_stream::ForcedRemeshManifestState::Complete {
                proposal_target.map(|(target, source)| {
                    render_queue.freeze_target_expectation(target, source, 0, observed_at)
                })
            } else {
                None
            };
            let expectation = acceptance.full_view_remesh.reconcile_presented_expectation(
                snapshot,
                manifest_state,
                proposed,
                observed_at,
                frame_count,
            );
            if let Some(expectation) = expectation {
                presented_frames.set_expectation(expectation);
            } else {
                presented_frames.clear();
            }
            let completion = presented_frames
                .drain()
                .into_iter()
                .find_map(|acknowledgement| {
                    acceptance
                        .full_view_remesh
                        .observe_presented_frame(acknowledgement, frame_count)
                });
            if let Some(completion) = completion {
                presented_frames.clear();
                let cohort = snapshot
                    .cohort
                    .expect("forced remesh completion requires the frozen exact cohort");
                let proof = forced_remesh_proof(cohort, &completion);
                metrics
                    .0
                    .record_forced_full_view_remesh_proof(proof.clone());
                let Some(target) = acceptance.full_view_teleport.completed_target_mutation else {
                    error!("forced remesh completed without deterministic mutation coordinates");
                    return;
                };
                if !acceptance.retarget_mutation(target, completion.stable_frame.gpu_completed_at) {
                    error!(
                        ?target,
                        "could not arm target-only mutation after forced remesh"
                    );
                    return;
                }
                let Some(mutation_marker) = acceptance.target_mutation_marker() else {
                    error!("target mutation armed without complete manifest-comparable evidence");
                    return;
                };
                let mut stdout = std::io::stdout().lock();
                let _ = writeln!(stdout, "{}", forced_remesh_settled_marker(&proof));
                let _ = writeln!(stdout, "{mutation_marker}");
                let _ = stdout.flush();
            }
        }

        let completed_remesh_target =
            acceptance
                .full_view_remesh
                .completed
                .as_ref()
                .map(|completion| {
                    (
                        completion.expectation.cohort,
                        completion.expectation.source_cohort,
                    )
                });
        if let Some((target, source)) = completed_remesh_target {
            let proposed = render_queue.freeze_target_expectation(target, source, 0, observed_at);
            let expectation =
                acceptance.reconcile_mutation_presented_expectation(proposed, observed_at);
            if let Some(expectation) = expectation {
                presented_frames.set_expectation(expectation);
                if let Some(latency) =
                    presented_frames
                        .drain()
                        .into_iter()
                        .find_map(|acknowledgement| {
                            acceptance.observe_presented_mutation(acknowledgement)
                        })
                {
                    presented_frames.clear();
                    metrics.0.record_mutation_to_visible(latency);
                }
            } else {
                presented_frames.clear();
            }
        }
        return;
    }
    let mutation_coordinate = acceptance.mutation_coordinate();
    let mutation_target = mutation_coordinate.map(|coordinate| {
        SubChunkKey::new(
            stream.current_dimension(),
            coordinate[0].div_euclid(16),
            coordinate[1].div_euclid(16),
            coordinate[2].div_euclid(16),
        )
    });
    let snapshot = WorldReadySnapshot {
        mutation_coordinate,
        received_radius_chunks: stats.received_radius_chunks,
        publisher_radius_chunks: stats.publisher_radius_chunks,
        rendered_sub_chunks: cache.rendered.len(),
        resident_sub_chunks: stats.resident_sub_chunks,
        visible_sub_chunks: cache.visible_rendered,
        mutation_target_rendered: mutation_target
            .is_some_and(|target| cache.rendered.contains(&target)),
        mutation_target_visible: mutation_target.is_some_and(|target| cache.is_visible(target)),
        mutation_target_clean: mutation_target.is_some_and(|target| stream.is_mesh_clean(target)),
        work,
    };
    let ready_at = Instant::now();
    if let Some(marker) = acceptance
        .gallery_anchor
        .observe(model_witness_source.configured(), snapshot)
    {
        let mut stdout = std::io::stdout().lock();
        write_stdout_marker(&mut stdout, &marker);
    }
    let Some(markers) = acceptance.world_ready_settler.observe(snapshot, ready_at) else {
        return;
    };
    metrics
        .0
        .record_asset_counters(missing_mapping_count, diagnostic_quads.0.total());
    let asset_marker = metrics
        .0
        .asset_metrics()
        .world_ready_marker(snapshot.resident_sub_chunks, snapshot.visible_sub_chunks);
    let coordinate = snapshot
        .mutation_coordinate
        .expect("world-ready markers require a mutation coordinate");
    auto_fly.set_look_target(Vec3::new(
        coordinate[0] as f32 + 0.5,
        coordinate[1] as f32 + 0.5,
        coordinate[2] as f32 + 0.5,
    ));
    let mut stdout = std::io::stdout().lock();
    for marker in markers {
        let _ = writeln!(stdout, "{marker}");
    }
    let _ = writeln!(stdout, "{asset_marker}");
    let _ = stdout.flush();
    stream.begin_timed_session();
    metrics.0.begin_timed_session(ready_at);
    acceptance.begin_world_ready(
        ready_at,
        stream.resolved_server_position().position,
        stream.local_player_runtime_id(),
    );
}

fn flush_sub_chunk_requests(
    stream: &mut WorldStream,
    budget: usize,
    mut send: impl FnMut(
        world::ChunkKey,
        i32,
        usize,
        protocol::Packet,
    ) -> Result<(), network::PacketSendError>,
) -> Result<usize, String> {
    let mut sent = 0;
    for _ in 0..budget {
        let Some(request) = stream.pop_next_request() else {
            break;
        };
        let world_stream::PendingSubChunkRequest {
            packet,
            dimension,
            chunk,
            base_sub_chunk_y,
            count,
        } = request;
        match send(chunk, base_sub_chunk_y, count, packet) {
            Ok(()) => {
                stream.record_sub_chunk_request_transport_pending(chunk, base_sub_chunk_y, count);
                debug!(
                    dimension,
                    chunk_x = chunk.x,
                    chunk_z = chunk.z,
                    base_sub_chunk_y,
                    count,
                    "requested streamed sub-chunk column"
                );
                sent += 1;
            }
            Err(error) => {
                let closed = error.is_closed();
                let retry = world_stream::PendingSubChunkRequest {
                    packet: error.into_packet(),
                    dimension,
                    chunk,
                    base_sub_chunk_y,
                    count,
                };
                if stream.retry_request_front(retry).is_err() {
                    return Err(
                        "failed to restore an unsent SubChunkRequest to the bounded FIFO"
                            .to_owned(),
                    );
                }
                if closed {
                    return Err(
                        "failed to send SubChunkRequest: network command channel is closed"
                            .to_owned(),
                    );
                }
                break;
            }
        }
    }
    Ok(sent)
}

fn apply_committed_control(
    control: CommittedControlEvent,
    camera: &mut Transform,
    pending_surface_spawn: &mut Option<[i32; 2]>,
) {
    let resolved = match control {
        CommittedControlEvent::MovePlayer {
            movement, resolved, ..
        } => {
            info!(
                runtime_id = movement.runtime_id,
                position = ?movement.position,
                "applying committed local MovePlayer"
            );
            if movement.yaw.is_finite() && movement.pitch.is_finite() {
                camera.rotation = bedrock_camera_rotation(movement.yaw, movement.pitch);
            }
            resolved
        }
        CommittedControlEvent::ChangeDimension { resolved, .. } => resolved,
    };
    camera.translation = Vec3::from_array(resolved.position);
    *pending_surface_spawn = resolved.surface_anchor;
}

fn refresh_cave_visibility(
    client_world: Res<ClientWorld>,
    camera: Query<&Transform, With<FlyCamera>>,
    mut cache: ResMut<CaveVisibilityCache>,
    mut chunks: Query<(&ChunkRenderInstance, &mut Visibility)>,
) {
    let (Some(stream), Ok(camera)) = (client_world.stream.as_ref(), camera.single()) else {
        return;
    };
    let camera_key = camera_sub_chunk_key(stream.current_dimension(), camera.translation);
    let generation = stream.connectivity_generation();
    if cache.camera == Some(camera_key)
        && cache.graph_generation == Some(generation)
        && cache.initialized
    {
        return;
    }

    cache.visible = stream.cave_visible_sub_chunks(camera_key);
    cache.camera = Some(camera_key);
    cache.graph_generation = Some(generation);
    cache.initialized = true;
    cache.rendered.clear();
    cache.visible_rendered = 0;
    for (instance, mut visibility) in &mut chunks {
        let key = instance.key();
        cache.rendered.insert(key);
        let is_visible = cache.visible.contains(&key);
        *visibility = if is_visible {
            Visibility::Inherited
        } else {
            Visibility::Hidden
        };
        cache.visible_rendered += usize::from(is_visible);
    }
}

fn apply_added_chunk_visibility(
    add: On<Add, ChunkRenderInstance>,
    mut cache: ResMut<CaveVisibilityCache>,
    mut chunks: Query<(&ChunkRenderInstance, &mut Visibility)>,
) {
    let Ok((instance, mut visibility)) = chunks.get_mut(add.entity) else {
        return;
    };
    let key = instance.key();
    let is_visible = cache.is_visible(key);
    *visibility = if is_visible {
        Visibility::Inherited
    } else {
        Visibility::Hidden
    };
    if cache.rendered.insert(key) && is_visible {
        cache.visible_rendered += 1;
    }
}

fn remove_chunk_visibility(
    remove: On<Remove, ChunkRenderInstance>,
    mut cache: ResMut<CaveVisibilityCache>,
    chunks: Query<&ChunkRenderInstance>,
) {
    let Ok(instance) = chunks.get(remove.entity) else {
        return;
    };
    let key = instance.key();
    if cache.rendered.remove(&key) && cache.is_visible(key) {
        cache.visible_rendered = cache.visible_rendered.saturating_sub(1);
    }
}

fn lower_hex(bytes: &[u8]) -> String {
    bytes.iter().map(|byte| format!("{byte:02x}")).collect()
}

fn model_witness_manifest_hash(records: &[ModelWitnessManifestRecord]) -> String {
    let mut hasher = Sha256::new();
    for record in records {
        hasher.update(record.key.dimension.to_le_bytes());
        hasher.update(record.key.x.to_le_bytes());
        hasher.update(record.key.y.to_le_bytes());
        hasher.update(record.key.z.to_le_bytes());
        hasher.update(record.generation.to_le_bytes());
        hasher.update((record.model_ref_count as u64).to_le_bytes());
    }
    lower_hex(&hasher.finalize())
}

#[allow(clippy::too_many_arguments)]
fn record_metrics_and_title(
    time: Res<Time>,
    mut client_world: ResMut<ClientWorld>,
    acceptance: Res<AcceptanceRun>,
    cache: Res<CaveVisibilityCache>,
    mut metrics: ResMut<AppMetrics>,
    diagnostic_quads: Res<DiagnosticQuads>,
    render_queue: Res<ChunkRenderQueue>,
    transparent_sort: Res<TransparentSortMetrics>,
    transparent_witness: Res<TransparentWitnessEvidence>,
    model_witness: Res<ModelWitnessEvidence>,
    chunks: Query<&ChunkRenderInstance>,
    camera: Query<&Transform, With<FlyCamera>>,
    mut window: Query<(&mut Window, &CursorOptions), With<PrimaryWindow>>,
    mut title_elapsed: Local<Duration>,
    mut rolling_fps: Local<RollingFps>,
    mut last_marked_transparent_sort_generation: Local<u64>,
) {
    let now = Instant::now();
    if let Some(deadline) = acceptance.deadline.filter(|deadline| now >= *deadline) {
        metrics.0.finish_timed_session(deadline);
    }
    let frame_time = time.delta();
    metrics.0.record_frame(frame_time);
    rolling_fps.record(frame_time);
    metrics.0.record_asset_counters(
        client_world.runtime_assets.missing_count(),
        diagnostic_quads.0.total(),
    );
    let transparent_sort_snapshot =
        TransparentSortMetricsSnapshot::from(transparent_sort.snapshot());
    if let Some(marker) = transparent_sort_committed_marker(
        *last_marked_transparent_sort_generation,
        transparent_sort_snapshot,
    ) {
        let mut stdout = std::io::stdout().lock();
        write_stdout_marker(&mut stdout, &marker);
        *last_marked_transparent_sort_generation = transparent_sort_snapshot.presented_generation;
    }
    for event in transparent_witness.drain_events() {
        let marker = format!(
            "RUST_MCBE_TRANSPARENT_WITNESS_COMPLETE revision={} sequence={} generation={} key_count={} consecutive={}",
            event.revision, event.sequence, event.generation, event.key_count, event.consecutive,
        );
        let mut stdout = std::io::stdout().lock();
        write_stdout_marker(&mut stdout, &marker);
    }
    for event in model_witness.drain_events() {
        let acknowledgement = &event.acknowledgement;
        let marker = format!(
            "RUST_MCBE_MODEL_WITNESS_COMPLETE revision={} request_sha256={} sequence={} view_generation={} key_count={} model_ref_count={} manifest_count={} manifest_sha256={} missing={} stale={} wrong_stream={} zero_ref={} draw_mismatch={} consecutive={}",
            acknowledgement.revision,
            lower_hex(&acknowledgement.request_hash),
            acknowledgement.frame_sequence,
            acknowledgement.view_generation,
            acknowledgement.manifest.len(),
            acknowledgement.total_model_ref_count,
            acknowledgement.manifest.len(),
            model_witness_manifest_hash(&acknowledgement.manifest),
            acknowledgement.missing_key_count,
            acknowledgement.stale_generation_count,
            acknowledgement.wrong_stream_count,
            acknowledgement.zero_model_ref_count,
            acknowledgement.draw_mismatch_count,
            event.consecutive,
        );
        let mut stdout = std::io::stdout().lock();
        write_stdout_marker(&mut stdout, &marker);
    }
    for event in transparent_witness.drain_incomplete_events() {
        let missing = event
            .missing_keys
            .iter()
            .map(|key| format!("{},{},{},{}", key.dimension, key.x, key.y, key.z))
            .collect::<Vec<_>>()
            .join(";");
        let marker = format!(
            "RUST_MCBE_TRANSPARENT_WITNESS_INCOMPLETE revision={} sequence={} generation={} missing_count={} missing={missing}",
            event.revision,
            event.sequence,
            event.generation,
            event.missing_keys.len(),
        );
        let mut stdout = std::io::stdout().lock();
        write_stdout_marker(&mut stdout, &marker);
    }
    for event in transparent_witness.drain_stage_events() {
        let records = event
            .records
            .iter()
            .map(|record| {
                let app_entity = chunks.iter().any(|instance| instance.key() == record.key);
                format!(
                    "{},{},{},{}:app_entity={}:cave_visible={}:extracted_visible={}:instance={}:liquid_quads={}:instance_generation={}:allocation={}:liquid_range={}:lighting_range={}:allocation_matches={}:committed_member={}",
                    record.key.dimension,
                    record.key.x,
                    record.key.y,
                    record.key.z,
                    u8::from(app_entity),
                    u8::from(cache.visible.contains(&record.key)),
                    u8::from(record.extracted_visible),
                    u8::from(record.instance_present),
                    record.liquid_quad_count,
                    record.instance_generation,
                    u8::from(record.allocation_present),
                    record.liquid_range_len,
                    record.lighting_range_len,
                    u8::from(record.allocation_matches),
                    u8::from(record.committed_member),
                )
            })
            .collect::<Vec<_>>()
            .join(";");
        let marker = format!(
            "RUST_MCBE_TRANSPARENT_WITNESS_STAGE revision={} committed_generation={} records={records}",
            event.revision, event.committed_generation,
        );
        let mut stdout = std::io::stdout().lock();
        write_stdout_marker(&mut stdout, &marker);
    }
    let stream_errors = client_world.stream.as_ref().map_or(0, |stream| {
        let stats = stream.stats();
        metrics.0.record_pipeline_snapshot(PipelineMetricsSnapshot {
            world_ready: acceptance.world_ready,
            requested_radius_chunks: PHASE0_REQUESTED_RADIUS_CHUNKS,
            received_radius_chunks: stats.received_radius_chunks,
            publisher_radius_chunks: stats.publisher_radius_chunks,
            mutation_coordinate: acceptance.mutation_coordinate(),
            visible_mutation_count: acceptance.visible_mutation_count(),
            max_decode: stats.max_decode_duration,
            max_mesh: stats.max_mesh_duration,
            max_remesh: stats.max_remesh_latency,
            rendered_sub_chunks: cache.rendered.len(),
            resident_sub_chunks: stats.resident_sub_chunks,
            visible_sub_chunks: cache.visible_rendered,
            admitted_world_events: stats.admitted_world_events,
            admitted_heavy_events: stats.admitted_heavy_events,
            queued_decode_jobs: stats.queued_decode_jobs,
            in_flight_decode_jobs: stats.in_flight_decode_jobs,
            completed_decode_results: stats.completed_decode_results,
            pending_retry_requests: stats.pending_retry_requests,
            outbound_requests: stream.pending_request_count(),
            pending_mesh_jobs: stats.pending_mesh_jobs,
            in_flight_mesh_jobs: stats.in_flight_mesh_jobs,
            gpu_upload_bytes: render_queue.gpu_upload_bytes(),
            transparent_sort: transparent_sort_snapshot,
        });
        stats
            .decode_errors
            .saturating_add(stats.normalization_errors)
    });
    let total_errors = client_world
        .network_decode_errors
        .saturating_add(stream_errors);
    if total_errors != client_world.reported_decode_errors {
        let (world_decode_errors, world_normalization_errors, normalization_reasons) =
            client_world.stream.as_ref().map_or_else(
                || (0, 0, Default::default()),
                |stream| {
                    let stats = stream.stats();
                    (
                        stats.decode_errors,
                        stats.normalization_errors,
                        stats.normalization_reasons,
                    )
                },
            );
        let normalization_reason_total = normalization_reasons.total();
        eprintln!(
            "RUST_MCBE_ERROR_COUNTERS network={} world_decode={} world_normalization={} reason_total={} reasons={normalization_reasons:?}",
            client_world.network_decode_errors,
            world_decode_errors,
            world_normalization_errors,
            normalization_reason_total,
        );
    }
    let error_delta = cumulative_counter_delta(total_errors, client_world.reported_decode_errors);
    metrics.0.add_decode_errors(error_delta);
    client_world.reported_decode_errors = total_errors;

    *title_elapsed += time.delta();
    if *title_elapsed < TITLE_REFRESH_INTERVAL {
        return;
    }
    *title_elapsed = Duration::ZERO;
    let (Ok(camera), Ok((mut window, cursor))) = (camera.single(), window.single_mut()) else {
        return;
    };
    let resident = client_world
        .stream
        .as_ref()
        .map_or(0, |stream| stream.stats().resident_sub_chunks);
    let mut title = status_title(
        camera,
        resident,
        cache.visible_rendered,
        camera::input_is_active(&window, cursor),
        rolling_fps.value(),
    );
    if let Some(error) = &client_world.fatal_error {
        title.push_str(" | ERROR: ");
        title.push_str(error);
    }
    window.title = title;
}

fn transparent_sort_committed_marker(
    last_presented_generation: u64,
    snapshot: TransparentSortMetricsSnapshot,
) -> Option<String> {
    (snapshot.presented_generation > last_presented_generation
        && snapshot.presented_generation == snapshot.committed_generation
        && snapshot.ref_count > 0)
        .then(|| {
            format!(
                "RUST_MCBE_TRANSPARENT_SORT_COMMITTED generation={} ref_count={}",
                snapshot.presented_generation, snapshot.ref_count
            )
        })
}

fn finish_acceptance_run(
    mut acceptance: ResMut<AcceptanceRun>,
    client_world: Res<ClientWorld>,
    mut metrics: ResMut<AppMetrics>,
    transparent_sort: Res<TransparentSortMetrics>,
    mut network: ResMut<NetworkHandle>,
    mut exit: MessageWriter<AppExit>,
) {
    if acceptance.finished {
        return;
    }
    let now = Instant::now();
    let fatal = client_world.fatal_error.is_some();
    if let Some(deadline) = acceptance.deadline.filter(|deadline| now >= *deadline) {
        metrics.0.finish_timed_session(deadline);
    }
    let transparent_snapshot = TransparentSortMetricsSnapshot::from(transparent_sort.snapshot());
    let decision = acceptance.exit_decision(now, fatal, transparent_snapshot);
    if matches!(
        decision,
        AcceptanceExitDecision::Continue | AcceptanceExitDecision::WaitForTransparentPresentation
    ) {
        return;
    }

    acceptance.finished = true;
    metrics
        .0
        .record_transparent_sort_snapshot(transparent_snapshot);
    let mut output_failed = false;
    if let Some(path) = &acceptance.metrics_out
        && let Err(error) = metrics.0.report().write_json(path)
    {
        error!(
            "failed to write acceptance metrics to {}: {error}",
            path.display()
        );
        output_failed = true;
    }
    if let Some(error) = &client_world.fatal_error {
        error!("{error}");
    }
    if decision == AcceptanceExitDecision::TransparentPresentationTimedOut {
        error!(
            "transparent presentation did not settle within {:.3}s after the timed session: committed={} encoded={} presented={} ref_count={}",
            TRANSPARENT_PRESENTATION_EXIT_GRACE.as_secs_f64(),
            transparent_snapshot.committed_generation,
            transparent_snapshot.encoded_generation,
            transparent_snapshot.presented_generation,
            transparent_snapshot.ref_count,
        );
    }
    network.shutdown();
    exit.write(if decision.is_error() || output_failed {
        AppExit::error()
    } else {
        AppExit::Success
    });
}

fn cumulative_counter_delta(current: u64, previous: u64) -> u64 {
    current.checked_sub(previous).unwrap_or(current)
}

#[cfg(test)]
mod tests {
    use assets::RuntimeAssets;
    use bevy::prelude::{Quat, Transform, Vec3};
    use protocol::{
        BiomeDefinitionEvent, BiomeDefinitionsEvent, BlockUpdateEvent, LevelChunkEvent,
        LevelChunkMode, SubChunkBatchEvent, SubChunkEntryEvent, SubChunkResult, WorldBootstrap,
        WorldEvent,
    };
    use render::{ChunkBiomeTints, PresentedFrameAck, RenderViewCohort, TargetRenderExpectation};
    use std::{
        sync::Arc,
        time::{Duration, Instant},
    };
    use world::{ChunkKey, SubChunkKey};

    use crate::metrics::TransparentSortMetricsSnapshot;
    use crate::network::{NetworkControlEvent, SequencedWorldEvent};
    use crate::world_stream::{
        ForcedRemeshManifest, ForcedRemeshManifestState, ViewCohort, ViewCohortStatus, WorldStream,
    };
    use crate::{
        AcceptanceExitDecision, AcceptanceRun, FullViewRemeshTracker, FullViewTeleportCompletion,
        FullViewTeleportTracker, GalleryAnchorEmitter, MutationTracker,
        NETWORK_INGRESS_BUDGET_PER_FRAME, OUTBOUND_SEND_BUDGET_PER_FRAME, RollingFps,
        SubChunkTimeoutProgress, TRANSPARENT_PRESENTATION_EXIT_GRACE, TeleportReadySnapshot,
        WORLD_READY_QUIET_INTERVAL, WorldReadySettler, WorldReadySnapshot, WorldReadyWork,
        accepted_move_player_ingress_marker, bedrock_camera_rotation, camera_sub_chunk_key,
        cumulative_counter_delta, deterministic_mutation_coordinate, drain_network_controls,
        drain_network_ingress, flush_sub_chunk_requests, leaf_forest_target_mutation_coordinate,
        record_fatal_error, resolve_socket_dir_from, startup_biome_tints, status_title,
        synchronize_biome_tints, target_mutation_armed_marker, teleport_proof,
        transparent_sort_committed_marker, world_ready_markers,
        write_move_player_ingress_before_source_capture, write_stdout_marker,
    };

    fn overworld_biome_payload() -> Vec<u8> {
        let mut payload = vec![1, 2];
        payload.extend(std::iter::repeat_n(0xff, 23));
        payload.push(0);
        payload
    }

    fn complete_world_stream_decodes(stream: &mut WorldStream) {
        for _ in 0..10_000 {
            stream.poll([0.0; 3], 0);
            let stats = stream.stats();
            if stats.queued_decode_jobs == 0
                && stats.in_flight_decode_jobs == 0
                && stats.completed_decode_results == 0
            {
                return;
            }
            std::thread::yield_now();
        }
        panic!("world stream decode did not complete");
    }

    #[test]
    fn compiled_and_live_biome_tables_preserve_raw_id_water_colour_parity() {
        let runtime_assets = Arc::new(RuntimeAssets::diagnostic());
        let mut active = startup_biome_tints(&runtime_assets);
        let initial = runtime_assets.biome_assets().resolve_live(&[]).unwrap();
        assert_eq!(active.entries().len(), initial.records.len());
        assert_eq!(active.revision(), 0);

        let mut stream = WorldStream::new_with_assets(
            WorldBootstrap {
                dimension: 0,
                local_player_runtime_id: 1,
                player_position: [0.0; 3],
                world_spawn_position: [0; 3],
                air_network_id: 12_530,
                block_network_ids_are_hashes: false,
            },
            runtime_assets,
            [0.0, 96.0, 0.0],
            None,
        );
        stream
            .submit(
                1,
                WorldEvent::BiomeDefinitions(BiomeDefinitionsEvent {
                    definitions: Arc::from([
                        BiomeDefinitionEvent {
                            biome_id: Some(42),
                            name: Arc::from("example:cool_water"),
                            temperature: 0.8,
                            downfall: 0.4,
                            snow_foliage: 0.0,
                            map_water_color: 0xff44_6688,
                        },
                        BiomeDefinitionEvent {
                            biome_id: Some(43),
                            name: Arc::from("example:warm_water"),
                            temperature: 0.8,
                            downfall: 0.4,
                            snow_foliage: 0.0,
                            map_water_color: 0xffaa_3300,
                        },
                    ]),
                }),
            )
            .unwrap();

        assert!(synchronize_biome_tints(&stream, &mut active));
        assert_eq!(active.revision(), stream.biome_tint_revision());
        assert_eq!(active.entries().len(), 3);
        let resolved = stream.resolved_biome_tints_snapshot();
        let cool = usize::try_from(resolved.dense_index(42)).unwrap();
        let warm = usize::try_from(resolved.dense_index(43)).unwrap();
        assert_ne!(cool, warm);
        assert_eq!(
            active.entries()[cool].water,
            resolved.records[cool].water[..3]
        );
        assert_eq!(
            active.entries()[warm].water,
            resolved.records[warm].water[..3]
        );
        assert_ne!(active.entries()[cool].water, active.entries()[warm].water);
        assert!(!synchronize_biome_tints(&stream, &mut active));
    }

    #[test]
    fn equal_numeric_revisions_from_different_streams_replace_the_active_table() {
        fn stream_with_live_temperature(
            runtime_assets: Arc<RuntimeAssets>,
            temperature: f32,
        ) -> WorldStream {
            let mut stream = WorldStream::new_with_assets(
                WorldBootstrap {
                    dimension: 0,
                    local_player_runtime_id: 1,
                    player_position: [0.0; 3],
                    world_spawn_position: [0; 3],
                    air_network_id: 12_530,
                    block_network_ids_are_hashes: false,
                },
                runtime_assets,
                [0.0, 96.0, 0.0],
                None,
            );
            stream
                .submit(
                    1,
                    WorldEvent::BiomeDefinitions(BiomeDefinitionsEvent {
                        definitions: Arc::from([BiomeDefinitionEvent {
                            biome_id: Some(42),
                            name: Arc::from("example:live"),
                            temperature,
                            downfall: 0.4,
                            snow_foliage: 0.0,
                            map_water_color: if temperature > 0.5 {
                                0xff11_2233
                            } else {
                                0xffaa_bbcc
                            },
                        }]),
                    }),
                )
                .unwrap();
            stream
        }

        let runtime_assets = Arc::new(RuntimeAssets::diagnostic());
        let first = stream_with_live_temperature(Arc::clone(&runtime_assets), 0.8);
        let second = stream_with_live_temperature(runtime_assets, 0.2);
        assert_eq!(first.biome_tint_revision(), second.biome_tint_revision());

        let mut active = ChunkBiomeTints::default();
        assert!(synchronize_biome_tints(&first, &mut active));
        let first_entries = active.entries().to_vec();
        assert!(synchronize_biome_tints(&second, &mut active));
        assert_ne!(active.entries(), first_entries);
    }

    fn settled_world_snapshot() -> WorldReadySnapshot {
        WorldReadySnapshot {
            mutation_coordinate: Some([14, 71, -6]),
            received_radius_chunks: Some(16),
            publisher_radius_chunks: Some(16),
            rendered_sub_chunks: 2,
            resident_sub_chunks: 3,
            visible_sub_chunks: 1,
            mutation_target_rendered: true,
            mutation_target_visible: true,
            mutation_target_clean: true,
            work: WorldReadyWork::default(),
        }
    }

    #[test]
    fn acceptance_run_retains_the_spawn_surface_anchor_until_coordinate_resolution() {
        let mut acceptance = AcceptanceRun::new(Some(900), None, false, false);
        assert!(acceptance.enabled());
        acceptance.set_mutation_surface_anchor([10, -6]);
        assert_eq!(acceptance.mutation_surface_anchor(), Some([10, -6]));
        acceptance.set_mutation_coordinate([14, 71, -6]);
        assert_eq!(acceptance.mutation_surface_anchor(), None);
        assert_eq!(acceptance.mutation_coordinate(), Some([14, 71, -6]));
    }

    #[test]
    fn full_view_move_player_ingress_marker_is_exact_and_nonbinding_events_are_silent() {
        let started = Instant::now();
        let mut acceptance = AcceptanceRun::new(Some(900), None, true, false);
        acceptance.set_mutation_coordinate([0, 58, 0]);
        acceptance.begin_world_ready(started, [0.5, 70.0, 0.5], 1);

        let publisher = WorldEvent::PublisherUpdate(protocol::PublisherUpdateEvent {
            center: [0, 70, 0],
            radius_blocks: 256,
        });
        let accepted =
            acceptance.observe_full_view_teleport_ingress(&publisher, 40, started, 0, 10);
        assert!(!accepted);
        assert_eq!(
            accepted_move_player_ingress_marker(accepted, 40, &publisher),
            None,
        );

        let near = WorldEvent::MovePlayer(protocol::MovePlayerEvent {
            runtime_id: 1,
            position: [16.5, 70.0, 0.5],
            pitch: 0.0,
            yaw: 0.0,
        });
        let accepted = acceptance.observe_full_view_teleport_ingress(
            &near,
            41,
            started + Duration::from_millis(1),
            0,
            11,
        );
        assert!(!accepted);
        assert_eq!(
            accepted_move_player_ingress_marker(accepted, 41, &near),
            None,
        );

        let binding = WorldEvent::MovePlayer(protocol::MovePlayerEvent {
            runtime_id: 1,
            position: [1_040.5, 93.75, 1_040.5],
            pitch: 0.0,
            yaw: 0.0,
        });
        let accepted = acceptance.observe_full_view_teleport_ingress(
            &binding,
            42,
            started + Duration::from_millis(2),
            0,
            12,
        );
        assert!(accepted);
        assert_eq!(
            accepted_move_player_ingress_marker(accepted, 42, &binding),
            Some(
                "RUST_MCBE_MOVE_PLAYER_INGRESS sequence=42 position=1040.5,93.75,1040.5".to_owned(),
            ),
        );
        assert_eq!(
            acceptance
                .full_view_teleport
                .pending_move_ingress
                .map(|(sequence, _, _)| sequence),
            Some(42),
        );
    }

    #[test]
    fn full_view_move_player_ingress_marker_rejects_nonfinite_xz_but_preserves_y_independence() {
        let started = Instant::now();
        for position in [
            [f32::NAN, 70.0, 1_040.5],
            [1_040.5, 70.0, f32::NEG_INFINITY],
        ] {
            let mut acceptance = AcceptanceRun::new(Some(900), None, true, false);
            acceptance.set_mutation_coordinate([0, 58, 0]);
            acceptance.begin_world_ready(started, [0.5, 70.0, 0.5], 1);
            let movement = WorldEvent::MovePlayer(protocol::MovePlayerEvent {
                runtime_id: 1,
                position,
                pitch: 0.0,
                yaw: 0.0,
            });
            let accepted =
                acceptance.observe_full_view_teleport_ingress(&movement, 43, started, 0, 10);
            assert!(!accepted);
            assert_eq!(
                accepted_move_player_ingress_marker(accepted, 43, &movement),
                None,
                "nonfinite position {position:?} produced parser-visible ingress evidence",
            );
        }

        let mut acceptance = AcceptanceRun::new(Some(900), None, true, false);
        acceptance.set_mutation_coordinate([0, 58, 0]);
        acceptance.begin_world_ready(started, [0.5, 70.0, 0.5], 1);
        let movement = WorldEvent::MovePlayer(protocol::MovePlayerEvent {
            runtime_id: 1,
            position: [1_040.5, f32::INFINITY, 1_040.5],
            pitch: 0.0,
            yaw: 0.0,
        });
        let accepted = acceptance.observe_full_view_teleport_ingress(&movement, 43, started, 0, 10);
        assert!(
            accepted,
            "nonfinite MovePlayer Y changed binding acceptance"
        );
        assert_eq!(
            accepted_move_player_ingress_marker(accepted, 43, &movement),
            None,
            "nonfinite MovePlayer Y produced a non-parser-safe marker instead of only preserving capture",
        );
        assert_eq!(
            acceptance
                .full_view_teleport
                .pending_move_ingress
                .map(|(sequence, _, _)| sequence),
            Some(43),
        );
    }

    #[test]
    fn move_player_ingress_marker_is_flushed_before_source_capture() {
        struct OrderingWriter {
            bytes: std::rc::Rc<std::cell::RefCell<Vec<u8>>>,
            flushed: std::rc::Rc<std::cell::Cell<bool>>,
        }

        impl std::io::Write for OrderingWriter {
            fn write(&mut self, bytes: &[u8]) -> std::io::Result<usize> {
                self.bytes.borrow_mut().extend_from_slice(bytes);
                Ok(bytes.len())
            }

            fn flush(&mut self) -> std::io::Result<()> {
                self.flushed.set(true);
                Ok(())
            }
        }

        let bytes = std::rc::Rc::new(std::cell::RefCell::new(Vec::new()));
        let flushed = std::rc::Rc::new(std::cell::Cell::new(false));
        let capture_called = std::rc::Rc::new(std::cell::Cell::new(false));
        let mut writer = OrderingWriter {
            bytes: std::rc::Rc::clone(&bytes),
            flushed: std::rc::Rc::clone(&flushed),
        };
        let capture_called_for_callback = std::rc::Rc::clone(&capture_called);
        let bytes_for_callback = std::rc::Rc::clone(&bytes);
        let flushed_for_callback = std::rc::Rc::clone(&flushed);

        write_move_player_ingress_before_source_capture(
            &mut writer,
            "RUST_MCBE_MOVE_PLAYER_INGRESS sequence=42 position=1040.5,93.75,1040.5",
            || {
                assert!(flushed_for_callback.get());
                assert_eq!(
                    bytes_for_callback.borrow().as_slice(),
                    b"RUST_MCBE_MOVE_PLAYER_INGRESS sequence=42 position=1040.5,93.75,1040.5\n",
                );
                capture_called_for_callback.set(true);
            },
        );

        assert!(capture_called.get());
    }

    #[test]
    fn full_view_mutation_stays_disarmed_until_exact_target_and_remesh_binding() {
        let source = [0, 58, 0];
        let started = Instant::now();
        let mut acceptance = AcceptanceRun::new(Some(900), None, true, false);
        acceptance.set_mutation_coordinate(source);

        assert_eq!(acceptance.source_mutation_coordinate(), Some(source));
        assert!(acceptance.mutation.is_none());
        assert_eq!(acceptance.target_mutation_marker(), None);
        assert!(!acceptance.retarget_mutation([1_040, 58, 1_052], started));

        acceptance.full_view_teleport = destination_tracker(started);
        let key = SubChunkKey::new(0, 65, 64, 65);
        let expectation = acceptance
            .full_view_teleport
            .reconcile_presented_expectation(
                settled_teleport_snapshot(),
                proposed_render_expectation(started + Duration::from_millis(200), [(key, 7)]),
                started + Duration::from_millis(200),
            )
            .unwrap();
        assert_eq!(
            acceptance
                .full_view_teleport
                .observe_presented_frame(presented_acknowledgement(
                    &expectation,
                    1,
                    Duration::from_millis(10),
                    Duration::from_millis(20),
                )),
            None
        );
        let completion = acceptance
            .full_view_teleport
            .observe_presented_frame(presented_acknowledgement(
                &expectation,
                2,
                Duration::from_millis(30),
                Duration::from_millis(40),
            ))
            .expect("the exact adjacent frame pair should bind the target");
        let target = [1_040, 58, 1_052];
        assert_eq!(completion.target_mutation_coordinate, target);
        assert!(
            !acceptance.retarget_mutation(target, completion.stable_frame.gpu_completed_at),
            "teleport binding armed mutation before the frozen forced remesh completed"
        );
        assert_eq!(acceptance.target_mutation_marker(), None);

        let remesh_started = completion.stable_frame.gpu_completed_at + Duration::from_millis(1);
        let manifest = ForcedRemeshManifest {
            started_at: remesh_started,
            entries: Arc::from([(key, 8)]),
        };
        assert!(acceptance.full_view_remesh.start(
            Some(&completion),
            exact_destination_status(),
            manifest,
            3,
        ));
        let remesh_expectation = acceptance
            .full_view_remesh
            .reconcile_presented_expectation(
                settled_teleport_snapshot(),
                ForcedRemeshManifestState::Complete,
                Some(proposed_render_expectation(
                    remesh_started + Duration::from_millis(10),
                    [(key, 8)],
                )),
                remesh_started + Duration::from_millis(10),
                4,
            )
            .unwrap();
        assert_eq!(
            acceptance.full_view_remesh.observe_presented_frame(
                presented_acknowledgement(
                    &remesh_expectation,
                    3,
                    Duration::from_millis(10),
                    Duration::from_millis(20),
                ),
                4,
            ),
            None
        );
        let remesh_completion = acceptance
            .full_view_remesh
            .observe_presented_frame(
                presented_acknowledgement(
                    &remesh_expectation,
                    4,
                    Duration::from_millis(30),
                    Duration::from_millis(40),
                ),
                5,
            )
            .expect("the exact forced-remesh frame pair should settle");
        assert_eq!(acceptance.target_mutation_marker(), None);
        assert!(
            acceptance.retarget_mutation(target, remesh_completion.stable_frame.gpu_completed_at,)
        );
        assert_eq!(
            acceptance.target_mutation_marker(),
            Some(format!(
                "RUST_MCBE_TARGET_MUTATION_ARMED source=0,58,0 target=1040,58,1052 view_generation={}",
                remesh_completion.view_generation
            ))
        );
        assert_eq!(acceptance.mutation_coordinate(), Some(target));
        assert_eq!(acceptance.visible_mutation_count(), 0);
    }

    #[test]
    fn leaf_forest_target_mutation_uses_the_binding_move_player_offset() {
        let source = [4, 70, -2];
        assert_eq!(
            leaf_forest_target_mutation_coordinate([1_044.5, 93.75, 1_038.5], source),
            Some([1_044, 70, 1_050])
        );
        assert_eq!(
            leaf_forest_target_mutation_coordinate([f32::NAN, 93.75, 1_038.5], source),
            None
        );
        assert_eq!(
            leaf_forest_target_mutation_coordinate([1_044.5, 93.75, f32::INFINITY], source),
            None
        );
        assert_eq!(
            leaf_forest_target_mutation_coordinate([1_044.5, f32::INFINITY, 1_038.5], source,),
            Some([1_044, 70, 1_050]),
            "target mutation Y must come from the manifest-compatible source coordinate"
        );
        assert_eq!(
            leaf_forest_target_mutation_coordinate([1_043.5, 93.75, 1_038.5], source),
            None,
            "a MovePlayer target outside the exact 65-chunk forest offset was accepted"
        );
    }

    #[test]
    fn target_mutation_marker_is_exact_and_manifest_comparable() {
        assert_eq!(
            target_mutation_armed_marker([4, 70, -2], [1_044, 70, 1_050], 9),
            "RUST_MCBE_TARGET_MUTATION_ARMED source=4,70,-2 target=1044,70,1050 view_generation=9"
        );
    }

    #[test]
    fn timed_acceptance_deadline_begins_only_when_the_world_is_ready() {
        let mut acceptance = AcceptanceRun::new(Some(900), None, false, false);
        assert_eq!(acceptance.deadline, None);

        let world_ready_at = Instant::now() + Duration::from_secs(60);
        acceptance.begin_world_ready(world_ready_at, [0.5, 70.0, 0.5], 1);

        assert!(acceptance.world_ready);
        assert_eq!(
            acceptance.deadline,
            Some(world_ready_at + Duration::from_secs(900))
        );
    }

    fn settled_transparent_snapshot(generation: u64) -> TransparentSortMetricsSnapshot {
        TransparentSortMetricsSnapshot {
            committed_generation: generation,
            encoded_generation: generation,
            presented_generation: generation,
            ref_count: 42,
            ..Default::default()
        }
    }

    #[test]
    fn timed_exit_completes_immediately_for_gpu_presented_transparent_snapshot() {
        let deadline = Instant::now();
        let mut acceptance = AcceptanceRun::new(Some(60), None, false, true);
        acceptance.deadline = Some(deadline);

        assert_eq!(
            acceptance.exit_decision(deadline, false, settled_transparent_snapshot(17)),
            AcceptanceExitDecision::Complete
        );
    }

    #[test]
    fn timed_exit_does_not_impose_water_settle_on_opaque_acceptance() {
        let deadline = Instant::now();
        let mut acceptance = AcceptanceRun::new(Some(60), None, false, false);
        acceptance.deadline = Some(deadline);

        assert_eq!(
            acceptance.exit_decision(deadline, false, TransparentSortMetricsSnapshot::default(),),
            AcceptanceExitDecision::Complete
        );
    }

    #[test]
    fn timed_exit_waits_for_delayed_gpu_presentation_within_grace() {
        let deadline = Instant::now();
        let mut acceptance = AcceptanceRun::new(Some(60), None, false, true);
        acceptance.deadline = Some(deadline);
        let pending = TransparentSortMetricsSnapshot {
            committed_generation: 18,
            encoded_generation: 18,
            presented_generation: 17,
            ref_count: 42,
            ..Default::default()
        };

        assert_eq!(
            acceptance.exit_decision(deadline, false, pending),
            AcceptanceExitDecision::WaitForTransparentPresentation
        );
        assert_eq!(
            acceptance.exit_decision(
                deadline + TRANSPARENT_PRESENTATION_EXIT_GRACE - Duration::from_millis(1),
                false,
                settled_transparent_snapshot(18),
            ),
            AcceptanceExitDecision::Complete
        );
    }

    #[test]
    fn timed_exit_turns_unsettled_transparency_into_bounded_fatal_failure() {
        let deadline = Instant::now();
        let mut acceptance = AcceptanceRun::new(Some(60), None, false, true);
        acceptance.deadline = Some(deadline);
        let pending = TransparentSortMetricsSnapshot {
            committed_generation: 581,
            encoded_generation: 581,
            presented_generation: 0,
            ref_count: 42,
            ..Default::default()
        };

        assert_eq!(
            acceptance.exit_decision(
                deadline + TRANSPARENT_PRESENTATION_EXIT_GRACE,
                false,
                pending,
            ),
            AcceptanceExitDecision::TransparentPresentationTimedOut
        );
        assert!(AcceptanceExitDecision::TransparentPresentationTimedOut.is_error());
    }

    #[test]
    fn fatal_error_exits_immediately_even_before_timed_deadline() {
        let now = Instant::now();
        let mut acceptance = AcceptanceRun::new(Some(60), None, false, true);
        acceptance.deadline = Some(now + Duration::from_secs(60));

        assert_eq!(
            acceptance.exit_decision(now, true, TransparentSortMetricsSnapshot::default()),
            AcceptanceExitDecision::Fatal
        );
        assert!(AcceptanceExitDecision::Fatal.is_error());
    }

    const DESTINATION_COHORT: ViewCohort = ViewCohort {
        dimension: 0,
        center: [65, 65],
        radius: 16,
    };

    const SOURCE_COHORT: ViewCohort = ViewCohort {
        dimension: 0,
        center: [0, 0],
        radius: 16,
    };

    fn exact_destination_status() -> ViewCohortStatus {
        ViewCohortStatus {
            target: DESTINATION_COHORT,
            committed: Some(DESTINATION_COHORT),
            expected: 1_089,
            loaded_target: 1_089,
            missing_target: 0,
            foreign_loaded: 0,
            foreign_requested: 0,
            foreign_resident: 0,
            source_leftover: 0,
            resident_count: 9_000,
            resident_hash: 0x1234,
            known_air_count: 1_000,
            known_air_hash: 0x5678,
        }
    }

    fn settled_teleport_snapshot() -> TeleportReadySnapshot {
        TeleportReadySnapshot {
            received_radius_chunks: Some(16),
            publisher_radius_chunks: Some(16),
            rendered_sub_chunks: 8_000,
            resident_sub_chunks: 9_000,
            visible_sub_chunks: 7_000,
            loaded_columns: 1_089,
            cohort: Some(exact_destination_status()),
            last_chunk_commit_at: None,
            last_mesh_dispatch_at: None,
            last_mesh_completion_at: None,
            last_mesh_ack_at: None,
            work: WorldReadyWork::default(),
        }
    }

    fn destination_tracker(started: Instant) -> FullViewTeleportTracker {
        let mut tracker = FullViewTeleportTracker::new(true);
        tracker.set_source_mutation_coordinate([0, 58, 0]);
        tracker.begin_world_ready([0.5, 70.0, 0.5], 1);
        assert!(tracker.observe(
            &WorldEvent::MovePlayer(protocol::MovePlayerEvent {
                runtime_id: 1,
                position: [1_040.5, 70.0, 1_040.5],
                pitch: 0.0,
                yaw: 0.0,
            }),
            started,
            0,
        ));
        tracker.observe(
            &WorldEvent::PublisherUpdate(protocol::PublisherUpdateEvent {
                center: [1_040, 70, 1_040],
                radius_blocks: 256,
            }),
            started + Duration::from_millis(100),
            0,
        );
        tracker
    }

    fn proposed_render_expectation(
        render_ready_at: Instant,
        manifest: impl IntoIterator<Item = (SubChunkKey, u64)>,
    ) -> TargetRenderExpectation {
        TargetRenderExpectation {
            cohort: RenderViewCohort::new(
                DESTINATION_COHORT.dimension,
                DESTINATION_COHORT.center,
                DESTINATION_COHORT.radius,
            ),
            source_cohort: Some(RenderViewCohort::new(
                SOURCE_COHORT.dimension,
                SOURCE_COHORT.center,
                SOURCE_COHORT.radius,
            )),
            manifest: Arc::from(manifest.into_iter().collect::<Vec<_>>()),
            view_generation: 0,
            render_ready_at,
        }
    }

    fn presented_acknowledgement(
        expectation: &TargetRenderExpectation,
        frame_sequence: u64,
        present_after_ready: Duration,
        gpu_after_ready: Duration,
    ) -> PresentedFrameAck {
        PresentedFrameAck {
            cohort: expectation.cohort,
            frame_sequence,
            allocation_manifest: Arc::clone(&expectation.manifest),
            visible_allocation_manifest: Arc::clone(&expectation.manifest),
            drawn_manifest: Arc::clone(&expectation.manifest),
            view_generation: expectation.view_generation,
            render_ready_at: expectation.render_ready_at,
            present_returned_at: expectation.render_ready_at + present_after_ready,
            gpu_completed_at: expectation.render_ready_at + gpu_after_ready,
            missing_target_instances: 0,
            unexpected_target_instances: 0,
            source_instances: 0,
            foreign_instances: 0,
            stale_generation_instances: 0,
            orphan_allocations: 0,
            transparent_sort_generation: 17,
            model_witness: None,
        }
    }

    #[test]
    fn transparent_sort_marker_requires_new_presented_committed_generation_with_refs() {
        let valid = TransparentSortMetricsSnapshot {
            committed_generation: 17,
            presented_generation: 17,
            ref_count: 99,
            ..Default::default()
        };
        assert_eq!(
            transparent_sort_committed_marker(16, valid),
            Some("RUST_MCBE_TRANSPARENT_SORT_COMMITTED generation=17 ref_count=99".to_owned())
        );
        assert_eq!(transparent_sort_committed_marker(17, valid), None);
        assert_eq!(
            transparent_sort_committed_marker(
                16,
                TransparentSortMetricsSnapshot {
                    presented_generation: 17,
                    committed_generation: 18,
                    ref_count: 99,
                    ..Default::default()
                }
            ),
            None
        );
        assert_eq!(
            transparent_sort_committed_marker(
                16,
                TransparentSortMetricsSnapshot {
                    presented_generation: 17,
                    committed_generation: 17,
                    ref_count: 0,
                    ..Default::default()
                }
            ),
            None
        );
    }

    #[test]
    fn transparent_sort_marker_writer_targets_and_flushes_stdout_sink() {
        struct FlushRecordingWriter {
            bytes: Vec<u8>,
            flushed: bool,
        }

        impl std::io::Write for FlushRecordingWriter {
            fn write(&mut self, bytes: &[u8]) -> std::io::Result<usize> {
                self.bytes.extend_from_slice(bytes);
                Ok(bytes.len())
            }

            fn flush(&mut self) -> std::io::Result<()> {
                self.flushed = true;
                Ok(())
            }
        }

        let mut writer = FlushRecordingWriter {
            bytes: Vec::new(),
            flushed: false,
        };
        write_stdout_marker(
            &mut writer,
            "RUST_MCBE_TRANSPARENT_SORT_COMMITTED generation=17 ref_count=99",
        );

        assert_eq!(
            writer.bytes,
            b"RUST_MCBE_TRANSPARENT_SORT_COMMITTED generation=17 ref_count=99\n"
        );
        assert!(writer.flushed);
    }

    fn binding_teleport_completion(
        started: Instant,
        settle_latency: Duration,
    ) -> FullViewTeleportCompletion {
        let mut tracker = destination_tracker(started);
        let key = SubChunkKey::new(0, 64, 65, 65);
        let render_ready = Duration::from_millis(200);
        let expectation = tracker
            .reconcile_presented_expectation(
                settled_teleport_snapshot(),
                proposed_render_expectation(started + render_ready, [(key, 7)]),
                started + render_ready,
            )
            .unwrap();
        let stable_gpu_after_ready = settle_latency.saturating_sub(render_ready);
        let first_gpu_after_ready =
            stable_gpu_after_ready.saturating_sub(Duration::from_millis(20));
        assert_eq!(
            tracker.observe_presented_frame(presented_acknowledgement(
                &expectation,
                41,
                first_gpu_after_ready.saturating_sub(Duration::from_millis(10)),
                first_gpu_after_ready,
            )),
            None
        );
        tracker
            .observe_presented_frame(presented_acknowledgement(
                &expectation,
                42,
                stable_gpu_after_ready.saturating_sub(Duration::from_millis(10)),
                stable_gpu_after_ready,
            ))
            .unwrap()
    }

    #[test]
    fn exact_full_view_proof_uses_stable_presented_transparent_sort_generation() {
        let completion = binding_teleport_completion(Instant::now(), Duration::from_millis(1_500));

        let proof = teleport_proof(exact_destination_status(), &completion);

        assert_eq!(proof.exact.transparent_sort_generation, 17);
        assert!(
            super::exact_full_view_proof_marker_fields(&proof.exact)
                .contains("transparent_sort_generation=17")
        );
    }

    #[test]
    fn full_view_teleport_arms_only_with_a_fifo_committed_source_cohort() {
        let started = Instant::now();
        let movement = WorldEvent::MovePlayer(protocol::MovePlayerEvent {
            runtime_id: 1,
            position: [1_040.5, 70.0, 1_040.5],
            pitch: 0.0,
            yaw: 0.0,
        });
        let mut tracker = FullViewTeleportTracker::new(true);
        tracker.set_source_mutation_coordinate([0, 58, 0]);
        tracker.begin_world_ready([0.5, 70.0, 0.5], 1);

        assert!(tracker.observe_ingress(&movement, 1, started, 0, 10));
        let WorldEvent::MovePlayer(move_player) = &movement else {
            unreachable!();
        };
        let move_player = *move_player;
        assert!(!tracker.commit_move(1, move_player, None));
        assert!(tracker.pending.is_none());
        assert!(tracker.observe_ingress(&movement, 2, started + Duration::from_millis(1), 0, 11,));
        assert!(tracker.commit_move(2, move_player, Some(SOURCE_COHORT)));
        assert_eq!(
            tracker.pending.as_ref().map(|pending| pending.source),
            Some(SOURCE_COHORT)
        );
    }

    #[test]
    fn out_of_order_move_waits_for_fifo_source_commit_before_arming() {
        let started = Instant::now();
        let movement = protocol::MovePlayerEvent {
            runtime_id: 1,
            position: [1_040.5, 70.0, 1_040.5],
            pitch: 0.0,
            yaw: 0.0,
        };
        let mut tracker = FullViewTeleportTracker::new(true);
        tracker.set_source_mutation_coordinate([0, 58, 0]);
        tracker.begin_world_ready([0.5, 70.0, 0.5], 1);
        let mut stream = WorldStream::new(WorldBootstrap {
            dimension: 0,
            local_player_runtime_id: 1,
            player_position: [0.5, 70.0, 0.5],
            world_spawn_position: [0, 70, 0],
            air_network_id: 12_530,
            block_network_ids_are_hashes: false,
        });

        assert!(tracker.observe_ingress(&WorldEvent::MovePlayer(movement), 2, started, 0, 10,));
        stream.schedule_source_capture(2);
        stream.submit(2, WorldEvent::MovePlayer(movement)).unwrap();
        assert!(stream.take_committed_controls().is_empty());
        assert!(tracker.pending.is_none());

        stream
            .submit(
                1,
                WorldEvent::PublisherUpdate(protocol::PublisherUpdateEvent {
                    center: [0, 70, 0],
                    radius_blocks: 256,
                }),
            )
            .unwrap();
        let controls = stream.take_committed_controls();
        assert_eq!(controls.len(), 1);
        assert!(tracker.observe_committed_control(&controls[0]));
        let pending = tracker.pending.as_ref().unwrap();
        assert_eq!(pending.started, started);
        assert_eq!(pending.source, SOURCE_COHORT);
        assert_eq!(stream.committed_view_cohort(), Some(SOURCE_COHORT));
    }

    #[test]
    fn write_buffer_ack_alone_never_settles_binding_teleport() {
        let started = Instant::now();
        let mut tracker = destination_tracker(started);
        let key = SubChunkKey::new(0, 64, 65, 65);
        let proposal =
            proposed_render_expectation(started + Duration::from_millis(200), [(key, 7)]);

        let first = tracker
            .reconcile_presented_expectation(
                settled_teleport_snapshot(),
                proposal.clone(),
                started + Duration::from_millis(200),
            )
            .expect("an exact clean cohort should freeze a render expectation");
        let unchanged = tracker
            .reconcile_presented_expectation(
                settled_teleport_snapshot(),
                proposal,
                started + Duration::from_secs(30),
            )
            .expect("an unchanged exact cohort should retain its expectation");

        assert_eq!(unchanged, first);
        assert_eq!(tracker.completed, None);
        assert!(tracker.pending.is_some());
    }

    #[test]
    fn non_empty_leaf_forest_never_binds_an_empty_target_expectation() {
        let started = Instant::now();
        let mut tracker = destination_tracker(started);
        let empty = proposed_render_expectation(started + Duration::from_millis(200), []);

        assert_eq!(
            tracker.reconcile_presented_expectation(
                settled_teleport_snapshot(),
                empty,
                started + Duration::from_millis(200),
            ),
            None
        );
        assert!(tracker.pending.is_some());
        assert_eq!(tracker.completed, None);
        assert_eq!(tracker.completed_target_mutation, None);
    }

    #[test]
    fn binding_teleport_requires_two_identical_presented_gpu_completed_frames() {
        let started = Instant::now();
        let mut tracker = destination_tracker(started);
        let key = SubChunkKey::new(0, 64, 65, 65);
        let expectation = tracker
            .reconcile_presented_expectation(
                settled_teleport_snapshot(),
                proposed_render_expectation(started + Duration::from_millis(200), [(key, 7)]),
                started + Duration::from_millis(200),
            )
            .unwrap();
        let first = presented_acknowledgement(
            &expectation,
            10,
            Duration::from_millis(10),
            Duration::from_millis(20),
        );
        let second = presented_acknowledgement(
            &expectation,
            11,
            Duration::from_millis(30),
            Duration::from_millis(40),
        );

        assert_eq!(tracker.observe_presented_frame(first), None);
        let completion = tracker
            .observe_presented_frame(second)
            .expect("the adjacent second exact GPU-complete frame should settle");
        assert_eq!(completion.settle_latency, Duration::from_millis(240));
        assert_eq!(tracker.completed, Some(Duration::from_millis(240)));
    }

    #[test]
    fn render_manifest_change_resets_teleport_stability() {
        let started = Instant::now();
        let mut tracker = destination_tracker(started);
        let key_a = SubChunkKey::new(0, 64, 65, 65);
        let key_b = SubChunkKey::new(0, 65, 65, 65);
        let expectation_a = tracker
            .reconcile_presented_expectation(
                settled_teleport_snapshot(),
                proposed_render_expectation(started + Duration::from_millis(200), [(key_a, 7)]),
                started + Duration::from_millis(200),
            )
            .unwrap();
        assert_eq!(
            tracker.observe_presented_frame(presented_acknowledgement(
                &expectation_a,
                1,
                Duration::from_millis(10),
                Duration::from_millis(20),
            )),
            None
        );

        let expectation_b = tracker
            .reconcile_presented_expectation(
                settled_teleport_snapshot(),
                proposed_render_expectation(
                    started + Duration::from_millis(300),
                    [(key_a, 7), (key_b, 8)],
                ),
                started + Duration::from_millis(300),
            )
            .unwrap();
        assert_ne!(expectation_b.view_generation, expectation_a.view_generation);
        assert_eq!(
            tracker.observe_presented_frame(presented_acknowledgement(
                &expectation_b,
                2,
                Duration::from_millis(10),
                Duration::from_millis(20),
            )),
            None,
            "the first frame for a replacement manifest must only re-arm stability"
        );
        assert!(
            tracker
                .observe_presented_frame(presented_acknowledgement(
                    &expectation_b,
                    3,
                    Duration::from_millis(30),
                    Duration::from_millis(40),
                ))
                .is_some()
        );
    }

    #[test]
    fn source_render_instance_blocks_settle_with_clean_world_queues() {
        let started = Instant::now();
        let mut tracker = destination_tracker(started);
        let key = SubChunkKey::new(0, 64, 65, 65);
        let expectation = tracker
            .reconcile_presented_expectation(
                settled_teleport_snapshot(),
                proposed_render_expectation(started + Duration::from_millis(200), [(key, 7)]),
                started + Duration::from_millis(200),
            )
            .unwrap();
        for sequence in [1, 2] {
            let mut blocked = presented_acknowledgement(
                &expectation,
                sequence,
                Duration::from_millis(sequence * 10),
                Duration::from_millis(sequence * 10 + 5),
            );
            blocked.source_instances = 1;
            assert_eq!(tracker.observe_presented_frame(blocked), None);
        }
        assert_eq!(tracker.completed, None);
        assert!(tracker.pending.is_some());
    }

    #[test]
    fn cohort_identity_change_resets_stability_even_when_counts_match() {
        let started = Instant::now();
        let mut tracker = destination_tracker(started);
        let key = SubChunkKey::new(0, 64, 65, 65);
        let first_expectation = tracker
            .reconcile_presented_expectation(
                settled_teleport_snapshot(),
                proposed_render_expectation(started + Duration::from_millis(200), [(key, 7)]),
                started + Duration::from_millis(200),
            )
            .unwrap();
        assert_eq!(
            tracker.observe_presented_frame(presented_acknowledgement(
                &first_expectation,
                1,
                Duration::from_millis(10),
                Duration::from_millis(20),
            )),
            None
        );

        let mut changed_snapshot = settled_teleport_snapshot();
        changed_snapshot.cohort.as_mut().unwrap().resident_hash ^= 0x55aa;
        let replacement = tracker
            .reconcile_presented_expectation(
                changed_snapshot,
                proposed_render_expectation(started + Duration::from_millis(300), [(key, 7)]),
                started + Duration::from_millis(300),
            )
            .unwrap();
        assert_ne!(
            replacement.view_generation,
            first_expectation.view_generation
        );
        assert_eq!(
            tracker.observe_presented_frame(presented_acknowledgement(
                &first_expectation,
                2,
                Duration::from_millis(30),
                Duration::from_millis(40),
            )),
            None,
            "an acknowledgement for the old cohort identity must not settle"
        );
    }

    #[test]
    fn visibility_count_change_does_not_reset_binding_render_stability() {
        let started = Instant::now();
        let mut tracker = destination_tracker(started);
        let key = SubChunkKey::new(0, 64, 65, 65);
        let proposal =
            proposed_render_expectation(started + Duration::from_millis(200), [(key, 7)]);
        let expectation = tracker
            .reconcile_presented_expectation(
                settled_teleport_snapshot(),
                proposal.clone(),
                started + Duration::from_millis(200),
            )
            .unwrap();
        assert_eq!(
            tracker.observe_presented_frame(presented_acknowledgement(
                &expectation,
                1,
                Duration::from_millis(10),
                Duration::from_millis(20),
            )),
            None
        );

        let mut culled = settled_teleport_snapshot();
        culled.visible_sub_chunks = 0;
        let retained = tracker.reconcile_presented_expectation(
            culled,
            proposal,
            started + Duration::from_millis(220),
        );
        assert_eq!(
            retained,
            Some(expectation.clone()),
            "a non-binding culling count replaced the frozen expectation"
        );
        assert!(
            tracker
                .observe_presented_frame(presented_acknowledgement(
                    &expectation,
                    2,
                    Duration::from_millis(30),
                    Duration::from_millis(40),
                ))
                .is_some(),
            "a non-binding visibility change discarded the first exact frame"
        );
    }

    #[test]
    fn teleport_stage_offsets_are_monotonic_and_settle_equals_stable_gpu_completion() {
        let started = Instant::now();
        let mut tracker = destination_tracker(started);
        let key = SubChunkKey::new(0, 64, 65, 65);
        let expectation = tracker
            .reconcile_presented_expectation(
                settled_teleport_snapshot(),
                proposed_render_expectation(started + Duration::from_millis(200), [(key, 7)]),
                started + Duration::from_millis(200),
            )
            .unwrap();
        let mut malformed =
            presented_acknowledgement(&expectation, 9, Duration::ZERO, Duration::from_millis(1));
        malformed.present_returned_at = expectation.render_ready_at - Duration::from_millis(1);
        assert_eq!(tracker.observe_presented_frame(malformed), None);

        assert_eq!(
            tracker.observe_presented_frame(presented_acknowledgement(
                &expectation,
                10,
                Duration::from_millis(10),
                Duration::from_millis(20),
            )),
            None
        );
        let completion = tracker
            .observe_presented_frame(presented_acknowledgement(
                &expectation,
                11,
                Duration::from_millis(30),
                Duration::from_millis(40),
            ))
            .unwrap();

        assert_eq!(completion.render_ready_latency, Duration::from_millis(200));
        assert_eq!(
            completion.first_present_return_latency,
            Duration::from_millis(210)
        );
        assert_eq!(
            completion.first_gpu_completion_latency,
            Duration::from_millis(220)
        );
        assert_eq!(
            completion.stable_present_return_latency,
            Duration::from_millis(230)
        );
        assert_eq!(
            completion.stable_gpu_completion_latency,
            Duration::from_millis(240)
        );
        assert_eq!(
            completion.settle_latency,
            completion.stable_gpu_completion_latency
        );
        assert_eq!(completion.view_generation, expectation.view_generation);
    }

    #[test]
    fn presented_frame_timestamp_regression_resets_the_stability_pair() {
        let started = Instant::now();
        let mut tracker = destination_tracker(started);
        let key = SubChunkKey::new(0, 64, 65, 65);
        let expectation = tracker
            .reconcile_presented_expectation(
                settled_teleport_snapshot(),
                proposed_render_expectation(started + Duration::from_millis(200), [(key, 7)]),
                started + Duration::from_millis(200),
            )
            .unwrap();
        assert_eq!(
            tracker.observe_presented_frame(presented_acknowledgement(
                &expectation,
                1,
                Duration::from_millis(30),
                Duration::from_millis(40),
            )),
            None
        );
        assert_eq!(
            tracker.observe_presented_frame(presented_acknowledgement(
                &expectation,
                2,
                Duration::from_millis(20),
                Duration::from_millis(50),
            )),
            None,
            "an earlier present-return timestamp formed a false stable pair"
        );
        let completion = tracker
            .observe_presented_frame(presented_acknowledgement(
                &expectation,
                3,
                Duration::from_millis(60),
                Duration::from_millis(70),
            ))
            .expect("a later adjacent monotonic frame should complete after the reset");
        assert_eq!(
            completion.first_present_return_latency,
            Duration::from_millis(220)
        );
        assert_eq!(completion.settle_latency, Duration::from_millis(270));
    }

    #[test]
    fn radius_16_with_one_loaded_target_column_and_empty_work_never_arms() {
        let started = Instant::now();
        let mut tracker = destination_tracker(started);
        let mut snapshot = settled_teleport_snapshot();
        let mut status = exact_destination_status();
        status.loaded_target = 1;
        status.missing_target = 1_088;
        status.resident_count = 1;
        status.known_air_count = 0;
        snapshot.loaded_columns = 1;
        snapshot.cohort = Some(status);

        assert_eq!(
            tracker.observe_snapshot(snapshot, started + Duration::from_millis(200)),
            None
        );
        assert_eq!(
            tracker.observe_snapshot(snapshot, started + Duration::from_secs(5)),
            None
        );
        assert!(!tracker.has_clean_candidate());
    }

    #[test]
    fn equal_total_loaded_count_with_missing_target_and_foreign_source_never_arms() {
        let started = Instant::now();
        let mut tracker = destination_tracker(started);
        let mut snapshot = settled_teleport_snapshot();
        let mut status = exact_destination_status();
        status.loaded_target = 1_088;
        status.missing_target = 1;
        status.foreign_loaded = 1;
        status.source_leftover = 1;
        snapshot.loaded_columns = 1_089;
        snapshot.cohort = Some(status);

        assert_eq!(
            tracker.observe_snapshot(snapshot, started + Duration::from_millis(200)),
            None
        );
        assert_eq!(
            tracker.observe_snapshot(snapshot, started + Duration::from_secs(5)),
            None
        );
        assert!(!tracker.has_clean_candidate());
    }

    #[test]
    fn replacing_resident_key_at_equal_counts_resets_stability() {
        let started = Instant::now();
        let mut tracker = destination_tracker(started);
        let first = settled_teleport_snapshot();
        assert_eq!(
            tracker.observe_snapshot(first, started + Duration::from_millis(200)),
            None
        );

        let mut replacement = first;
        replacement.cohort.as_mut().unwrap().resident_hash = 0x9999;
        assert_eq!(
            tracker.observe_snapshot(replacement, started + Duration::from_millis(2_300)),
            None,
            "equal resident counts with different keys retained the old stability candidate"
        );
        assert_eq!(
            tracker.observe_snapshot(replacement, started + Duration::from_millis(4_300)),
            None,
            "clean/upload state alone must never complete the presented-frame gate"
        );
        assert_eq!(tracker.completed, None);
    }

    #[test]
    fn wrong_target_center_dimension_or_radius_never_arms() {
        let started = Instant::now();
        let mut tracker = destination_tracker(started);
        for (index, committed) in [
            ViewCohort {
                center: [64, 65],
                ..DESTINATION_COHORT
            },
            ViewCohort {
                dimension: 1,
                ..DESTINATION_COHORT
            },
            ViewCohort {
                radius: 15,
                ..DESTINATION_COHORT
            },
        ]
        .into_iter()
        .enumerate()
        {
            let mut snapshot = settled_teleport_snapshot();
            snapshot.cohort.as_mut().unwrap().committed = Some(committed);
            assert_eq!(
                tracker.observe_snapshot(
                    snapshot,
                    started + Duration::from_secs(u64::try_from(index).unwrap() * 3 + 1),
                ),
                None
            );
            assert!(!tracker.has_clean_candidate());
        }
    }

    #[test]
    fn previously_seen_wrong_radius_publisher_does_not_arm_target_stage() {
        let started = Instant::now();
        let mut tracker = FullViewTeleportTracker::new(true);
        tracker.set_source_mutation_coordinate([0, 58, 0]);
        tracker.begin_world_ready([0.5, 70.0, 0.5], 1);
        tracker.observe(
            &WorldEvent::PublisherUpdate(protocol::PublisherUpdateEvent {
                center: [1_040, 70, 1_040],
                radius_blocks: 240,
            }),
            started,
            0,
        );
        assert!(tracker.observe(
            &WorldEvent::MovePlayer(protocol::MovePlayerEvent {
                runtime_id: 1,
                position: [1_040.5, 70.0, 1_040.5],
                pitch: 0.0,
                yaw: 0.0,
            }),
            started + Duration::from_millis(100),
            0,
        ));

        assert_eq!(
            tracker.observe_snapshot(
                settled_teleport_snapshot(),
                started + Duration::from_millis(200),
            ),
            None
        );
        assert_eq!(
            tracker.observe_snapshot(
                settled_teleport_snapshot(),
                started + Duration::from_millis(2_200),
            ),
            None
        );
        assert!(!tracker.has_clean_candidate());
    }

    #[test]
    fn teleport_cohort_progress_is_target_tagged_formatted_and_rate_limited() {
        let started = Instant::now();
        let mut tracker = destination_tracker(started);
        let mut status = exact_destination_status();
        status.loaded_target = 1;
        status.missing_target = 1_088;
        let work = WorldReadyWork {
            outstanding_sub_chunks: 7,
            unacknowledged_meshes: 3,
            ..Default::default()
        };
        let timeout_progress = SubChunkTimeoutProgress {
            awaiting_responses: 5,
            timeouts: 4,
            retries_scheduled: 3,
            retry_exhaustions: 2,
        };

        let line = tracker
            .cohort_progress_line(
                status,
                work,
                timeout_progress,
                started + Duration::from_millis(200),
            )
            .expect("first pending cohort observation should be inspectable");
        assert!(line.starts_with("RUST_MCBE_TELEPORT_COHORT target=0:65:65:16"));
        assert!(line.contains("committed=0:65:65:16"));
        assert!(line.contains("expected=1089 loaded_target=1 missing_target=1088"));
        assert!(line.contains("resident_count=9000 resident_hash=0000000000001234"));
        assert!(line.contains("known_air_count=1000 known_air_hash=0000000000005678"));
        assert!(line.contains("outstanding_sub_chunks=7"));
        assert!(line.contains("awaiting_sub_chunk_responses=5"));
        assert!(line.contains("sub_chunk_timeouts=4"));
        assert!(line.contains("sub_chunk_retries_scheduled=3"));
        assert!(line.contains("sub_chunk_retry_exhaustions=2"));
        assert!(line.contains("unacknowledged_meshes=3"));
        assert_eq!(
            tracker.cohort_progress_line(
                status,
                work,
                timeout_progress,
                started + Duration::from_millis(1_199),
            ),
            None
        );
        assert!(
            tracker
                .cohort_progress_line(
                    status,
                    work,
                    timeout_progress,
                    started + Duration::from_millis(1_200),
                )
                .is_some()
        );
    }

    #[test]
    fn global_stream_timestamps_are_emitted_as_separate_target_tagged_diagnostics() {
        let mut completion =
            binding_teleport_completion(Instant::now(), Duration::from_millis(1_500));
        completion.last_chunk_commit_latency = Some(Duration::from_millis(10));
        completion.last_mesh_dispatch_latency = Some(Duration::from_millis(20));
        completion.last_mesh_completion_latency = Some(Duration::from_millis(30));
        completion.last_mesh_ack_latency = Some(Duration::from_millis(40));
        let marker =
            super::teleport_global_stage_diagnostic_marker(DESTINATION_COHORT, &completion);

        assert_eq!(
            marker,
            "RUST_MCBE_TELEPORT_GLOBAL_STAGE_DIAGNOSTIC target=0:65:65:16 global_commit_ms=10.0000 global_mesh_dispatch_ms=20.0000 global_mesh_complete_ms=30.0000 global_mesh_ack_ms=40.0000"
        );
    }

    #[test]
    fn foreign_and_source_events_do_not_advance_target_stage_diagnostics() {
        let started = Instant::now();
        let mut tracker = destination_tracker(started);
        for (event, observed_at) in [
            (
                WorldEvent::LevelChunk(LevelChunkEvent {
                    dimension: 0,
                    x: 0,
                    z: 0,
                    mode: LevelChunkMode::LimitlessRequests,
                    payload: Vec::new(),
                }),
                started + Duration::from_millis(200),
            ),
            (
                WorldEvent::LevelChunk(LevelChunkEvent {
                    dimension: 1,
                    x: 65,
                    z: 65,
                    mode: LevelChunkMode::LimitlessRequests,
                    payload: Vec::new(),
                }),
                started + Duration::from_millis(300),
            ),
            (
                WorldEvent::SubChunks(SubChunkBatchEvent {
                    dimension: 0,
                    entries: vec![SubChunkEntryEvent {
                        position: [0, -4, 0],
                        result: SubChunkResult::AllAir,
                    }],
                }),
                started + Duration::from_millis(400),
            ),
            (
                WorldEvent::SubChunks(SubChunkBatchEvent {
                    dimension: 1,
                    entries: vec![SubChunkEntryEvent {
                        position: [65, -4, 65],
                        result: SubChunkResult::AllAir,
                    }],
                }),
                started + Duration::from_millis(500),
            ),
        ] {
            tracker.observe(&event, observed_at, 0);
        }
        tracker.observe(
            &WorldEvent::LevelChunk(LevelChunkEvent {
                dimension: 0,
                x: 65,
                z: 65,
                mode: LevelChunkMode::LimitlessRequests,
                payload: Vec::new(),
            }),
            started + Duration::from_millis(600),
            0,
        );
        tracker.observe(
            &WorldEvent::SubChunks(SubChunkBatchEvent {
                dimension: 0,
                entries: vec![
                    SubChunkEntryEvent {
                        position: [0, -4, 0],
                        result: SubChunkResult::AllAir,
                    },
                    SubChunkEntryEvent {
                        position: [65, -4, 65],
                        result: SubChunkResult::AllAir,
                    },
                ],
            }),
            started + Duration::from_millis(700),
            0,
        );
        assert_eq!(
            tracker.observe_snapshot(
                settled_teleport_snapshot(),
                started + Duration::from_millis(800),
            ),
            None
        );
        let expectation = tracker
            .pending
            .as_ref()
            .and_then(|pending| pending.presented_candidate.as_ref())
            .map(|candidate| candidate.expectation.clone())
            .unwrap();
        assert_eq!(
            tracker.observe_presented_frame(presented_acknowledgement(
                &expectation,
                1,
                Duration::from_millis(10),
                Duration::from_millis(20),
            )),
            None
        );
        let completion = tracker
            .observe_presented_frame(presented_acknowledgement(
                &expectation,
                2,
                Duration::from_millis(30),
                Duration::from_millis(40),
            ))
            .unwrap();

        assert_eq!(completion.level_chunk_events, 1);
        assert_eq!(
            completion.first_level_chunk_latency,
            Some(Duration::from_millis(600))
        );
        assert_eq!(
            completion.last_level_chunk_latency,
            Some(Duration::from_millis(600))
        );
        assert_eq!(completion.sub_chunk_events, 1);
        assert_eq!(
            completion.first_sub_chunk_latency,
            Some(Duration::from_millis(700))
        );
        assert_eq!(
            completion.last_sub_chunk_latency,
            Some(Duration::from_millis(700))
        );
    }

    #[test]
    fn full_view_teleport_requires_far_motion_matching_publisher_and_two_presented_frames() {
        let started = Instant::now();
        let mut tracker = FullViewTeleportTracker::new(true);
        tracker.set_source_mutation_coordinate([0, 58, 0]);
        tracker.begin_world_ready([0.5, 70.0, 0.5], 1);

        tracker.observe(
            &WorldEvent::MovePlayer(protocol::MovePlayerEvent {
                runtime_id: 1,
                position: [32.5, 70.0, 0.5],
                pitch: 0.0,
                yaw: 0.0,
            }),
            started,
            0,
        );
        assert!(
            !tracker.is_pending(),
            "near movement armed a full-view gate"
        );

        tracker.observe(
            &WorldEvent::MovePlayer(protocol::MovePlayerEvent {
                runtime_id: 1,
                position: [1_040.5, 70.0, 1_040.5],
                pitch: 0.0,
                yaw: 0.0,
            }),
            started,
            0,
        );
        assert!(tracker.is_pending());
        assert_eq!(
            tracker.observe_snapshot(
                settled_teleport_snapshot(),
                started + Duration::from_secs(1)
            ),
            None,
            "clean work settled before the matching publisher update"
        );

        tracker.observe(
            &WorldEvent::PublisherUpdate(protocol::PublisherUpdateEvent {
                center: [1_040, 70, 1_040],
                radius_blocks: 256,
            }),
            started + Duration::from_millis(1_100),
            0,
        );
        assert_eq!(
            tracker.observe_snapshot(
                settled_teleport_snapshot(),
                started + Duration::from_millis(1_200),
            ),
            None
        );
        tracker.observe(
            &WorldEvent::LevelChunk(LevelChunkEvent {
                dimension: 0,
                x: 65,
                z: 65,
                mode: LevelChunkMode::LimitlessRequests,
                payload: Vec::new(),
            }),
            started + Duration::from_millis(1_150),
            0,
        );
        tracker.observe(
            &WorldEvent::SubChunks(protocol::SubChunkBatchEvent {
                dimension: 0,
                entries: vec![SubChunkEntryEvent {
                    position: [65, -4, 65],
                    result: SubChunkResult::AllAir,
                }],
            }),
            started + Duration::from_millis(1_300),
            0,
        );
        tracker.observe(
            &WorldEvent::SubChunks(protocol::SubChunkBatchEvent {
                dimension: 0,
                entries: vec![SubChunkEntryEvent {
                    position: [65, -4, 65],
                    result: SubChunkResult::AllAir,
                }],
            }),
            started + Duration::from_millis(1_500),
            0,
        );
        tracker.observe(
            &WorldEvent::PublisherUpdate(protocol::PublisherUpdateEvent {
                center: [1_040, 70, 1_040],
                radius_blocks: 256,
            }),
            started + Duration::from_millis(1_600),
            0,
        );
        let mut clean = settled_teleport_snapshot();
        clean.last_chunk_commit_at = Some(started + Duration::from_millis(1_650));
        clean.last_mesh_dispatch_at = Some(started + Duration::from_millis(1_700));
        clean.last_mesh_completion_at = Some(started + Duration::from_millis(1_800));
        clean.last_mesh_ack_at = Some(started + Duration::from_millis(1_900));
        let mut busy = clean;
        busy.work.network_events = 4;
        busy.work.pending_mesh_jobs = 1;
        assert_eq!(
            tracker.observe_snapshot(busy, started + Duration::from_secs(2)),
            None,
            "late work did not reset the clean candidate"
        );
        assert_eq!(
            tracker.observe_snapshot(clean, started + Duration::from_millis(2_100),),
            None
        );
        let expectation = tracker
            .pending
            .as_ref()
            .and_then(|pending| pending.presented_candidate.as_ref())
            .map(|candidate| candidate.expectation.clone())
            .expect("clean exact target should freeze a render expectation");
        assert_eq!(
            tracker.observe_presented_frame(presented_acknowledgement(
                &expectation,
                10,
                Duration::from_millis(10),
                Duration::from_millis(20),
            )),
            None
        );
        let completion = tracker
            .observe_presented_frame(presented_acknowledgement(
                &expectation,
                11,
                Duration::from_millis(30),
                Duration::from_millis(40),
            ))
            .expect("the adjacent second exact GPU-complete frame should complete");
        assert_eq!(completion.settle_latency, Duration::from_millis(2_140));
        assert_eq!(
            completion.publisher_latency,
            Some(Duration::from_millis(1_100))
        );
        assert_eq!(
            completion.first_level_chunk_latency,
            Some(Duration::from_millis(1_150))
        );
        assert_eq!(
            completion.last_level_chunk_latency,
            Some(Duration::from_millis(1_150))
        );
        assert_eq!(completion.level_chunk_events, 1);
        assert_eq!(
            completion.first_sub_chunk_latency,
            Some(Duration::from_millis(1_300))
        );
        assert_eq!(
            completion.last_sub_chunk_latency,
            Some(Duration::from_millis(1_500))
        );
        assert_eq!(completion.sub_chunk_events, 2);
        assert_eq!(
            completion.last_chunk_commit_latency,
            Some(Duration::from_millis(1_650))
        );
        assert_eq!(
            completion.last_mesh_dispatch_latency,
            Some(Duration::from_millis(1_700))
        );
        assert_eq!(
            completion.last_mesh_completion_latency,
            Some(Duration::from_millis(1_800))
        );
        assert_eq!(
            completion.last_mesh_ack_latency,
            Some(Duration::from_millis(1_900))
        );
        assert_eq!(completion.peak_network_events, 4);
    }

    #[test]
    fn partial_target_column_coverage_never_settles_the_teleport_stream() {
        let started = Instant::now();
        let mut tracker = FullViewTeleportTracker::new(true);
        tracker.set_source_mutation_coordinate([0, 58, 0]);
        tracker.begin_world_ready([0.5, 70.0, 0.5], 1);
        tracker.observe(
            &WorldEvent::MovePlayer(protocol::MovePlayerEvent {
                runtime_id: 1,
                position: [1_040.5, 70.0, 1_040.5],
                pitch: 0.0,
                yaw: 0.0,
            }),
            started,
            0,
        );
        tracker.observe(
            &WorldEvent::PublisherUpdate(protocol::PublisherUpdateEvent {
                center: [1_040, 70, 1_040],
                radius_blocks: 256,
            }),
            started + Duration::from_millis(100),
            0,
        );
        let mut partial = settled_teleport_snapshot();
        partial.loaded_columns = 999;
        let status = partial.cohort.as_mut().unwrap();
        status.loaded_target = 999;
        status.missing_target = status.expected - status.loaded_target;

        assert_eq!(
            tracker.observe_snapshot(partial, started + Duration::from_millis(200)),
            None
        );
        assert_eq!(
            tracker.observe_snapshot(partial, started + Duration::from_secs(5)),
            None,
            "a quiet partial target view passed the coverage gate"
        );
        assert!(tracker.is_pending());
    }

    #[test]
    fn forced_remesh_starts_only_after_binding_teleport_completion() {
        let teleport_started = Instant::now();
        let binding = binding_teleport_completion(teleport_started, Duration::from_millis(1_500));
        let started = teleport_started + Duration::from_millis(1_501);
        let key = SubChunkKey::new(0, 64, 65, 65);
        let manifest = ForcedRemeshManifest {
            started_at: started,
            entries: Arc::from([(key, 8)]),
        };
        let mut tracker = FullViewRemeshTracker::default();

        assert!(!tracker.start(None, exact_destination_status(), manifest.clone(), 90,));
        assert!(tracker.start(Some(&binding), exact_destination_status(), manifest, 90,));
        assert!(tracker.is_pending());
    }

    #[test]
    fn fast_forced_remesh_does_not_replace_or_fix_a_slow_binding_teleport() {
        let teleport_started = Instant::now();
        let binding = binding_teleport_completion(teleport_started, Duration::from_millis(2_400));
        let started = teleport_started + Duration::from_millis(2_401);
        let key = SubChunkKey::new(0, 64, 65, 65);
        let manifest = ForcedRemeshManifest {
            started_at: started,
            entries: Arc::from([(key, 8)]),
        };
        let mut tracker = FullViewRemeshTracker::default();
        assert!(tracker.start(Some(&binding), exact_destination_status(), manifest, 145,));
        let expectation = tracker
            .reconcile_presented_expectation(
                settled_teleport_snapshot(),
                ForcedRemeshManifestState::Complete,
                Some(proposed_render_expectation(
                    started + Duration::from_millis(10),
                    [(key, 8)],
                )),
                started + Duration::from_millis(10),
                146,
            )
            .unwrap();
        assert_eq!(
            tracker.observe_presented_frame(
                presented_acknowledgement(
                    &expectation,
                    43,
                    Duration::from_millis(20),
                    Duration::from_millis(40),
                ),
                148,
            ),
            None
        );
        let forced = tracker
            .observe_presented_frame(
                presented_acknowledgement(
                    &expectation,
                    44,
                    Duration::from_millis(80),
                    Duration::from_millis(90),
                ),
                151,
            )
            .unwrap();

        assert_eq!(binding.settle_latency, Duration::from_millis(2_400));
        assert_eq!(forced.settle_latency, Duration::from_millis(100));
        assert!(binding.settle_latency > Duration::from_secs(2));
        assert!(forced.settle_latency < Duration::from_secs(2));
    }

    #[test]
    fn forced_remesh_busy_gap_resets_the_presented_pair() {
        let teleport_started = Instant::now();
        let binding = binding_teleport_completion(teleport_started, Duration::from_millis(1_500));
        let started = teleport_started + Duration::from_millis(1_501);
        let key = SubChunkKey::new(0, 64, 65, 65);
        let manifest = ForcedRemeshManifest {
            started_at: started,
            entries: Arc::from([(key, 8)]),
        };
        let mut tracker = FullViewRemeshTracker::default();
        assert!(tracker.start(Some(&binding), exact_destination_status(), manifest, 90,));
        let first_expectation = tracker
            .reconcile_presented_expectation(
                settled_teleport_snapshot(),
                ForcedRemeshManifestState::Complete,
                Some(proposed_render_expectation(
                    started + Duration::from_millis(10),
                    [(key, 8)],
                )),
                started + Duration::from_millis(10),
                91,
            )
            .unwrap();
        assert_eq!(
            tracker.observe_presented_frame(
                presented_acknowledgement(
                    &first_expectation,
                    43,
                    Duration::from_millis(20),
                    Duration::from_millis(40),
                ),
                92,
            ),
            None
        );

        let mut busy = settled_teleport_snapshot();
        busy.work.pending_mesh_jobs = 1;
        assert_eq!(
            tracker.reconcile_presented_expectation(
                busy,
                ForcedRemeshManifestState::Complete,
                Some(proposed_render_expectation(
                    started + Duration::from_millis(50),
                    [(key, 8)],
                )),
                started + Duration::from_millis(50),
                93,
            ),
            None
        );

        let resumed_expectation = tracker
            .reconcile_presented_expectation(
                settled_teleport_snapshot(),
                ForcedRemeshManifestState::Complete,
                Some(proposed_render_expectation(
                    started + Duration::from_millis(60),
                    [(key, 8)],
                )),
                started + Duration::from_millis(60),
                94,
            )
            .unwrap();
        assert_ne!(
            resumed_expectation.render_ready_at, first_expectation.render_ready_at,
            "resumed proof reused the pre-gap render-ready boundary"
        );
        assert_eq!(
            tracker.observe_presented_frame(
                presented_acknowledgement(
                    &resumed_expectation,
                    44,
                    Duration::from_millis(20),
                    Duration::from_millis(40),
                ),
                95,
            ),
            None,
            "the first post-gap exact frame paired with the pre-gap frame"
        );
    }

    #[test]
    fn cohort_or_manifest_change_invalidates_forced_remesh() {
        let teleport_started = Instant::now();
        let binding = binding_teleport_completion(teleport_started, Duration::from_millis(1_500));
        let started = teleport_started + Duration::from_millis(1_501);
        let key = SubChunkKey::new(0, 64, 65, 65);
        let manifest = ForcedRemeshManifest {
            started_at: started,
            entries: Arc::from([(key, 8)]),
        };
        let proposal = proposed_render_expectation(started + Duration::from_millis(10), [(key, 8)]);

        let mut cohort_changed = FullViewRemeshTracker::default();
        assert!(cohort_changed.start(
            Some(&binding),
            exact_destination_status(),
            manifest.clone(),
            90,
        ));
        assert!(
            cohort_changed
                .reconcile_presented_expectation(
                    settled_teleport_snapshot(),
                    ForcedRemeshManifestState::Complete,
                    Some(proposal.clone()),
                    started + Duration::from_millis(10),
                    91,
                )
                .is_some()
        );
        let mut replacement = settled_teleport_snapshot();
        replacement.cohort.as_mut().unwrap().resident_hash ^= 0x55aa;
        assert!(
            cohort_changed
                .reconcile_presented_expectation(
                    replacement,
                    ForcedRemeshManifestState::Complete,
                    Some(proposal.clone()),
                    started + Duration::from_millis(20),
                    92,
                )
                .is_none()
        );
        assert!(cohort_changed.is_invalidated());

        let mut manifest_changed = FullViewRemeshTracker::default();
        assert!(manifest_changed.start(Some(&binding), exact_destination_status(), manifest, 90,));
        assert!(
            manifest_changed
                .reconcile_presented_expectation(
                    settled_teleport_snapshot(),
                    ForcedRemeshManifestState::Complete,
                    Some(proposal),
                    started + Duration::from_millis(10),
                    91,
                )
                .is_some()
        );
        assert!(
            manifest_changed
                .reconcile_presented_expectation(
                    settled_teleport_snapshot(),
                    ForcedRemeshManifestState::Complete,
                    Some(proposed_render_expectation(
                        started + Duration::from_millis(20),
                        [(key, 9)],
                    )),
                    started + Duration::from_millis(20),
                    92,
                )
                .is_none()
        );
        assert!(manifest_changed.is_invalidated());

        let mut presented_changed = FullViewRemeshTracker::default();
        let manifest = ForcedRemeshManifest {
            started_at: started,
            entries: Arc::from([(key, 8)]),
        };
        assert!(presented_changed.start(Some(&binding), exact_destination_status(), manifest, 90,));
        let expectation = presented_changed
            .reconcile_presented_expectation(
                settled_teleport_snapshot(),
                ForcedRemeshManifestState::Complete,
                Some(proposed_render_expectation(
                    started + Duration::from_millis(10),
                    [(key, 8)],
                )),
                started + Duration::from_millis(10),
                91,
            )
            .unwrap();
        let mut changed_ack = presented_acknowledgement(
            &expectation,
            43,
            Duration::from_millis(20),
            Duration::from_millis(40),
        );
        changed_ack.allocation_manifest = Arc::from([(key, 9)]);
        changed_ack.drawn_manifest = Arc::clone(&changed_ack.allocation_manifest);
        assert_eq!(
            presented_changed.observe_presented_frame(changed_ack, 92),
            None
        );
        assert!(
            presented_changed.is_invalidated(),
            "a forced-generation change in presented evidence did not invalidate the benchmark"
        );
    }

    #[test]
    fn deterministic_mutation_coordinate_is_visible_above_the_surface_anchor() {
        assert_eq!(
            deterministic_mutation_coordinate([10.5, 72.62, -5.5], [10, -6]),
            [14, 71, -6]
        );
    }

    #[test]
    fn world_ready_markers_require_radius_rendering_and_include_the_exact_coordinate() {
        let mut snapshot = settled_world_snapshot();
        snapshot.received_radius_chunks = Some(15);
        assert_eq!(world_ready_markers(snapshot), None);
        snapshot.received_radius_chunks = Some(16);
        snapshot.publisher_radius_chunks = Some(15);
        assert_eq!(world_ready_markers(snapshot), None);
        snapshot.publisher_radius_chunks = Some(16);
        snapshot.rendered_sub_chunks = 0;
        assert_eq!(world_ready_markers(snapshot), None);
        snapshot.rendered_sub_chunks = 2;
        assert_eq!(
            world_ready_markers(snapshot),
            Some([
                "RUST_MCBE_MUTATION_COORDINATE=14,71,-6".to_owned(),
                "RUST_MCBE_WORLD_READY radius=16 rendered=2 resident=3 visible=1".to_owned(),
            ])
        );
    }

    #[test]
    fn gallery_anchor_is_one_shot_mode_scoped_and_only_requires_the_clean_rendered_target() {
        let mut emitter = GalleryAnchorEmitter::default();
        let mut snapshot = settled_world_snapshot();
        snapshot.received_radius_chunks = None;
        snapshot.publisher_radius_chunks = None;
        snapshot.resident_sub_chunks = 0;
        snapshot.visible_sub_chunks = 0;
        snapshot.work.pending_mesh_jobs = 99;

        assert_eq!(emitter.observe(false, snapshot), None);

        snapshot.mutation_target_rendered = false;
        assert_eq!(emitter.observe(true, snapshot), None);
        snapshot.mutation_target_rendered = true;
        snapshot.mutation_target_visible = false;
        snapshot.mutation_target_clean = false;
        assert_eq!(emitter.observe(true, snapshot), None);
        snapshot.mutation_target_clean = true;

        assert_eq!(
            emitter.observe(true, snapshot),
            Some(
                "RUST_MCBE_GALLERY_ANCHOR_READY coordinate=14,71,-6 rendered=true visible=false clean=true"
                    .to_owned()
            )
        );
        assert_eq!(emitter.observe(true, snapshot), None);
    }

    #[test]
    fn world_ready_markers_are_withheld_for_every_pending_stage_and_an_unclean_target() {
        let pending_stages = [
            (
                "network ingress",
                WorldReadyWork {
                    network_events: 1,
                    ..Default::default()
                },
            ),
            (
                "network commands",
                WorldReadyWork {
                    network_commands: 1,
                    ..Default::default()
                },
            ),
            (
                "admitted world events",
                WorldReadyWork {
                    admitted_world_events: 1,
                    ..Default::default()
                },
            ),
            (
                "queued decode",
                WorldReadyWork {
                    queued_decode_jobs: 1,
                    ..Default::default()
                },
            ),
            (
                "in-flight decode",
                WorldReadyWork {
                    in_flight_decode_jobs: 1,
                    ..Default::default()
                },
            ),
            (
                "completed decode",
                WorldReadyWork {
                    completed_decode_results: 1,
                    ..Default::default()
                },
            ),
            (
                "pending mesh",
                WorldReadyWork {
                    pending_mesh_jobs: 1,
                    ..Default::default()
                },
            ),
            (
                "in-flight mesh",
                WorldReadyWork {
                    in_flight_mesh_jobs: 1,
                    ..Default::default()
                },
            ),
            (
                "mesh changes",
                WorldReadyWork {
                    pending_mesh_changes: 1,
                    ..Default::default()
                },
            ),
            (
                "outbound requests",
                WorldReadyWork {
                    outbound_requests: 1,
                    ..Default::default()
                },
            ),
            (
                "outstanding sub-chunks",
                WorldReadyWork {
                    outstanding_sub_chunks: 1,
                    ..Default::default()
                },
            ),
            (
                "retry requests",
                WorldReadyWork {
                    pending_retry_requests: 1,
                    ..Default::default()
                },
            ),
            (
                "render queue",
                WorldReadyWork {
                    render_queue_items: 1,
                    ..Default::default()
                },
            ),
            (
                "GPU acknowledgements",
                WorldReadyWork {
                    pending_gpu_acknowledgements: 1,
                    ..Default::default()
                },
            ),
            (
                "unacknowledged meshes",
                WorldReadyWork {
                    unacknowledged_meshes: 1,
                    ..Default::default()
                },
            ),
        ];
        for (stage, work) in pending_stages {
            let mut snapshot = settled_world_snapshot();
            snapshot.work = work;
            assert_eq!(world_ready_markers(snapshot), None, "pending {stage}");
        }

        let mut target_not_rendered = settled_world_snapshot();
        target_not_rendered.mutation_target_rendered = false;
        assert_eq!(world_ready_markers(target_not_rendered), None);

        let mut target_not_visible = settled_world_snapshot();
        target_not_visible.mutation_target_visible = false;
        assert_eq!(world_ready_markers(target_not_visible), None);

        let mut target_not_clean = settled_world_snapshot();
        target_not_clean.mutation_target_clean = false;
        assert_eq!(world_ready_markers(target_not_clean), None);
    }

    #[test]
    fn world_ready_requires_a_stable_quiet_interval_and_resets_when_work_reappears() {
        let started = Instant::now();
        let snapshot = settled_world_snapshot();
        let mut settler = WorldReadySettler::default();

        assert_eq!(settler.observe(snapshot, started), None);
        assert_eq!(
            settler.observe(
                snapshot,
                started + WORLD_READY_QUIET_INTERVAL - Duration::from_millis(1)
            ),
            None
        );

        let mut busy = snapshot;
        busy.work.pending_mesh_jobs = 1;
        assert_eq!(
            settler.observe(busy, started + WORLD_READY_QUIET_INTERVAL),
            None
        );

        let restarted = started + WORLD_READY_QUIET_INTERVAL + Duration::from_millis(1);
        assert_eq!(settler.observe(snapshot, restarted), None);
        let mut changed = snapshot;
        changed.rendered_sub_chunks += 1;
        assert_eq!(
            settler.observe(changed, restarted + WORLD_READY_QUIET_INTERVAL),
            None,
            "a changing candidate is not yet stable"
        );
        assert_eq!(
            settler.observe(changed, restarted + WORLD_READY_QUIET_INTERVAL * 2),
            world_ready_markers(changed)
        );
    }

    #[test]
    fn mutation_tracker_closes_latency_only_on_the_target_gpu_acknowledgement() {
        let coordinate = [14, 71, -6];
        let observed_at = Instant::now();
        let mut tracker = MutationTracker::armed(coordinate, observed_at);
        let target_update = WorldEvent::BlockUpdates(vec![BlockUpdateEvent {
            dimension: 0,
            position: coordinate,
            layer: 0,
            network_id: 7,
        }]);
        assert!(tracker.observe(&target_update, observed_at));

        let target_key = SubChunkKey::new(0, 0, 4, -1);
        assert_eq!(
            tracker.acknowledge(
                SubChunkKey::new(0, 1, 4, -1),
                observed_at,
                observed_at + Duration::from_millis(25),
            ),
            None
        );
        assert_eq!(
            tracker.acknowledge(
                target_key,
                observed_at - Duration::from_millis(1),
                observed_at + Duration::from_millis(25),
            ),
            None
        );
        assert_eq!(
            tracker.acknowledge(
                target_key,
                observed_at + Duration::from_millis(1),
                observed_at + Duration::from_millis(75),
            ),
            Some(Duration::from_millis(75))
        );
        assert_eq!(tracker.visible_count(), 1);
    }

    #[test]
    fn full_view_mutation_closes_only_on_the_target_presented_generation() {
        let coordinate = [1_040, 58, 1_052];
        let key = SubChunkKey::new(0, 65, 3, 65);
        let armed_at = Instant::now();
        let observed_at = armed_at + Duration::from_millis(10);
        let render_ready_at = armed_at + Duration::from_millis(20);
        let mut tracker = MutationTracker::armed(coordinate, armed_at);
        let source_update = WorldEvent::BlockUpdates(vec![BlockUpdateEvent {
            dimension: 0,
            position: [4, 70, -2],
            layer: 0,
            network_id: 7,
        }]);
        let target_update = WorldEvent::BlockUpdates(vec![BlockUpdateEvent {
            dimension: 0,
            position: coordinate,
            layer: 0,
            network_id: 7,
        }]);

        assert!(!tracker.observe(&source_update, observed_at));
        assert!(!tracker.observe(&target_update, armed_at - Duration::from_millis(1)));
        assert!(tracker.observe(&target_update, observed_at));
        assert_eq!(
            tracker.acknowledge_upload(
                key,
                77,
                observed_at,
                observed_at + Duration::from_millis(5),
                true,
            ),
            None,
            "an upload acknowledgement settled a full-view mutation before presentation"
        );

        let expectation = tracker
            .reconcile_presented_expectation(
                proposed_render_expectation(render_ready_at, [(key, 77)]),
                8,
                render_ready_at,
            )
            .expect("the uploaded target generation should freeze an exact expectation");
        assert_eq!(expectation.view_generation, 9);
        let mut wrong_generation = presented_acknowledgement(
            &expectation,
            90,
            Duration::from_millis(10),
            Duration::from_millis(20),
        );
        wrong_generation.allocation_manifest = Arc::from([(key, 76)]);
        wrong_generation.drawn_manifest = Arc::from([(key, 76)]);
        assert_eq!(tracker.observe_presented_frame(wrong_generation), None);

        let latency = tracker
            .observe_presented_frame(presented_acknowledgement(
                &expectation,
                91,
                Duration::from_millis(10),
                Duration::from_millis(20),
            ))
            .expect("the exact target generation should close on GPU-completed presentation");
        assert_eq!(latency, Duration::from_millis(30));
        assert_eq!(tracker.visible_count(), 1);
    }

    #[test]
    fn full_outbound_queue_retries_the_same_request_then_preserves_fifo_order() {
        let mut stream = crate::world_stream::WorldStream::new(WorldBootstrap {
            dimension: 0,
            local_player_runtime_id: 1,
            player_position: [0.0; 3],
            world_spawn_position: [0; 3],
            air_network_id: 12_530,
            block_network_ids_are_hashes: false,
        });
        for (sequence, x) in [(1, 0), (2, 1), (3, 2)] {
            stream
                .submit(
                    sequence,
                    WorldEvent::LevelChunk(LevelChunkEvent {
                        dimension: 0,
                        x,
                        z: 0,
                        mode: LevelChunkMode::LimitedRequests { highest: 1 },
                        payload: overworld_biome_payload(),
                    }),
                )
                .unwrap();
        }
        complete_world_stream_decodes(&mut stream);

        let mut attempts = Vec::new();
        let mut calls = 0;
        let sent = flush_sub_chunk_requests(&mut stream, 8, |chunk, _, _, packet| {
            attempts.push(chunk.x);
            calls += 1;
            if calls == 2 {
                Err(crate::network::PacketSendError::Full(packet))
            } else {
                Ok(())
            }
        })
        .unwrap();
        assert_eq!(sent, 1);
        assert_eq!(stream.pending_request_count(), 2);
        stream.acknowledge_sub_chunk_request_sent(
            SubChunkKey::new(0, 0, -4, 0).chunk(),
            -4,
            1,
            Instant::now(),
        );
        assert_eq!(stream.stats().awaiting_sub_chunk_responses, 1);

        let sent = flush_sub_chunk_requests(&mut stream, 8, |chunk, _, _, _packet| {
            attempts.push(chunk.x);
            Ok(())
        })
        .unwrap();
        assert_eq!(sent, 2);
        assert_eq!(attempts, [0, 1, 1, 2]);
        assert_eq!(stream.pending_request_count(), 0);
        for x in [1, 2] {
            stream.acknowledge_sub_chunk_request_sent(
                SubChunkKey::new(0, x, -4, 0).chunk(),
                -4,
                1,
                Instant::now(),
            );
        }
        assert_eq!(stream.stats().awaiting_sub_chunk_responses, 3);
    }

    #[test]
    fn command_admission_leaves_deadline_unarmed_until_transport_success_acknowledgement() {
        let request_stream = || {
            let mut stream = crate::world_stream::WorldStream::new(WorldBootstrap {
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
                        payload: overworld_biome_payload(),
                    }),
                )
                .unwrap();
            complete_world_stream_decodes(&mut stream);
            stream
        };
        let key = SubChunkKey::new(0, 0, -4, 0);
        let mut stream = request_stream();

        assert_eq!(
            flush_sub_chunk_requests(&mut stream, 1, |_, _, _, _| Ok(())).unwrap(),
            1
        );
        assert_eq!(stream.stats().awaiting_sub_chunk_responses, 0);
        let acknowledged_at = Instant::now() + Duration::from_secs(100);

        stream.acknowledge_sub_chunk_request_sent(key.chunk(), key.y, 1, acknowledged_at);
        assert_eq!(stream.stats().awaiting_sub_chunk_responses, 1);

        let mut failed = request_stream();
        assert_eq!(
            flush_sub_chunk_requests(&mut failed, 1, |_, _, _, packet| {
                Err(crate::network::PacketSendError::Full(packet))
            })
            .unwrap(),
            0
        );
        assert_eq!(failed.stats().awaiting_sub_chunk_responses, 0);
        assert_eq!(failed.stats().sub_chunk_timeouts, 0);
    }

    #[test]
    fn network_session_fatal_is_retained_when_command_sender_closes_in_same_frame() {
        let mut stream = crate::world_stream::WorldStream::new(WorldBootstrap {
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
                    payload: overworld_biome_payload(),
                }),
            )
            .unwrap();
        complete_world_stream_decodes(&mut stream);
        let original = "network session failed: Protocol error: original fatal";
        let mut fatal_error = None;
        record_fatal_error(&mut fatal_error, original.to_owned());

        let closed = flush_sub_chunk_requests(&mut stream, 1, |_, _, _, packet| {
            Err(crate::network::PacketSendError::Closed(packet))
        })
        .unwrap_err();
        record_fatal_error(&mut fatal_error, closed);

        assert_eq!(fatal_error.as_deref(), Some(original));
        assert_eq!(stream.pending_request_count(), 1);
        assert_eq!(stream.stats().awaiting_sub_chunk_responses, 0);
    }

    #[test]
    fn zero_world_admission_still_drains_control_ack_and_leaves_world_fifo_untouched() {
        let (control_sender, mut control_receiver) = tokio::sync::mpsc::channel(1);
        let sent_at = Instant::now();
        control_sender
            .try_send(NetworkControlEvent::SubChunkRequestSent {
                chunk: ChunkKey::new(0, 3, -2),
                base_sub_chunk_y: -4,
                count: 24,
                sent_at,
            })
            .unwrap();
        let (world_sender, mut world_receiver) = tokio::sync::mpsc::channel(1);
        world_sender
            .try_send(SequencedWorldEvent {
                sequence: 1,
                event: WorldEvent::ChunkRadiusUpdated(16),
            })
            .unwrap();

        let controls =
            drain_network_controls(&mut control_receiver, OUTBOUND_SEND_BUDGET_PER_FRAME);
        let world =
            drain_network_ingress(&mut world_receiver, NETWORK_INGRESS_BUDGET_PER_FRAME.min(0));

        assert!(matches!(
            controls.as_slice(),
            [NetworkControlEvent::SubChunkRequestSent {
                chunk,
                base_sub_chunk_y: -4,
                count: 24,
                sent_at: observed,
            }] if *chunk == ChunkKey::new(0, 3, -2) && *observed == sent_at
        ));
        assert!(world.is_empty());
        assert!(matches!(
            world_receiver.try_recv(),
            Ok(SequencedWorldEvent {
                sequence: 1,
                event: WorldEvent::ChunkRadiusUpdated(16),
            })
        ));
    }

    #[test]
    fn control_ingress_is_bounded_to_outbound_budget_and_preserves_fifo() {
        assert_eq!(OUTBOUND_SEND_BUDGET_PER_FRAME, 16);
        let (sender, mut receiver) = tokio::sync::mpsc::channel(OUTBOUND_SEND_BUDGET_PER_FRAME + 2);
        for value in 0..OUTBOUND_SEND_BUDGET_PER_FRAME + 2 {
            sender.try_send(value).unwrap();
        }

        let drained = drain_network_controls(&mut receiver, OUTBOUND_SEND_BUDGET_PER_FRAME);

        assert_eq!(
            drained,
            (0..OUTBOUND_SEND_BUDGET_PER_FRAME).collect::<Vec<_>>()
        );
        assert_eq!(receiver.try_recv(), Ok(OUTBOUND_SEND_BUDGET_PER_FRAME));
        assert_eq!(receiver.try_recv(), Ok(OUTBOUND_SEND_BUDGET_PER_FRAME + 1));
    }

    #[test]
    fn world_ingress_is_bounded_to_eight_and_preserves_fifo() {
        assert_eq!(NETWORK_INGRESS_BUDGET_PER_FRAME, 8);
        let (sender, mut receiver) =
            tokio::sync::mpsc::channel(NETWORK_INGRESS_BUDGET_PER_FRAME + 2);
        for value in 0..NETWORK_INGRESS_BUDGET_PER_FRAME + 2 {
            sender.try_send(value).unwrap();
        }

        let drained = drain_network_ingress(&mut receiver, NETWORK_INGRESS_BUDGET_PER_FRAME);

        assert_eq!(
            drained,
            (0..NETWORK_INGRESS_BUDGET_PER_FRAME).collect::<Vec<_>>()
        );
        assert_eq!(receiver.try_recv(), Ok(NETWORK_INGRESS_BUDGET_PER_FRAME));
        assert_eq!(
            receiver.try_recv(),
            Ok(NETWORK_INGRESS_BUDGET_PER_FRAME + 1)
        );
    }

    #[test]
    fn camera_sub_chunk_key_uses_floor_and_euclidean_chunks() {
        assert_eq!(
            camera_sub_chunk_key(2, Vec3::new(-0.1, -64.1, 16.0)),
            SubChunkKey::new(2, -1, -5, 1)
        );
    }

    #[test]
    fn status_title_exposes_live_input_coordinates_for_acceptance() {
        let transform = Transform {
            translation: Vec3::new(1.25, 72.0, -8.5),
            rotation: Quat::from_rotation_y(0.5),
            ..Default::default()
        };
        let title = status_title(&transform, 42, 37, true, 59.94);

        assert!(title.contains("59.9 FPS"));
        assert!(title.contains("pos 1.25 72.00 -8.50"));
        assert!(title.contains("yaw 0.50"));
        assert!(title.contains("chunks 37/42"));
        assert!(title.contains("captured"));
    }

    #[test]
    fn rolling_fps_uses_only_the_most_recent_second() {
        let mut fps = RollingFps::default();
        for _ in 0..60 {
            fps.record(Duration::from_secs_f64(1.0 / 60.0));
        }
        assert!((fps.value() - 60.0).abs() < 0.01);

        for _ in 0..30 {
            fps.record(Duration::from_secs_f64(1.0 / 30.0));
        }
        assert!((fps.value() - 30.0).abs() < 0.01);
    }

    #[test]
    fn cumulative_counter_delta_tolerates_a_counter_reset() {
        assert_eq!(cumulative_counter_delta(9, 4), 5);
        assert_eq!(cumulative_counter_delta(2, 9), 2);
    }

    #[test]
    fn bedrock_yaw_and_pitch_map_to_bevys_negative_z_camera() {
        let south = bedrock_camera_rotation(0.0, 0.0) * Vec3::NEG_Z;
        let west = bedrock_camera_rotation(90.0, 0.0) * Vec3::NEG_Z;
        let looking_down = bedrock_camera_rotation(180.0, 45.0) * Vec3::NEG_Z;

        assert!(south.abs_diff_eq(Vec3::Z, 0.0001));
        assert!(west.abs_diff_eq(Vec3::NEG_X, 0.0001));
        assert!(looking_down.y < -0.7);
    }

    #[test]
    fn relative_socket_dir_falls_back_to_the_development_project_root() {
        let unique = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_nanos();
        let root = std::env::temp_dir().join(format!(
            "rust-mcbe-socket-resolution-{}-{unique}",
            std::process::id()
        ));
        let current_dir = root.join("launcher");
        let executable = root.join("project/target/debug/bedrock-client.exe");
        let expected = root.join("project/.local/run");
        std::fs::create_dir_all(&current_dir).unwrap();
        std::fs::create_dir_all(&expected).unwrap();
        std::fs::write(expected.join("game.addr"), "127.0.0.1:19132\n").unwrap();

        assert_eq!(
            resolve_socket_dir_from(
                std::path::Path::new(".local/run"),
                &current_dir,
                &executable,
            ),
            expected
        );

        let _ = std::fs::remove_dir_all(root);
    }
}
