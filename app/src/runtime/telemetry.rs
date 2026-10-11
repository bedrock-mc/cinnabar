#[cfg(feature = "acceptance")]
use ::acceptance::AcceptanceRun;
#[cfg(feature = "acceptance")]
use acceptance::phase2_evidence::{
    CombinedPhase2Snapshot, PlayerColumnPresentationEvidence, build_profile_identity,
    generation_manifest_identity, graphics_identity_sha256, key_manifest_identity,
    phase2_publication_line_if_changed, present_mode_identity, sha256_identity_from_hex_or_text,
};
mod attribution;
use attribution::DiagnosticAttributionLogState;
pub(crate) use attribution::refresh_diagnostic_attribution;

#[cfg(feature = "acceptance")]
use bevy::prelude::{Transform, With};
#[cfg(feature = "acceptance")]
use meshing::biome_lattice::{BIOME_BLEND_RADIUS, BLEND_SAMPLE_COUNT};
use std::{
    fmt::Write as _,
    time::{Duration, Instant},
};

use bevy::{
    diagnostic::{DiagnosticPath, DiagnosticsStore},
    ecs::system::SystemParam,
    log::info,
    prelude::{EulerRot, Local, Quat, Query, Res, ResMut, Time, Vec3},
    time::Real,
};
#[cfg(feature = "acceptance")]
use chunk_pipeline::PresentationSnapshot;
#[cfg(feature = "acceptance")]
use meshing::{BiomeBlendSample, ChunkBiomeTintIdentity, PackedBiomeRecord};
use render::{
    ChunkRenderInstance, ChunkRenderQueue, ModelWorkloadMetrics, RuntimeStage,
    RuntimeStageProfiler, TransparentSortMetrics, VisibilityDiagnostics,
    VisibilityDiagnosticsInput,
};
#[cfg(feature = "acceptance")]
use render::{ModelWitnessEvidence, RenderViewCohort, TransparentWitnessEvidence};
use world::SubChunkKey;

mod visibility_snapshot;

#[cfg(feature = "acceptance")]
use client_presentation::camera::FlyCamera;
use diagnostics::metrics::{
    GpuPassMeasurement, ModelWorkloadMetricsSnapshot, PipelineMetricsSnapshot,
    TransparentSortMetricsSnapshot, pair_gpu_pass_sample,
};
use {
    crate::{
        movement::MovementTicker,
        runtime::{
            network::{NetworkHandle, OUTBOUND_SEND_BUDGET_PER_FRAME},
            publication::{
                PublicationController, PublicationFrameWork, adaptive_publication_diagnostic_line,
            },
            shutdown::record_fatal_error,
            visibility::{AppMetrics, CaveVisibilityCache, DiagnosticQuads},
            world::{ClientWorld, WorldStreamFramePoll},
        },
        semantic_controls::SemanticInputSnapshot,
    },
    client_presentation::{
        camera::THIRD_PERSON_RADIUS_BLOCKS, local_player::LocalPlayerFrameCarrier,
    },
    diagnostics::{
        markers::{
            ERROR_COUNTERS, STAGE_PROFILE, VISIBILITY_SNAPSHOT, acceptance_runtime_metadata_marker,
            cumulative_counter_delta, visibility_delta_marker_fields,
            visibility_digest_marker_fields, world_publication_snapshot_marker,
        },
        write_stdout_marker,
    },
    gameplay::movement::{
        MovementSendError, PhysicsTickEvidenceContext, flush_player_auth_inputs_guarded,
    },
};

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
    #[cfg(feature = "acceptance")]
    local_player: Res<'w, LocalPlayerFrameCarrier>,
    frame_poll: Res<'w, WorldStreamFramePoll>,
    profiler: Option<Res<'w, RuntimeStageProfiler>>,
    visibility_input: Res<'w, VisibilityDiagnosticsInput>,
}

pub(crate) fn camera_sub_chunk_key(dimension: i32, position: Vec3) -> SubChunkKey {
    SubChunkKey::new(
        dimension,
        (position.x.floor() as i32).div_euclid(16),
        (position.y.floor() as i32).div_euclid(16),
        (position.z.floor() as i32).div_euclid(16),
    )
}

pub(crate) fn local_subject_column(dimension: i32, position: Vec3) -> Option<world::ChunkKey> {
    position
        .is_finite()
        .then(|| camera_sub_chunk_key(dimension, position).chunk())
}

#[derive(Default)]
pub(crate) struct MetricsSamplingState {
    pub(crate) last_marked_transparent_sort_generation: u64,
    pub(crate) last_gpu_measurement_time: Option<Instant>,
    pub(crate) visibility_elapsed: Duration,
    pub(crate) runtime_metadata_emitted: bool,
    pub(crate) diagnostic_attribution_revision: u64,
    diagnostic_attribution_log: DiagnosticAttributionLogState,
    #[cfg(feature = "acceptance")]
    pub(crate) last_biome_blend_identity: Option<CommittedBiomeBlendIdentity>,
    #[cfg(feature = "acceptance")]
    pub(crate) last_phase2_snapshot: Option<CombinedPhase2Snapshot>,
}

use diagnostics::AcceptanceRuntimeConfig;

#[cfg(feature = "acceptance")]
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) struct CommittedBiomeBlendIdentity {
    key: SubChunkKey,
    generation: u64,
    tint_identity: ChunkBiomeTintIdentity,
    record_hash: u64,
    local: [i32; 3],
}

#[cfg(feature = "acceptance")]
#[derive(Debug, Clone, Copy, PartialEq)]
pub(crate) struct CommittedBiomeBlendSnapshot {
    identity: CommittedBiomeBlendIdentity,
    samples: [BiomeBlendSample; BLEND_SAMPLE_COUNT],
}

#[cfg(feature = "acceptance")]
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

#[cfg(feature = "acceptance")]
pub(crate) fn biome_blend_diagnostics_enabled(acceptance: &AcceptanceRun) -> bool {
    acceptance.enabled()
}

#[cfg(feature = "acceptance")]
pub(crate) fn publication_diagnostics_enabled(acceptance: &AcceptanceRun) -> bool {
    acceptance.enabled() || acceptance.metrics_out.is_some()
}

#[cfg(feature = "acceptance")]
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
        "BIOME_BLEND_COMMITTED stage=app_committed key={},{},{},{} generation={} tint_stream={} tint_revision={} record_hash={:016x} local={},{},{} radius={} samples=",
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
    );
    for (index, sample) in snapshot.samples.into_iter().enumerate() {
        if index != 0 {
            marker.push(';');
        }
        write!(marker, "{}:{:.8}", sample.tint_index, sample.weight,)
            .expect("writing to String cannot fail");
    }
    Some(marker)
}

#[cfg(feature = "acceptance")]
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
    #[cfg(feature = "acceptance")] acceptance: Res<AcceptanceRun>,
    input: Res<SemanticInputSnapshot>,
    local_frame: Res<LocalPlayerFrameCarrier>,
    mut metrics: ResMut<AppMetrics>,
    mut movement: ResMut<MovementTicker>,
    mut client_world: ResMut<ClientWorld>,
) {
    #[cfg(feature = "acceptance")]
    if acceptance.deadline_reached(Instant::now()) {
        movement.begin_terminal_drain();
    }
    let evidence_context = input
        .snapshot()
        .zip(local_frame.snapshot())
        .zip(client_world.stream.as_ref())
        .map(|((input, frame), stream)| {
            let third_person = frame.perspective() != semantic_input::PerspectiveMode::FirstPerson;
            let camera_distance = frame.pose().translation.distance(frame.eye());
            let camera_fallback =
                third_person && camera_distance <= render_api::CAMERA_NEAR_PLANE_BLOCKS;
            let camera_blocked = third_person
                && !camera_fallback
                && camera_distance + render_api::CAMERA_NEAR_PLANE_BLOCKS
                    < THIRD_PERSON_RADIUS_BLOCKS;
            PhysicsTickEvidenceContext {
                fifo_sequence: frame.fifo_sequence(),
                pose_generation: frame.pose_generation(),
                dimension: stream.current_dimension(),
                perspective: frame.perspective(),
                camera_blocked,
                camera_fallback,
                local_avatar_visible: third_person,
                look_delta: input.look_delta,
                outbound_authorized: movement.physics_is_authorized(),
                outbox_depth: movement.pending_count(),
                outbox_drops: movement.dropped_tick_count(),
                free_camera_packet_count: movement.sent_free_camera_packet_count(),
            }
        });
    if network.closed_command_has_pending_control() {
        metrics.0.record_outbound_movement_telemetry(
            movement.sent_physics_packet_count(),
            movement.pending_count(),
            movement.pending_authority_fault().is_some(),
        );
        return;
    }
    let result = flush_player_auth_inputs_guarded(
        &mut movement,
        OUTBOUND_SEND_BUDGET_PER_FRAME,
        evidence_context,
        |identity, packet, mining_guard| {
            network.send_physics_packet(identity, packet, mining_guard)
        },
    );
    match result {
        Ok(_) => {}
        Err(MovementSendError::Transport(
            crate::runtime::network::session::PacketSendError::Full(_),
        )) => {
            metrics.0.add_outbound_budget_drops(1);
            movement.note_full_restore();
        }
        Err(MovementSendError::Encode(error)) => {
            movement.deactivate();
            record_fatal_error(
                &mut client_world.fatal_error,
                format!("failed to encode PlayerAuthInput: {error}"),
            );
        }
        Err(MovementSendError::Transport(
            crate::runtime::network::session::PacketSendError::Closed(_),
        )) if network.closed_command_has_pending_control() => {}
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
        Err(MovementSendError::MissingEvidenceContext) => {
            movement.deactivate();
            record_fatal_error(
                &mut client_world.fatal_error,
                "failed to stage immutable Phase 3 evidence for PlayerAuthInput".to_owned(),
            );
        }
    }
    metrics.0.record_outbound_movement_telemetry(
        movement.sent_physics_packet_count(),
        movement.pending_count(),
        movement.pending_authority_fault().is_some(),
    );
}

pub(crate) fn update_visibility_diagnostics(
    cache: Res<CaveVisibilityCache>,
    chunks: Query<&ChunkRenderInstance>,
    local_player: Res<LocalPlayerFrameCarrier>,
    client_world: Res<ClientWorld>,
    mut diagnostics: ResMut<VisibilityDiagnosticsInput>,
) {
    if !diagnostics.enabled() {
        return;
    }
    let witness_column = client_world.stream.as_ref().and_then(|stream| {
        let eye = local_player.snapshot()?.eye();
        local_subject_column(stream.current_dimension(), eye)
    });
    diagnostics.set_witness_column(witness_column);
    let resident_mesh = chunks.iter().map(ChunkRenderInstance::key);
    let cave_visible = chunks
        .iter()
        .map(ChunkRenderInstance::key)
        .filter(|&key| cache.is_visible(key));
    diagnostics.advance(resident_mesh, cave_visible);
}

#[allow(clippy::too_many_arguments)]
pub(crate) fn record_metrics(
    time: Res<Time<Real>>,
    mut client_world: ResMut<ClientWorld>,
    #[cfg(feature = "acceptance")] acceptance: Res<AcceptanceRun>,
    cache: Res<CaveVisibilityCache>,
    mut metrics: ResMut<AppMetrics>,
    diagnostic_quads: Res<DiagnosticQuads>,
    render_queue: Res<ChunkRenderQueue>,
    mut render_metrics: TelemetryRenderMetrics,
    #[cfg(feature = "acceptance")] transparent_witness: Res<TransparentWitnessEvidence>,
    #[cfg(feature = "acceptance")] model_witness: Res<ModelWitnessEvidence>,
    visibility_diagnostics: Res<VisibilityDiagnostics>,
    runtime_config: Res<AcceptanceRuntimeConfig>,
    #[cfg(feature = "acceptance")] chunks: Query<&ChunkRenderInstance>,
    #[cfg(feature = "acceptance")] camera: Query<&Transform, With<FlyCamera>>,
    mut sampling: Local<MetricsSamplingState>,
) {
    let _timer = render_metrics
        .profiler
        .as_deref()
        .map(|profiler| profiler.time(RuntimeStage::AcceptanceTelemetry));
    let now = Instant::now();
    if !sampling.runtime_metadata_emitted
        && let Some(graphics_adapter) = visibility_diagnostics.graphics_adapter()
    {
        let marker = acceptance_runtime_metadata_marker(*runtime_config, &graphics_adapter);
        let mut stdout = diagnostics::console::stdout();
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
    #[cfg(feature = "acceptance")]
    if let Some(deadline) = acceptance.deadline.filter(|deadline| now >= *deadline) {
        metrics.0.finish_timed_session(deadline);
    }
    let frame_time = time.delta();
    metrics.0.record_frame(frame_time);
    metrics.0.record_asset_counters(
        client_world.missing_asset_count(),
        diagnostic_quads.0.total(),
    );
    let fresh_marker = refresh_diagnostic_attribution(
        &mut sampling.diagnostic_attribution_revision,
        &diagnostic_quads.0,
        &mut metrics.0,
    );
    if let Some(marker) = sampling.diagnostic_attribution_log.take(now, fresh_marker) {
        info!("{marker}");
    }
    let visibility_snapshot = visibility_snapshot::active_snapshot(
        &render_metrics.visibility_input,
        visibility_diagnostics.snapshot(),
    );
    // Full-cohort manifests and their JSON/timing markers are acceptance
    // evidence, not gameplay work. Gate the collection as well as the output.
    #[cfg(feature = "acceptance")]
    if publication_diagnostics_enabled(&acceptance)
        && let (Some(stream), Some(local_frame), Some(graphics)) = (
            client_world.stream.as_ref(),
            render_metrics.local_player.snapshot(),
            visibility_diagnostics.graphics_adapter(),
        )
        && local_frame.eye().is_finite()
    {
        let player_column = local_subject_column(stream.current_dimension(), local_frame.eye())
            .expect("finite local-player eyes have a subject column");
        let publication = stream.phase2_publication_snapshot(player_column);
        let session_generation = publication.session_generation;
        let publisher_epoch = publication.publisher_epoch;
        let required_cohort_count = publication.required_columns;
        let required_cohort_hash = publication.required_cohort_hash;
        let stage_generation = visibility_snapshot.frame_generation;
        let render_cohort = stream
            .committed_view_cohort()
            .map(|cohort| RenderViewCohort::new(cohort.dimension, cohort.center, cohort.radius));
        let required_columns = stream.required_columns();
        let allocation_manifest = chunks
            .iter()
            .filter(|instance| required_columns.contains(&instance.key().chunk()))
            .map(|instance| (instance.key(), instance.generation()))
            .collect::<Vec<_>>();
        let allocation = generation_manifest_identity(
            session_generation,
            publisher_epoch,
            required_cohort_count,
            required_cohort_hash,
            &allocation_manifest,
        );
        let publisher_manifest = render_cohort.map_or_else(Vec::new, |cohort| {
            render_queue
                .freeze_target_expectation_for_columns(
                    cohort,
                    None,
                    required_columns.iter().copied(),
                    stage_generation,
                    now,
                )
                .map_or_else(Vec::new, |expectation| expectation.manifest.to_vec())
        });
        let publisher_disk = generation_manifest_identity(
            session_generation,
            publisher_epoch,
            required_cohort_count,
            required_cohort_hash,
            &publisher_manifest,
        );
        let presentation = PresentationSnapshot {
            build_profile: build_profile_identity(runtime_config.build_profile),
            graphics_identity_sha256: graphics_identity_sha256(&graphics),
            requested_present_mode: present_mode_identity(&graphics.requested_present_mode),
            effective_present_mode: present_mode_identity(&graphics.effective_present_mode),
            assets_manifest_sha256: sha256_identity_from_hex_or_text(
                &metrics.0.asset_metrics().blob_sha256,
            ),
            visible_subset_of_resident: visibility_snapshot
                .resident_to_frustum
                .is_some_and(|delta| delta.extra.count == 0),
            publisher_disk,
            resident: key_manifest_identity(
                session_generation,
                publisher_epoch,
                required_cohort_count,
                required_cohort_hash,
                visibility_snapshot.resident_mesh,
            ),
            allocation,
            visible: key_manifest_identity(
                session_generation,
                publisher_epoch,
                required_cohort_count,
                required_cohort_hash,
                visibility_snapshot.frustum_visible_opaque,
            ),
            submitted: key_manifest_identity(
                session_generation,
                publisher_epoch,
                required_cohort_count,
                required_cohort_hash,
                visibility_snapshot.submitted_opaque,
            ),
            gpu_presented: key_manifest_identity(
                session_generation,
                publisher_epoch,
                required_cohort_count,
                required_cohort_hash,
                visibility_snapshot.gpu_completed_opaque,
            ),
        };
        let exact_visibility_witness = visibility_snapshot.witness_column == Some(player_column);
        let player_column_presentation = PlayerColumnPresentationEvidence {
            column: player_column,
            resident_subchunks: exact_visibility_witness
                .then_some(visibility_snapshot.resident_witness_subchunks)
                .flatten(),
            allocated_subchunks: u32::try_from(
                chunks
                    .iter()
                    .filter(|instance| instance.key().chunk() == player_column)
                    .count(),
            )
            .unwrap_or(u32::MAX),
            visible_subchunks: exact_visibility_witness
                .then_some(visibility_snapshot.frustum_witness_subchunks)
                .flatten(),
            submitted_subchunks: exact_visibility_witness
                .then_some(visibility_snapshot.submitted_witness_subchunks)
                .flatten(),
            gpu_presented_subchunks: exact_visibility_witness
                .then_some(visibility_snapshot.gpu_completed_witness_subchunks)
                .flatten(),
        };
        if let Some(marker) = phase2_publication_line_if_changed(
            &mut sampling.last_phase2_snapshot,
            CombinedPhase2Snapshot {
                publication,
                presentation,
                player_column_presentation,
                present_mode_proven: graphics.present_mode_proven,
                client_blob_cache_enabled: client_world.client_blob_cache_enabled,
                client_blob_cache: client_world.client_blob_cache,
            },
        ) {
            let mut stdout = diagnostics::console::stdout();
            write_stdout_marker(&mut stdout, &marker);
            let observed_unix_ms = std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .map(|duration| u64::try_from(duration.as_millis()).unwrap_or(u64::MAX))
                .unwrap_or(0);
            write_stdout_marker(
                &mut stdout,
                &acceptance::phase2_evidence::phase2_publication_timing_line(
                    &marker,
                    observed_unix_ms,
                ),
            );
        }
    }
    if client_world.stream.is_some() && visibility_snapshot.frame_generation != 0 {
        let cohort = render_metrics.frame_poll.cohort_progress;
        let count = |digest: Option<render_model::VisibilityKeyDigest>| {
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
        #[cfg(feature = "acceptance")]
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
                let mut stdout = diagnostics::console::stdout();
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
            let mut stdout = diagnostics::console::stdout();
            write_stdout_marker(&mut stdout, &marker);
            write_stdout_marker(
                &mut stdout,
                &adaptive_publication_diagnostic_line(render_metrics.publication.diagnostics()),
            );
        }
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
            let mut stdout = diagnostics::console::stdout();
            write_stdout_marker(&mut stdout, &marker);
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
        let mut stdout = diagnostics::console::stdout();
        write_stdout_marker(&mut stdout, &marker);
        sampling.last_marked_transparent_sort_generation =
            transparent_sort_snapshot.presented_generation;
    }
    #[cfg(feature = "acceptance")]
    acceptance::witness_markers::emit_witness_observations(
        &transparent_witness,
        &model_witness,
        |key| chunks.iter().any(|instance| instance.key() == key),
        |key| cache.visible.contains(&key),
    );
    let stream_errors = client_world.stream.as_ref().map_or(0, |stream| {
        let stats = stream.stats();
        metrics.0.record_pipeline_snapshot(PipelineMetricsSnapshot {
            world_ready: {
                #[cfg(feature = "acceptance")]
                {
                    acceptance.world_ready
                }
                #[cfg(not(feature = "acceptance"))]
                {
                    false
                }
            },
            requested_radius_chunks: diagnostics::PHASE0_REQUESTED_RADIUS_CHUNKS,
            received_radius_chunks: stats.received_radius_chunks,
            publisher_radius_chunks: stats.publisher_radius_chunks,
            mutation_coordinate: {
                #[cfg(feature = "acceptance")]
                {
                    acceptance.mutation_coordinate()
                }
                #[cfg(not(feature = "acceptance"))]
                {
                    None
                }
            },
            visible_mutation_count: {
                #[cfg(feature = "acceptance")]
                {
                    acceptance.visible_mutation_count()
                }
                #[cfg(not(feature = "acceptance"))]
                {
                    0
                }
            },
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
}

pub(crate) fn publish_runtime_stage_profile(profiler: Option<Res<RuntimeStageProfiler>>) {
    let Some(snapshot) = profiler
        .as_deref()
        .and_then(|profiler| profiler.take_snapshot_if_due(Duration::from_secs(1)))
    else {
        return;
    };
    let mut line = format!(
        "{STAGE_PROFILE} interval_ms={:.3}",
        snapshot.interval.as_secs_f64() * 1_000.0
    );
    for (stage, sample) in RuntimeStage::ALL.into_iter().zip(snapshot.samples) {
        let _ = write!(
            line,
            " {}={},{:.3},{:.3}",
            stage.name(),
            sample.count,
            sample.total.as_secs_f64() * 1_000.0,
            sample.maximum.as_secs_f64() * 1_000.0,
        );
    }
    if let Some(slow) = profiler
        .as_deref()
        .and_then(RuntimeStageProfiler::slow_frame_counts)
    {
        let _ = write!(
            line,
            " slow_frames={},{},{}",
            slow.slow, slow.hitches, slow.hard_hitches
        );
    }
    eprintln!("{line}");
}

pub(crate) fn gpu_pass_measurement(
    diagnostics: &DiagnosticsStore,
    path: &DiagnosticPath,
) -> Option<GpuPassMeasurement> {
    diagnostics
        .get_measurement(path)
        .map(|measurement| GpuPassMeasurement::new(measurement.time, measurement.value))
}

use diagnostics::transparent_sort_committed_marker;

/// Releases acknowledged tick records when the optional evidence consumer is absent.
#[cfg(not(feature = "acceptance"))]
pub(crate) fn discard_completed_movement_evidence(mut movement: ResMut<MovementTicker>) {
    let _ = movement.take_tick_evidence();
}
