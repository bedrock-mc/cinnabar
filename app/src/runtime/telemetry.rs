use std::{
    collections::VecDeque,
    fmt::Write as _,
    time::{Duration, Instant},
};

use bevy::{
    diagnostic::{DiagnosticPath, DiagnosticsStore},
    ecs::system::SystemParam,
    log::info,
    prelude::{
        EulerRot, Local, Quat, Query, Res, ResMut, Resource, Time, Transform, Vec3, Window, With,
    },
    window::{CursorOptions, PrimaryWindow},
    winit::{UpdateMode, WinitSettings},
};
use client_world::Phase2PresentationSnapshot;
use meshing::{
    BIOME_BLEND_RADIUS, BIOME_BLEND_SAMPLE_COUNT, BIOME_BLEND_WEIGHT_DENOMINATOR, BiomeBlendSample,
    ChunkBiomeTintIdentity, PackedBiomeRecord,
};
use render::{
    ChunkRenderInstance, ChunkRenderQueue, ModelWitnessEvidence, ModelWitnessManifestRecord,
    ModelWorkloadMetrics, RenderViewCohort, TransparentSortMetrics, TransparentWitnessEvidence,
    VisibilityDiagnostics, VisibilityDiagnosticsInput,
};
use sha2::{Digest, Sha256};
use world::SubChunkKey;

use crate::{
    acceptance::{
        AcceptanceRun, PHASE0_REQUESTED_RADIUS_CHUNKS,
        markers::{
            ERROR_COUNTERS, MODEL_WITNESS_COMPLETE, TRANSPARENT_SORT_COMMITTED,
            TRANSPARENT_WITNESS_COMPLETE, TRANSPARENT_WITNESS_INCOMPLETE,
            TRANSPARENT_WITNESS_STAGE, VISIBILITY_SNAPSHOT, acceptance_runtime_metadata_marker,
            cumulative_counter_delta, visibility_delta_marker_fields,
            visibility_digest_marker_fields, world_publication_snapshot_marker,
        },
        mutation::write_stdout_marker,
    },
    camera::{self, FlyCamera},
    metrics::{
        DiagnosticQuadTracker, GpuPassMeasurement, MetricsCollector, ModelWorkloadMetricsSnapshot,
        PipelineMetricsSnapshot, TransparentSortMetricsSnapshot, pair_gpu_pass_sample,
    },
    movement::{MovementSendError, MovementTicker, flush_player_auth_inputs},
    runtime::{
        network::{NetworkHandle, OUTBOUND_SEND_BUDGET_PER_FRAME},
        phase2_evidence::{
            CombinedPhase2Snapshot, build_profile_identity, cohort_identity,
            generation_manifest_identity, graphics_identity_sha256,
            phase2_publication_line_if_changed, present_mode_identity,
            sha256_identity_from_hex_or_text,
        },
        publication::{
            PublicationController, PublicationFrameWork, adaptive_publication_diagnostic_line,
        },
        shutdown::record_fatal_error,
        visibility::{AppMetrics, CaveVisibilityCache, DiagnosticQuads},
        world::ClientWorld,
    },
};

const TITLE_REFRESH_INTERVAL: Duration = Duration::from_millis(250);
const VISIBILITY_DIAGNOSTIC_INTERVAL: Duration = Duration::from_secs(1);
const OPAQUE_3D_GPU_DIAGNOSTIC: DiagnosticPath =
    DiagnosticPath::const_new("render/main_opaque_pass_3d/elapsed_gpu");
const TRANSPARENT_3D_GPU_DIAGNOSTIC: DiagnosticPath =
    DiagnosticPath::const_new("render/main_transparent_pass_3d/elapsed_gpu");

#[derive(SystemParam)]
pub(crate) struct TelemetryRenderMetrics<'w> {
    transparent_sort: Res<'w, TransparentSortMetrics>,
    model_workload: Res<'w, ModelWorkloadMetrics>,
    diagnostics: Res<'w, DiagnosticsStore>,
    publication: ResMut<'w, PublicationController>,
}

pub(crate) fn camera_sub_chunk_key(dimension: i32, position: Vec3) -> SubChunkKey {
    SubChunkKey::new(
        dimension,
        (position.x.floor() as i32).div_euclid(16),
        (position.y.floor() as i32).div_euclid(16),
        (position.z.floor() as i32).div_euclid(16),
    )
}

pub(crate) fn frame_limited_winit_settings(frame_cap: Option<u32>) -> WinitSettings {
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
pub(crate) struct RollingFps {
    pub(crate) frame_times: VecDeque<Duration>,
    pub(crate) elapsed: Duration,
}

#[derive(Default)]
pub(crate) struct MetricsSamplingState {
    pub(crate) title_elapsed: Duration,
    pub(crate) rolling_fps: RollingFps,
    pub(crate) last_marked_transparent_sort_generation: u64,
    pub(crate) last_gpu_measurement_time: Option<Instant>,
    pub(crate) visibility_elapsed: Duration,
    pub(crate) runtime_metadata_emitted: bool,
    pub(crate) diagnostic_attribution_revision: u64,
    pub(crate) last_biome_blend_identity: Option<CommittedBiomeBlendIdentity>,
    pub(crate) last_phase2_snapshot: Option<CombinedPhase2Snapshot>,
}

pub(crate) fn refresh_diagnostic_attribution(
    last_revision: &mut u64,
    tracker: &DiagnosticQuadTracker,
    metrics: &mut MetricsCollector,
) -> Option<String> {
    let revision = tracker.revision();
    if *last_revision == revision {
        return None;
    }
    let snapshot = tracker.snapshot();
    let marker = format!("DIAGNOSTIC_GEOMETRY {}", snapshot.marker_fields());
    metrics.record_diagnostic_attribution(snapshot);
    *last_revision = revision;
    Some(marker)
}

#[derive(Resource, Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) struct AcceptanceRuntimeConfig {
    pub(crate) build_profile: &'static str,
}

impl RollingFps {
    pub(crate) fn record(&mut self, frame_time: Duration) {
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

    pub(crate) fn value(&self) -> f64 {
        if self.elapsed.is_zero() {
            return 0.0;
        }
        self.frame_times.len() as f64 / self.elapsed.as_secs_f64()
    }
}

pub(crate) fn status_title(
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

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) struct CommittedBiomeBlendIdentity {
    key: SubChunkKey,
    generation: u64,
    tint_identity: ChunkBiomeTintIdentity,
    record_hash: u64,
    local: [i32; 3],
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) struct CommittedBiomeBlendSnapshot {
    identity: CommittedBiomeBlendIdentity,
    samples: [BiomeBlendSample; BIOME_BLEND_SAMPLE_COUNT],
}

impl CommittedBiomeBlendSnapshot {
    pub(crate) fn from_record(
        key: SubChunkKey,
        generation: u64,
        tint_identity: ChunkBiomeTintIdentity,
        local: [i32; 3],
        record: &PackedBiomeRecord,
    ) -> Option<Self> {
        if local
            .into_iter()
            .any(|coordinate| !(0..16).contains(&coordinate))
        {
            return None;
        }
        Some(Self {
            identity: CommittedBiomeBlendIdentity {
                key,
                generation,
                tint_identity,
                record_hash: packed_biome_record_hash(record),
                local,
            },
            samples: record.blend_samples(local)?,
        })
    }
}

pub(crate) fn biome_blend_diagnostics_enabled(acceptance: &AcceptanceRun) -> bool {
    acceptance.enabled()
}

pub(crate) fn biome_blend_diagnostic_marker_if_changed(
    last_emitted: &mut Option<CommittedBiomeBlendIdentity>,
    snapshot: CommittedBiomeBlendSnapshot,
) -> Option<String> {
    if last_emitted.as_ref() == Some(&snapshot.identity) {
        return None;
    }
    *last_emitted = Some(snapshot.identity);
    let identity = snapshot.identity;
    let mut marker = format!(
        "BIOME_BLEND_COMMITTED stage=app_committed key={},{},{},{} generation={} tint_stream={} tint_revision={} record_hash={:016x} local={},{},{} radius={} denominator={} samples=",
        identity.key.dimension,
        identity.key.x,
        identity.key.y,
        identity.key.z,
        identity.generation,
        identity.tint_identity.stream(),
        identity.tint_identity.revision(),
        identity.record_hash,
        identity.local[0],
        identity.local[1],
        identity.local[2],
        BIOME_BLEND_RADIUS,
        BIOME_BLEND_WEIGHT_DENOMINATOR,
    );
    for (index, sample) in snapshot.samples.into_iter().enumerate() {
        if index != 0 {
            marker.push(';');
        }
        write!(
            marker,
            "{},{}:{}:{}",
            sample.offset[0], sample.offset[1], sample.tint_index, sample.weight_numerator,
        )
        .expect("writing to String cannot fail");
    }
    Some(marker)
}

fn packed_biome_record_hash(record: &PackedBiomeRecord) -> u64 {
    const FNV_OFFSET: u64 = 0xcbf2_9ce4_8422_2325;
    const FNV_PRIME: u64 = 0x0000_0100_0000_01b3;
    record.words().iter().fold(FNV_OFFSET, |hash, word| {
        word.to_le_bytes().into_iter().fold(hash, |hash, byte| {
            (hash ^ u64::from(byte)).wrapping_mul(FNV_PRIME)
        })
    })
}

pub(crate) fn bedrock_camera_rotation(yaw_degrees: f32, pitch_degrees: f32) -> Quat {
    Quat::from_euler(
        EulerRot::YXZ,
        (180.0 - yaw_degrees).to_radians(),
        -pitch_degrees.to_radians(),
        0.0,
    )
}

pub(crate) fn send_player_auth_inputs(
    network: Res<NetworkHandle>,
    mut movement: ResMut<MovementTicker>,
    mut client_world: ResMut<ClientWorld>,
) {
    let result =
        flush_player_auth_inputs(&mut movement, OUTBOUND_SEND_BUDGET_PER_FRAME, |packet| {
            network.send_packet(packet)
        });
    match result {
        Ok(_) => {}
        Err(MovementSendError::Transport(
            crate::runtime::network::session::PacketSendError::Full(_),
        )) => movement.note_full_restore(),
        Err(MovementSendError::Encode(error)) => {
            movement.deactivate();
            record_fatal_error(
                &mut client_world.fatal_error,
                format!("failed to encode PlayerAuthInput: {error}"),
            );
        }
        Err(MovementSendError::Transport(
            crate::runtime::network::session::PacketSendError::Closed(_),
        )) => {
            movement.deactivate();
            record_fatal_error(
                &mut client_world.fatal_error,
                "failed to send PlayerAuthInput: network command channel is closed".to_owned(),
            );
        }
        Err(MovementSendError::RestoreOverflow) => {
            movement.deactivate();
            record_fatal_error(
                &mut client_world.fatal_error,
                "failed to restore backpressured PlayerAuthInput".to_owned(),
            );
        }
    }
}

pub(crate) fn update_visibility_diagnostics(
    cache: Res<CaveVisibilityCache>,
    chunks: Query<&ChunkRenderInstance>,
    mut diagnostics: ResMut<VisibilityDiagnosticsInput>,
) {
    if !diagnostics.enabled() {
        return;
    }
    let resident_mesh = chunks.iter().map(ChunkRenderInstance::key);
    let cave_visible = chunks
        .iter()
        .map(ChunkRenderInstance::key)
        .filter(|&key| cache.is_visible(key));
    diagnostics.advance(resident_mesh, cave_visible);
}

pub(crate) fn lower_hex(bytes: &[u8]) -> String {
    bytes.iter().map(|byte| format!("{byte:02x}")).collect()
}

pub(crate) fn model_witness_manifest_hash(records: &[ModelWitnessManifestRecord]) -> String {
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
pub(crate) fn record_metrics_and_title(
    time: Res<Time>,
    mut client_world: ResMut<ClientWorld>,
    acceptance: Res<AcceptanceRun>,
    cache: Res<CaveVisibilityCache>,
    mut metrics: ResMut<AppMetrics>,
    diagnostic_quads: Res<DiagnosticQuads>,
    render_queue: Res<ChunkRenderQueue>,
    mut render_metrics: TelemetryRenderMetrics,
    transparent_witness: Res<TransparentWitnessEvidence>,
    model_witness: Res<ModelWitnessEvidence>,
    visibility_diagnostics: Res<VisibilityDiagnostics>,
    runtime_config: Res<AcceptanceRuntimeConfig>,
    chunks: Query<&ChunkRenderInstance>,
    camera: Query<&Transform, With<FlyCamera>>,
    mut window: Query<(&mut Window, &CursorOptions), With<PrimaryWindow>>,
    mut sampling: Local<MetricsSamplingState>,
) {
    let now = Instant::now();
    if !sampling.runtime_metadata_emitted
        && let Some(graphics_adapter) = visibility_diagnostics.graphics_adapter()
    {
        let marker = acceptance_runtime_metadata_marker(*runtime_config, &graphics_adapter);
        let mut stdout = std::io::stdout().lock();
        write_stdout_marker(&mut stdout, &marker);
        sampling.runtime_metadata_emitted = true;
    }
    let gpu_sample = {
        let diagnostics = &render_metrics.diagnostics;
        pair_gpu_pass_sample(
            sampling.last_gpu_measurement_time,
            gpu_pass_measurement(diagnostics, &OPAQUE_3D_GPU_DIAGNOSTIC),
            gpu_pass_measurement(diagnostics, &TRANSPARENT_3D_GPU_DIAGNOSTIC),
        )
    };
    if let Some((measurement_time, sample)) = gpu_sample {
        sampling.last_gpu_measurement_time = Some(measurement_time);
        metrics.0.record_gpu_pass_sample(measurement_time, sample);
    }
    if let Some(deadline) = acceptance.deadline.filter(|deadline| now >= *deadline) {
        metrics.0.finish_timed_session(deadline);
    }
    let frame_time = time.delta();
    metrics.0.record_frame(frame_time);
    sampling.rolling_fps.record(frame_time);
    metrics.0.record_asset_counters(
        client_world.runtime_assets.missing_count(),
        diagnostic_quads.0.total(),
    );
    if let Some(marker) = refresh_diagnostic_attribution(
        &mut sampling.diagnostic_attribution_revision,
        &diagnostic_quads.0,
        &mut metrics.0,
    ) {
        info!("{marker}");
    }
    let visibility_snapshot = visibility_diagnostics.snapshot();
    if let (Some(stream), Ok(camera), Some(graphics)) = (
        client_world.stream.as_ref(),
        camera.single(),
        visibility_diagnostics.graphics_adapter(),
    ) && camera.translation.is_finite()
    {
        let player_column =
            camera_sub_chunk_key(stream.current_dimension(), camera.translation).chunk();
        let publication = stream.phase2_publication_snapshot(player_column);
        let session_generation = publication.session_generation;
        let required_cohort_hash = publication.required_cohort_hash;
        let stage_generation = visibility_snapshot.frame_generation;
        let render_cohort = stream
            .committed_view_cohort()
            .map(|cohort| RenderViewCohort::new(cohort.dimension, cohort.center, cohort.radius));
        let allocation_manifest = chunks
            .iter()
            .filter(|instance| render_cohort.is_none_or(|cohort| cohort.contains(instance.key())))
            .map(|instance| (instance.key(), instance.generation()))
            .collect::<Vec<_>>();
        let allocation = generation_manifest_identity(
            session_generation,
            required_cohort_hash,
            &allocation_manifest,
        );
        let publisher_manifest = render_cohort.map_or_else(Vec::new, |cohort| {
            render_queue
                .freeze_target_expectation(cohort, None, stage_generation, now)
                .manifest
                .to_vec()
        });
        let publisher_disk = generation_manifest_identity(
            session_generation,
            required_cohort_hash,
            &publisher_manifest,
        );
        let presentation = Phase2PresentationSnapshot {
            build_profile: build_profile_identity(runtime_config.build_profile),
            graphics_identity_sha256: graphics_identity_sha256(&graphics),
            requested_present_mode: present_mode_identity(&graphics.requested_present_mode),
            effective_present_mode: present_mode_identity(&graphics.effective_present_mode),
            assets_manifest_sha256: sha256_identity_from_hex_or_text(
                &metrics.0.asset_metrics().blob_sha256,
            ),
            publisher_disk,
            resident: cohort_identity(
                session_generation,
                required_cohort_hash,
                stage_generation,
                visibility_snapshot.resident_mesh,
            ),
            allocation,
            visible: cohort_identity(
                session_generation,
                required_cohort_hash,
                stage_generation,
                visibility_snapshot.frustum_visible_opaque,
            ),
            submitted: cohort_identity(
                session_generation,
                required_cohort_hash,
                stage_generation,
                visibility_snapshot.submitted_opaque,
            ),
            gpu_presented: cohort_identity(
                session_generation,
                required_cohort_hash,
                stage_generation,
                visibility_snapshot.gpu_completed_opaque,
            ),
        };
        if let Some(marker) = phase2_publication_line_if_changed(
            &mut sampling.last_phase2_snapshot,
            CombinedPhase2Snapshot {
                publication,
                presentation,
                present_mode_proven: graphics.present_mode_proven,
                client_blob_cache_enabled: client_world.client_blob_cache_enabled,
                client_blob_cache: client_world.client_blob_cache,
            },
        ) {
            let mut stdout = std::io::stdout().lock();
            write_stdout_marker(&mut stdout, &marker);
        }
    }
    if let Some(stream) = client_world.stream.as_ref() {
        let cohort = stream
            .committed_view_cohort()
            .map(|target| stream.cohort_status(target));
        let count = |digest: Option<render::VisibilityKeyDigest>| {
            digest
                .and_then(|digest| usize::try_from(digest.count).ok())
                .unwrap_or(0)
        };
        let previous = render_metrics.publication.diagnostics().last_work;
        render_metrics
            .publication
            .finish_frame(PublicationFrameWork {
                cohort_expected: cohort.map_or(0, |status| status.expected),
                cohort_loaded: cohort.map_or(0, |status| status.loaded_target),
                resident_meshes: count(visibility_snapshot.resident_mesh),
                cave_visible_meshes: count(visibility_snapshot.cave_visible),
                frustum_visible_meshes: count(visibility_snapshot.frustum_visible_opaque),
                submitted_meshes: count(visibility_snapshot.submitted_opaque),
                gpu_completed_meshes: count(visibility_snapshot.gpu_completed_opaque),
                ..previous
            });
    }
    sampling.visibility_elapsed += frame_time;
    if sampling.visibility_elapsed >= VISIBILITY_DIAGNOSTIC_INTERVAL {
        sampling.visibility_elapsed = Duration::ZERO;
        let snapshot = visibility_snapshot;
        if biome_blend_diagnostics_enabled(&acceptance)
            && let (Some(stream), Ok(camera)) = (client_world.stream.as_ref(), camera.single())
            && camera.translation.is_finite()
        {
            let key = camera_sub_chunk_key(stream.current_dimension(), camera.translation);
            let block = camera.translation.floor().as_ivec3();
            let local = [
                block.x.rem_euclid(16),
                block.y.rem_euclid(16),
                block.z.rem_euclid(16),
            ];
            if let Some(instance) = chunks.iter().find(|instance| instance.key() == key)
                && let Some(snapshot) = CommittedBiomeBlendSnapshot::from_record(
                    key,
                    instance.generation(),
                    instance.tint_identity(),
                    local,
                    instance.biome_record(),
                )
                && let Some(marker) = biome_blend_diagnostic_marker_if_changed(
                    &mut sampling.last_biome_blend_identity,
                    snapshot,
                )
            {
                let mut stdout = std::io::stdout().lock();
                write_stdout_marker(&mut stdout, &marker);
            }
        }
        if snapshot.frame_generation != 0 {
            let marker = format!(
                "{VISIBILITY_SNAPSHOT} frame_generation={} camera={} pose_hash={:016x} camera_frustum_hash={:016x} pose_generation={} view_generation={} draw_mode={:?} {} {} {} {} {} {} {} {} {} {} resident_overflowed={} cave_overflowed={} frustum_overflowed={} submitted_overflowed={}",
                snapshot.frame_generation,
                snapshot.camera.stable_id,
                snapshot.camera.pose_hash,
                snapshot.camera.frustum_hash,
                snapshot.pose_generation,
                snapshot.view_generation,
                snapshot.draw_mode,
                visibility_digest_marker_fields("resident", snapshot.resident_mesh),
                visibility_digest_marker_fields("cave", snapshot.cave_visible),
                visibility_digest_marker_fields("frustum", snapshot.frustum_visible_opaque),
                visibility_digest_marker_fields("submitted", snapshot.submitted_opaque),
                visibility_digest_marker_fields("gpu_completed", snapshot.gpu_completed_opaque),
                visibility_delta_marker_fields("resident_to_cave", snapshot.resident_to_cave),
                visibility_delta_marker_fields("resident_to_frustum", snapshot.resident_to_frustum,),
                visibility_delta_marker_fields("cave_to_frustum", snapshot.cave_to_frustum),
                visibility_delta_marker_fields(
                    "frustum_to_submitted",
                    snapshot.frustum_to_submitted,
                ),
                visibility_delta_marker_fields(
                    "submitted_to_gpu_completed",
                    snapshot.submitted_to_gpu_completed,
                ),
                snapshot.resident_overflowed,
                snapshot.cave_overflowed,
                snapshot.frustum_overflowed,
                snapshot.submitted_overflowed,
            );
            let mut stdout = std::io::stdout().lock();
            write_stdout_marker(&mut stdout, &marker);
            write_stdout_marker(
                &mut stdout,
                &adaptive_publication_diagnostic_line(render_metrics.publication.diagnostics()),
            );
            if let (Some(stream), Some(graphics)) = (
                client_world.stream.as_ref(),
                visibility_diagnostics.graphics_adapter(),
            ) {
                let marker = world_publication_snapshot_marker(
                    stream.stats(),
                    render_queue.retained_len(),
                    render_queue.pending_bytes(),
                    render_queue.gpu_upload_bytes(),
                    snapshot,
                    *runtime_config,
                    &graphics,
                );
                write_stdout_marker(&mut stdout, &marker);
            }
        }
    }
    let transparent_sort_snapshot =
        TransparentSortMetricsSnapshot::from(render_metrics.transparent_sort.snapshot());
    let model_workload_snapshot =
        ModelWorkloadMetricsSnapshot::from(render_metrics.model_workload.snapshot());
    if let Some(marker) = transparent_sort_committed_marker(
        sampling.last_marked_transparent_sort_generation,
        transparent_sort_snapshot,
    ) {
        let mut stdout = std::io::stdout().lock();
        write_stdout_marker(&mut stdout, &marker);
        sampling.last_marked_transparent_sort_generation =
            transparent_sort_snapshot.presented_generation;
    }
    for event in transparent_witness.drain_events() {
        let marker = format!(
            "{TRANSPARENT_WITNESS_COMPLETE} revision={} sequence={} generation={} key_count={} consecutive={}",
            event.revision, event.sequence, event.generation, event.key_count, event.consecutive,
        );
        let mut stdout = std::io::stdout().lock();
        write_stdout_marker(&mut stdout, &marker);
    }
    for event in model_witness.drain_events() {
        let acknowledgement = &event.acknowledgement;
        let marker = format!(
            "{MODEL_WITNESS_COMPLETE} revision={} request_sha256={} sequence={} view_generation={} key_count={} model_ref_count={} manifest_count={} manifest_sha256={} missing={} stale={} wrong_stream={} zero_ref={} draw_mismatch={} consecutive={}",
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
            "{TRANSPARENT_WITNESS_INCOMPLETE} revision={} sequence={} generation={} missing_count={} missing={missing}",
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
            "{TRANSPARENT_WITNESS_STAGE} revision={} committed_generation={} records={records}",
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
            model_workload: model_workload_snapshot,
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
            "{ERROR_COUNTERS} network={} world_decode={} world_normalization={} reason_total={} reasons={normalization_reasons:?}",
            client_world.network_decode_errors,
            world_decode_errors,
            world_normalization_errors,
            normalization_reason_total,
        );
    }
    let error_delta = cumulative_counter_delta(total_errors, client_world.reported_decode_errors);
    metrics.0.add_decode_errors(error_delta);
    client_world.reported_decode_errors = total_errors;

    sampling.title_elapsed += time.delta();
    if sampling.title_elapsed < TITLE_REFRESH_INTERVAL {
        return;
    }
    sampling.title_elapsed = Duration::ZERO;
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
        sampling.rolling_fps.value(),
    );
    if let Some(error) = &client_world.fatal_error {
        title.push_str(" | ERROR: ");
        title.push_str(error);
    }
    window.title = title;
}

pub(crate) fn gpu_pass_measurement(
    diagnostics: &DiagnosticsStore,
    path: &DiagnosticPath,
) -> Option<GpuPassMeasurement> {
    diagnostics
        .get_measurement(path)
        .map(|measurement| GpuPassMeasurement::new(measurement.time, measurement.value))
}

pub(crate) fn transparent_sort_committed_marker(
    last_presented_generation: u64,
    snapshot: TransparentSortMetricsSnapshot,
) -> Option<String> {
    (snapshot.presented_generation > last_presented_generation
        && snapshot.presented_generation == snapshot.committed_generation
        && snapshot.ref_count > 0)
        .then(|| {
            format!(
                "{TRANSPARENT_SORT_COMMITTED} generation={} ref_count={}",
                snapshot.presented_generation, snapshot.ref_count
            )
        })
}
