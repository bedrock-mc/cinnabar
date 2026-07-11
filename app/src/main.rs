mod args;
mod asset_startup;
mod camera;
mod culling;
mod metrics;
mod network;
mod server_position;
mod world_stream;

use std::{
    collections::{BTreeSet, HashSet},
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
use metrics::{DiagnosticQuadTracker, MetricsCollector, PipelineMetricsSnapshot};
use network::{NetworkConfig, NetworkEvent, NetworkHandle, spawn_network};
use render::{
    ChunkRenderInstance, ChunkRenderQueue, ChunkTextureAssets, ChunkUploadAcknowledgements,
    ChunkUploadPriority, ChunkUploadToken, DebugWorldPlugin,
};
use server_position::SAFE_SERVER_HEIGHT;
use world::SubChunkKey;
use world_stream::{CommittedControlEvent, WorldMeshChange, WorldStream};

const MESH_JOB_BUDGET_PER_FRAME: usize = 128;
const GPU_UPLOAD_BUDGET_PER_FRAME: usize = 128;
const NETWORK_INGRESS_BUDGET_PER_FRAME: usize = 8;
const OUTBOUND_SEND_BUDGET_PER_FRAME: usize = 16;
const TITLE_REFRESH_INTERVAL: Duration = Duration::from_millis(100);
const WORLD_READY_QUIET_INTERVAL: Duration = Duration::from_secs(2);
const PHASE0_REQUESTED_RADIUS_CHUNKS: i32 = 16;
const MUTATION_X_OFFSET_BLOCKS: i32 = 4;
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

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
struct TeleportReadySnapshot {
    received_radius_chunks: Option<i32>,
    publisher_radius_chunks: Option<i32>,
    rendered_sub_chunks: usize,
    resident_sub_chunks: usize,
    visible_sub_chunks: usize,
    loaded_columns: usize,
    last_chunk_commit_at: Option<Instant>,
    last_mesh_dispatch_at: Option<Instant>,
    last_mesh_completion_at: Option<Instant>,
    last_mesh_ack_at: Option<Instant>,
    work: WorldReadyWork,
}

impl TeleportReadySnapshot {
    fn is_ready(self) -> bool {
        self.received_radius_chunks == Some(PHASE0_REQUESTED_RADIUS_CHUNKS)
            && self.publisher_radius_chunks == Some(PHASE0_REQUESTED_RADIUS_CHUNKS)
            && self.rendered_sub_chunks != 0
            && self.resident_sub_chunks != 0
            && self.visible_sub_chunks != 0
            && self.work.is_empty()
    }
}

#[derive(Debug, Clone, Copy)]
struct TeleportCleanCandidate {
    snapshot: TeleportReadySnapshot,
    since: Instant,
    first_clean_latency: Duration,
}

#[derive(Debug, Clone, Copy)]
struct PendingFullViewTeleport {
    started: Instant,
    target_chunk: [i32; 2],
    publisher_seen: bool,
    publisher_latency: Option<Duration>,
    first_level_chunk_latency: Option<Duration>,
    last_level_chunk_latency: Option<Duration>,
    level_chunk_events: u64,
    first_sub_chunk_latency: Option<Duration>,
    last_sub_chunk_latency: Option<Duration>,
    sub_chunk_events: u64,
    peak_network_events: usize,
    clean_candidate: Option<TeleportCleanCandidate>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
struct FullViewTeleportCompletion {
    settle_latency: Duration,
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
}

#[derive(Debug)]
struct FullViewTeleportTracker {
    enabled: bool,
    origin_chunk: Option<[i32; 2]>,
    latest_publisher_chunk: Option<[i32; 2]>,
    minimum_loaded_columns: Option<usize>,
    pending: Option<PendingFullViewTeleport>,
    completed: Option<Duration>,
}

impl FullViewTeleportTracker {
    const fn new(enabled: bool) -> Self {
        Self {
            enabled,
            origin_chunk: None,
            latest_publisher_chunk: None,
            minimum_loaded_columns: None,
            pending: None,
            completed: None,
        }
    }

    fn begin_world_ready(&mut self, position: [f32; 3], loaded_columns: usize) {
        if self.enabled {
            self.origin_chunk = horizontal_chunk(position);
            self.minimum_loaded_columns = Some(loaded_columns);
        }
    }

    fn observe(&mut self, event: &protocol::WorldEvent, observed_at: Instant) {
        if !self.enabled || self.completed.is_some() {
            return;
        }
        match event {
            protocol::WorldEvent::PublisherUpdate(update) => {
                let publisher = [
                    update.center[0].div_euclid(16),
                    update.center[2].div_euclid(16),
                ];
                self.latest_publisher_chunk = Some(publisher);
                if let Some(pending) = &mut self.pending
                    && publisher == pending.target_chunk
                    && !pending.publisher_seen
                {
                    pending.publisher_seen = true;
                    pending.publisher_latency =
                        Some(observed_at.saturating_duration_since(pending.started));
                }
            }
            protocol::WorldEvent::MovePlayer(movement) if self.pending.is_none() => {
                let (Some(origin), Some(target)) =
                    (self.origin_chunk, horizontal_chunk(movement.position))
                else {
                    return;
                };
                let far_enough = i64::from(origin[0]).abs_diff(i64::from(target[0]))
                    >= FULL_VIEW_TELEPORT_MIN_CHUNK_DELTA
                    || i64::from(origin[1]).abs_diff(i64::from(target[1]))
                        >= FULL_VIEW_TELEPORT_MIN_CHUNK_DELTA;
                if !far_enough {
                    return;
                }
                self.pending = Some(PendingFullViewTeleport {
                    started: observed_at,
                    target_chunk: target,
                    publisher_seen: self.latest_publisher_chunk == Some(target),
                    publisher_latency: (self.latest_publisher_chunk == Some(target))
                        .then_some(Duration::ZERO),
                    first_level_chunk_latency: None,
                    last_level_chunk_latency: None,
                    level_chunk_events: 0,
                    first_sub_chunk_latency: None,
                    last_sub_chunk_latency: None,
                    sub_chunk_events: 0,
                    peak_network_events: 0,
                    clean_candidate: None,
                });
            }
            protocol::WorldEvent::LevelChunk(_) => {
                if let Some(pending) = &mut self.pending {
                    let latency = observed_at.saturating_duration_since(pending.started);
                    pending.first_level_chunk_latency.get_or_insert(latency);
                    pending.last_level_chunk_latency = Some(latency);
                    pending.level_chunk_events = pending.level_chunk_events.saturating_add(1);
                }
            }
            protocol::WorldEvent::SubChunks(_) => {
                if let Some(pending) = &mut self.pending {
                    let latency = observed_at.saturating_duration_since(pending.started);
                    pending.first_sub_chunk_latency.get_or_insert(latency);
                    pending.last_sub_chunk_latency = Some(latency);
                    pending.sub_chunk_events = pending.sub_chunk_events.saturating_add(1);
                }
            }
            _ => {}
        }
    }

    fn observe_snapshot(
        &mut self,
        snapshot: TeleportReadySnapshot,
        now: Instant,
    ) -> Option<FullViewTeleportCompletion> {
        let completion = {
            let pending = self.pending.as_mut()?;
            pending.peak_network_events = pending
                .peak_network_events
                .max(snapshot.work.network_events);
            if !pending.publisher_seen
                || !snapshot.is_ready()
                || self
                    .minimum_loaded_columns
                    .is_none_or(|minimum| snapshot.loaded_columns < minimum)
            {
                pending.clean_candidate = None;
                return None;
            }
            match pending.clean_candidate {
                Some(candidate) if candidate.snapshot == snapshot => {
                    (now.saturating_duration_since(candidate.since) >= WORLD_READY_QUIET_INTERVAL)
                        .then_some(FullViewTeleportCompletion {
                            settle_latency: candidate.first_clean_latency,
                            publisher_latency: pending.publisher_latency,
                            first_level_chunk_latency: pending.first_level_chunk_latency,
                            last_level_chunk_latency: pending.last_level_chunk_latency,
                            level_chunk_events: pending.level_chunk_events,
                            first_sub_chunk_latency: pending.first_sub_chunk_latency,
                            last_sub_chunk_latency: pending.last_sub_chunk_latency,
                            sub_chunk_events: pending.sub_chunk_events,
                            last_chunk_commit_latency: latency_after(
                                pending.started,
                                snapshot.last_chunk_commit_at,
                            ),
                            last_mesh_dispatch_latency: latency_after(
                                pending.started,
                                snapshot.last_mesh_dispatch_at,
                            ),
                            last_mesh_completion_latency: latency_after(
                                pending.started,
                                snapshot.last_mesh_completion_at,
                            ),
                            last_mesh_ack_latency: latency_after(
                                pending.started,
                                snapshot.last_mesh_ack_at,
                            ),
                            peak_network_events: pending.peak_network_events,
                        })
                }
                _ => {
                    pending.clean_candidate = Some(TeleportCleanCandidate {
                        snapshot,
                        since: now,
                        first_clean_latency: now.saturating_duration_since(pending.started),
                    });
                    None
                }
            }
        };
        if let Some(completion) = completion {
            self.pending = None;
            self.completed = Some(completion.settle_latency);
        }
        completion
    }

    #[cfg(test)]
    const fn is_pending(&self) -> bool {
        self.pending.is_some()
    }
}

fn latency_after(started: Instant, observed: Option<Instant>) -> Option<Duration> {
    observed.and_then(|observed| observed.checked_duration_since(started))
}

#[derive(Debug, Default)]
struct FullViewRemeshTracker {
    pending: Option<(Instant, usize)>,
    queued_sub_chunks: Option<usize>,
    completed: bool,
}

impl FullViewRemeshTracker {
    fn start(&mut self, started: Instant, queued_sub_chunks: usize) -> bool {
        if queued_sub_chunks == 0 || self.pending.is_some() || self.completed {
            return false;
        }
        self.pending = Some((started, queued_sub_chunks));
        self.queued_sub_chunks = Some(queued_sub_chunks);
        true
    }

    fn observe_snapshot(
        &mut self,
        snapshot: TeleportReadySnapshot,
        now: Instant,
    ) -> Option<Duration> {
        if !snapshot.is_ready() {
            return None;
        }
        let (started, _) = self.pending.take()?;
        self.completed = true;
        Some(now.saturating_duration_since(started))
    }

    const fn queued_sub_chunks(&self) -> Option<usize> {
        self.queued_sub_chunks
    }
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
    mutation: Option<MutationTracker>,
    world_ready_settler: WorldReadySettler,
    full_view_teleport: FullViewTeleportTracker,
    full_view_remesh: FullViewRemeshTracker,
    world_ready: bool,
    finished: bool,
}

impl AcceptanceRun {
    fn new(
        seconds: Option<u64>,
        metrics_out: Option<PathBuf>,
        full_view_teleport_gate: bool,
    ) -> Self {
        Self {
            duration: seconds.map(Duration::from_secs),
            deadline: None,
            metrics_out,
            mutation_surface_anchor: None,
            mutation: None,
            world_ready_settler: WorldReadySettler::default(),
            full_view_teleport: FullViewTeleportTracker::new(full_view_teleport_gate),
            full_view_remesh: FullViewRemeshTracker::default(),
            world_ready: false,
            finished: false,
        }
    }

    fn enabled(&self) -> bool {
        self.duration.is_some()
    }

    fn begin_world_ready(&mut self, ready_at: Instant, position: [f32; 3], loaded_columns: usize) {
        self.deadline = self.duration.map(|duration| ready_at + duration);
        self.world_ready = true;
        self.full_view_teleport
            .begin_world_ready(position, loaded_columns);
    }

    fn set_mutation_surface_anchor(&mut self, anchor: [i32; 2]) {
        self.mutation_surface_anchor = Some(anchor);
    }

    fn mutation_surface_anchor(&self) -> Option<[i32; 2]> {
        self.mutation_surface_anchor
    }

    fn set_mutation_coordinate(&mut self, coordinate: [i32; 3]) {
        self.mutation_surface_anchor = None;
        self.mutation = Some(MutationTracker::new(coordinate));
    }

    fn observe_mutation(&mut self, event: &protocol::WorldEvent, observed_at: Instant) {
        if let Some(mutation) = &mut self.mutation {
            mutation.observe(event, observed_at);
        }
    }

    fn observe_full_view_teleport(&mut self, event: &protocol::WorldEvent, observed_at: Instant) {
        if self.world_ready {
            self.full_view_teleport.observe(event, observed_at);
        }
    }

    fn acknowledge_mutation(
        &mut self,
        key: SubChunkKey,
        dirty_since: Instant,
        applied_at: Instant,
    ) -> Option<Duration> {
        self.mutation
            .as_mut()
            .and_then(|mutation| mutation.acknowledge(key, dirty_since, applied_at))
    }

    fn mutation_coordinate(&self) -> Option<[i32; 3]> {
        self.mutation.as_ref().map(MutationTracker::coordinate)
    }

    fn visible_mutation_count(&self) -> u64 {
        self.mutation
            .as_ref()
            .map_or(0, MutationTracker::visible_count)
    }
}

#[derive(Debug, Clone, Copy)]
struct PendingMutation {
    key: SubChunkKey,
    observed_at: Instant,
}

#[derive(Debug)]
struct MutationTracker {
    coordinate: [i32; 3],
    pending: Option<PendingMutation>,
    visible_count: u64,
}

impl MutationTracker {
    const fn new(coordinate: [i32; 3]) -> Self {
        Self {
            coordinate,
            pending: None,
            visible_count: 0,
        }
    }

    const fn coordinate(&self) -> [i32; 3] {
        self.coordinate
    }

    fn observe(&mut self, event: &protocol::WorldEvent, observed_at: Instant) -> bool {
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
        });
        true
    }

    fn acknowledge(
        &mut self,
        key: SubChunkKey,
        dirty_since: Instant,
        applied_at: Instant,
    ) -> Option<Duration> {
        let pending = self.pending?;
        if pending.key != key || dirty_since < pending.observed_at {
            return None;
        }
        self.pending = None;
        self.visible_count = self.visible_count.saturating_add(1);
        Some(applied_at.saturating_duration_since(pending.observed_at))
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

fn status_title(
    camera: &Transform,
    resident_sub_chunks: usize,
    visible_sub_chunks: usize,
    captured: bool,
) -> String {
    let (yaw, pitch, _) = camera.rotation.to_euler(EulerRot::YXZ);
    format!(
        "Rust MCBE | pos {:.2} {:.2} {:.2} | yaw {yaw:.2} pitch {pitch:.2} | chunks {visible_sub_chunks}/{resident_sub_chunks} | {}",
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
            if let Err(error) = run(args) {
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
    .insert_resource(ChunkTextureAssets::new(runtime_assets))
    .insert_resource(CaveVisibilityCache::default())
    .insert_resource(AppMetrics(MetricsCollector::with_asset_metrics(
        asset_metrics,
    )))
    .insert_resource(DiagnosticQuads::default())
    .insert_resource(AcceptanceRun::new(
        args.acceptance_seconds,
        args.metrics_out,
        args.full_view_teleport_gate,
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
            drive_world_stream,
            refresh_cave_visibility,
            emit_world_ready,
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

fn receive_network_events(
    mut network: ResMut<NetworkHandle>,
    mut client_world: ResMut<ClientWorld>,
    mut acceptance: ResMut<AcceptanceRun>,
    acknowledgements: Res<ChunkUploadAcknowledgements>,
    mut cameras: Query<&mut Transform, With<FlyCamera>>,
) {
    let admission_capacity = client_world.stream.as_ref().map_or(
        NETWORK_INGRESS_BUDGET_PER_FRAME,
        WorldStream::remaining_admission_capacity,
    );
    let events = drain_network_ingress(
        network.events_mut(),
        NETWORK_INGRESS_BUDGET_PER_FRAME.min(admission_capacity),
    );
    for event in events {
        match event {
            NetworkEvent::Bootstrap(bootstrap) => {
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
            NetworkEvent::World(sequenced) => {
                let Some(stream) = client_world.stream.as_mut() else {
                    client_world.fatal_error =
                        Some("received world data before StartGame bootstrap".to_owned());
                    continue;
                };
                let observed_at = Instant::now();
                acceptance.observe_mutation(&sequenced.event, observed_at);
                acceptance.observe_full_view_teleport(&sequenced.event, observed_at);
                if let Err(error) = stream.submit(sequenced.sequence, sequenced.event) {
                    client_world.fatal_error = Some(format!("world FIFO rejected data: {error}"));
                }
            }
            NetworkEvent::Failed {
                message,
                decode_error_count,
            } => {
                client_world.network_decode_errors = decode_error_count;
                client_world.fatal_error = Some(format!("network session failed: {message}"));
            }
            NetworkEvent::Stopped { decode_error_count } => {
                client_world.network_decode_errors = decode_error_count;
                if client_world.fatal_error.is_none() {
                    client_world.fatal_error = Some("network session stopped unexpectedly".into());
                }
            }
        }
    }
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
    mut diagnostic_quads: ResMut<DiagnosticQuads>,
    acknowledgements: Res<ChunkUploadAcknowledgements>,
    mut camera: Query<&mut Transform, With<FlyCamera>>,
) {
    let Some(stream) = client_world.stream.as_mut() else {
        return;
    };
    for acknowledgement in acknowledgements.drain() {
        render_queue.record_gpu_upload_bytes(acknowledgement.uploaded_bytes);
        if let Some(latency) = acceptance.acknowledge_mutation(
            acknowledgement.key,
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
        flush_sub_chunk_requests(stream, OUTBOUND_SEND_BUDGET_PER_FRAME, |_, packet| {
            network.send_packet(packet)
        })
        .err()
    });
    if let Some(stream) = client_world.stream.as_mut() {
        while let Some(change) = stream.pop_mesh_change() {
            let retry = match change {
                WorldMeshChange::Upsert {
                    key,
                    mesh,
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
                    match render_queue.try_update_tracked(
                        key,
                        mesh,
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
                        Err(mesh) => Some(WorldMeshChange::Upsert {
                            key,
                            mesh,
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
        client_world.fatal_error = Some(error);
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

#[allow(clippy::too_many_arguments)]
fn emit_world_ready(
    network: Res<NetworkHandle>,
    mut client_world: ResMut<ClientWorld>,
    cache: Res<CaveVisibilityCache>,
    diagnostic_quads: Res<DiagnosticQuads>,
    render_queue: Res<ChunkRenderQueue>,
    acknowledgements: Res<ChunkUploadAcknowledgements>,
    mut acceptance: ResMut<AcceptanceRun>,
    mut auto_fly: ResMut<camera::AutoFly>,
    mut metrics: ResMut<AppMetrics>,
) {
    let missing_mapping_count = client_world.runtime_assets.missing_count();
    let Some(stream) = client_world.stream.as_mut() else {
        return;
    };
    let stats = stream.stats();
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
        let snapshot = TeleportReadySnapshot {
            received_radius_chunks: stats.received_radius_chunks,
            publisher_radius_chunks: stats.publisher_radius_chunks,
            rendered_sub_chunks: cache.rendered.len(),
            resident_sub_chunks: stats.resident_sub_chunks,
            visible_sub_chunks: cache.visible_rendered,
            loaded_columns: stream.loaded_column_count(),
            last_chunk_commit_at: stats.last_chunk_commit_at,
            last_mesh_dispatch_at: stats.last_mesh_dispatch_at,
            last_mesh_completion_at: stats.last_mesh_completion_at,
            last_mesh_ack_at: stats.last_mesh_ack_at,
            work,
        };
        let observed_at = Instant::now();
        if let Some(teleport) = acceptance
            .full_view_teleport
            .observe_snapshot(snapshot, observed_at)
        {
            let remesh_started = Instant::now();
            let queued_sub_chunks = stream.remesh_all_resident(remesh_started);
            if !acceptance
                .full_view_remesh
                .start(remesh_started, queued_sub_chunks)
            {
                error!(queued_sub_chunks, "could not start full-view remesh gate");
                return;
            }
            metrics.0.record_teleport_settle(teleport.settle_latency);
            let mut stdout = std::io::stdout().lock();
            let _ = writeln!(
                stdout,
                "RUST_MCBE_TELEPORT_SETTLED ms={:.4} publisher_ms={:.4} first_level_ms={:.4} last_level_ms={:.4} level_events={} first_sub_ms={:.4} last_sub_ms={:.4} sub_events={} commit_ms={:.4} mesh_dispatch_ms={:.4} mesh_complete_ms={:.4} mesh_ack_ms={:.4} peak_network_events={} queued={} columns={} rendered={} resident={} visible={}",
                teleport.settle_latency.as_secs_f64() * 1_000.0,
                optional_milliseconds(teleport.publisher_latency),
                optional_milliseconds(teleport.first_level_chunk_latency),
                optional_milliseconds(teleport.last_level_chunk_latency),
                teleport.level_chunk_events,
                optional_milliseconds(teleport.first_sub_chunk_latency),
                optional_milliseconds(teleport.last_sub_chunk_latency),
                teleport.sub_chunk_events,
                optional_milliseconds(teleport.last_chunk_commit_latency),
                optional_milliseconds(teleport.last_mesh_dispatch_latency),
                optional_milliseconds(teleport.last_mesh_completion_latency),
                optional_milliseconds(teleport.last_mesh_ack_latency),
                teleport.peak_network_events,
                queued_sub_chunks,
                snapshot.loaded_columns,
                snapshot.rendered_sub_chunks,
                snapshot.resident_sub_chunks,
                snapshot.visible_sub_chunks,
            );
            let _ = stdout.flush();
            return;
        }
        if let Some(remesh_latency) = acceptance
            .full_view_remesh
            .observe_snapshot(snapshot, observed_at)
        {
            metrics.0.record_forced_full_view_remesh(remesh_latency);
            let mut stdout = std::io::stdout().lock();
            let _ = writeln!(
                stdout,
                "RUST_MCBE_FORCED_FULL_VIEW_REMESH_SETTLED ms={:.4} queued={} rendered={} resident={} visible={}",
                remesh_latency.as_secs_f64() * 1_000.0,
                acceptance
                    .full_view_remesh
                    .queued_sub_chunks()
                    .unwrap_or_default(),
                snapshot.rendered_sub_chunks,
                snapshot.resident_sub_chunks,
                snapshot.visible_sub_chunks,
            );
            let _ = stdout.flush();
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
        stream.loaded_column_count(),
    );
}

fn optional_milliseconds(duration: Option<Duration>) -> f64 {
    duration.map_or(-1.0, |duration| duration.as_secs_f64() * 1_000.0)
}

fn flush_sub_chunk_requests(
    stream: &mut WorldStream,
    budget: usize,
    mut send: impl FnMut(world::ChunkKey, protocol::Packet) -> Result<(), network::PacketSendError>,
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
        match send(chunk, packet) {
            Ok(()) => {
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
        CommittedControlEvent::MovePlayer { movement, resolved } => {
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

#[allow(clippy::too_many_arguments)]
fn record_metrics_and_title(
    time: Res<Time>,
    mut client_world: ResMut<ClientWorld>,
    acceptance: Res<AcceptanceRun>,
    cache: Res<CaveVisibilityCache>,
    mut metrics: ResMut<AppMetrics>,
    diagnostic_quads: Res<DiagnosticQuads>,
    render_queue: Res<ChunkRenderQueue>,
    camera: Query<&Transform, With<FlyCamera>>,
    mut window: Query<(&mut Window, &CursorOptions), With<PrimaryWindow>>,
    mut title_elapsed: Local<Duration>,
) {
    metrics.0.record_frame(time.delta());
    metrics.0.record_asset_counters(
        client_world.runtime_assets.missing_count(),
        diagnostic_quads.0.total(),
    );
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
    );
    if let Some(error) = &client_world.fatal_error {
        title.push_str(" | ERROR: ");
        title.push_str(error);
    }
    window.title = title;
}

fn finish_acceptance_run(
    mut acceptance: ResMut<AcceptanceRun>,
    client_world: Res<ClientWorld>,
    metrics: Res<AppMetrics>,
    mut network: ResMut<NetworkHandle>,
    mut exit: MessageWriter<AppExit>,
) {
    if acceptance.finished {
        return;
    }
    let timed_out = acceptance
        .deadline
        .is_some_and(|deadline| Instant::now() >= deadline);
    let fatal = client_world.fatal_error.is_some();
    if !timed_out && !fatal {
        return;
    }

    acceptance.finished = true;
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
    network.shutdown();
    exit.write(if fatal || output_failed {
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
    use bevy::prelude::{Quat, Transform, Vec3};
    use protocol::{BlockUpdateEvent, LevelChunkEvent, LevelChunkMode, WorldBootstrap, WorldEvent};
    use std::time::{Duration, Instant};
    use world::SubChunkKey;

    use crate::{
        AcceptanceRun, FullViewRemeshTracker, FullViewTeleportTracker, MutationTracker,
        NETWORK_INGRESS_BUDGET_PER_FRAME, TeleportReadySnapshot, WORLD_READY_QUIET_INTERVAL,
        WorldReadySettler, WorldReadySnapshot, WorldReadyWork, bedrock_camera_rotation,
        camera_sub_chunk_key, cumulative_counter_delta, deterministic_mutation_coordinate,
        drain_network_ingress, flush_sub_chunk_requests, resolve_socket_dir_from, status_title,
        world_ready_markers,
    };

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
        let mut acceptance = AcceptanceRun::new(Some(900), None, false);
        assert!(acceptance.enabled());
        acceptance.set_mutation_surface_anchor([10, -6]);
        assert_eq!(acceptance.mutation_surface_anchor(), Some([10, -6]));
        acceptance.set_mutation_coordinate([14, 71, -6]);
        assert_eq!(acceptance.mutation_surface_anchor(), None);
        assert_eq!(acceptance.mutation_coordinate(), Some([14, 71, -6]));
    }

    #[test]
    fn timed_acceptance_deadline_begins_only_when_the_world_is_ready() {
        let mut acceptance = AcceptanceRun::new(Some(900), None, false);
        assert_eq!(acceptance.deadline, None);

        let world_ready_at = Instant::now() + Duration::from_secs(60);
        acceptance.begin_world_ready(world_ready_at, [0.5, 70.0, 0.5], 1_000);

        assert!(acceptance.world_ready);
        assert_eq!(
            acceptance.deadline,
            Some(world_ready_at + Duration::from_secs(900))
        );
    }

    fn settled_teleport_snapshot() -> TeleportReadySnapshot {
        TeleportReadySnapshot {
            received_radius_chunks: Some(16),
            publisher_radius_chunks: Some(16),
            rendered_sub_chunks: 8_000,
            resident_sub_chunks: 9_000,
            visible_sub_chunks: 7_000,
            loaded_columns: 1_000,
            last_chunk_commit_at: None,
            last_mesh_dispatch_at: None,
            last_mesh_completion_at: None,
            last_mesh_ack_at: None,
            work: WorldReadyWork::default(),
        }
    }

    #[test]
    fn full_view_teleport_requires_far_motion_matching_publisher_and_stable_clean_work() {
        let started = Instant::now();
        let mut tracker = FullViewTeleportTracker::new(true);
        tracker.begin_world_ready([0.5, 70.0, 0.5], 1_000);

        tracker.observe(
            &WorldEvent::MovePlayer(protocol::MovePlayerEvent {
                runtime_id: 1,
                position: [32.5, 70.0, 0.5],
                pitch: 0.0,
                yaw: 0.0,
            }),
            started,
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
        );
        tracker.observe(
            &WorldEvent::SubChunks(protocol::SubChunkBatchEvent {
                dimension: 0,
                entries: Vec::new(),
            }),
            started + Duration::from_millis(1_300),
        );
        tracker.observe(
            &WorldEvent::SubChunks(protocol::SubChunkBatchEvent {
                dimension: 0,
                entries: Vec::new(),
            }),
            started + Duration::from_millis(1_500),
        );
        tracker.observe(
            &WorldEvent::PublisherUpdate(protocol::PublisherUpdateEvent {
                center: [1_040, 70, 1_040],
                radius_blocks: 256,
            }),
            started + Duration::from_millis(1_600),
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
        let completion = tracker
            .observe_snapshot(clean, started + Duration::from_millis(4_100))
            .expect("stable clean target should complete");
        assert_eq!(completion.settle_latency, Duration::from_millis(2_100));
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
        tracker.begin_world_ready([0.5, 70.0, 0.5], 1_000);
        tracker.observe(
            &WorldEvent::MovePlayer(protocol::MovePlayerEvent {
                runtime_id: 1,
                position: [1_040.5, 70.0, 1_040.5],
                pitch: 0.0,
                yaw: 0.0,
            }),
            started,
        );
        tracker.observe(
            &WorldEvent::PublisherUpdate(protocol::PublisherUpdateEvent {
                center: [1_040, 70, 1_040],
                radius_blocks: 256,
            }),
            started + Duration::from_millis(100),
        );
        let mut partial = settled_teleport_snapshot();
        partial.loaded_columns = 999;

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
    fn full_view_remesh_closes_only_after_all_bounded_work_and_gpu_acknowledgements_clear() {
        let started = Instant::now();
        let mut tracker = FullViewRemeshTracker::default();
        assert!(tracker.start(started, 8_000));

        let mut busy = settled_teleport_snapshot();
        busy.work.unacknowledged_meshes = 1;
        assert_eq!(
            tracker.observe_snapshot(busy, started + Duration::from_millis(900)),
            None
        );
        assert_eq!(
            tracker.observe_snapshot(
                settled_teleport_snapshot(),
                started + Duration::from_millis(1_500),
            ),
            Some(Duration::from_millis(1_500))
        );
        assert_eq!(tracker.queued_sub_chunks(), Some(8_000));
        assert_eq!(
            tracker.observe_snapshot(
                settled_teleport_snapshot(),
                started + Duration::from_millis(1_600),
            ),
            None,
            "a completed gate emitted twice"
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
        let mut tracker = MutationTracker::new(coordinate);
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
                        payload: Vec::new(),
                    }),
                )
                .unwrap();
        }

        let mut attempts = Vec::new();
        let mut calls = 0;
        let sent = flush_sub_chunk_requests(&mut stream, 8, |chunk, packet| {
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

        let sent = flush_sub_chunk_requests(&mut stream, 8, |chunk, _packet| {
            attempts.push(chunk.x);
            Ok(())
        })
        .unwrap();
        assert_eq!(sent, 2);
        assert_eq!(attempts, [0, 1, 1, 2]);
        assert_eq!(stream.pending_request_count(), 0);
    }

    #[test]
    fn network_ingress_processes_a_fixed_budget_and_leaves_excess_in_the_bounded_channel() {
        let (sender, mut receiver) =
            tokio::sync::mpsc::channel(NETWORK_INGRESS_BUDGET_PER_FRAME + 2);
        for value in 0..NETWORK_INGRESS_BUDGET_PER_FRAME + 2 {
            sender.try_send(value).unwrap();
        }

        let drained = drain_network_ingress(&mut receiver, NETWORK_INGRESS_BUDGET_PER_FRAME);

        assert_eq!(drained.len(), NETWORK_INGRESS_BUDGET_PER_FRAME);
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
        let title = status_title(&transform, 42, 37, true);

        assert!(title.contains("pos 1.25 72.00 -8.50"));
        assert!(title.contains("yaw 0.50"));
        assert!(title.contains("chunks 37/42"));
        assert!(title.contains("captured"));
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
