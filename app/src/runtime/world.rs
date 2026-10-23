use crate::runtime::network::resource_packs::PackApplication;
#[cfg(feature = "acceptance")]
use crate::runtime::visibility::AppMetrics;
#[cfg(feature = "acceptance")]
use ::acceptance::AcceptanceRun;
use client_presentation::environment::{WeatherState, WorldClock, apply_environment_control};
use client_runtime::network::NetworkHandle;
#[cfg(feature = "acceptance")]
use {
    crate::runtime::movement_evidence::MovementEvidenceEmitter,
    acceptance::movement_evidence::MovementEvidenceEventKind,
};
#[cfg(feature = "acceptance")]
use {
    acceptance::model_witness::ModelWitnessFileSource,
    acceptance::mutation::deterministic_mutation_coordinate, diagnostics::write_stdout_marker,
};
pub(crate) mod camera_environment;
pub(super) mod committed_ui;
mod control_apply;
mod dimension;
mod input_locks;
mod local_retention;
mod respawn;
pub(crate) use committed_ui::drain_committed_ui_before_authority;
use committed_ui::refresh_player_list_cache_for_controls;
pub(crate) use dimension::advance_dimension_transfer;
#[cfg(test)]
mod player_list_tests;
mod service;
mod sub_chunk_requests;
mod terrain_generation;
pub(crate) use service::configure_world_service;
pub(crate) use sub_chunk_requests::flush_sub_chunk_requests;
pub(crate) use sub_chunk_requests::sub_chunk_request_center;
pub(crate) use terrain_generation::synchronize_terrain_generation;

#[cfg(feature = "acceptance")]
use acceptance::committed_control::{
    model_gallery_camera_committed_marker, refresh_mutation_anchor_from_committed_control,
};
pub(crate) use control_apply::apply_committed_control;

use std::sync::Arc;

use assets::{RuntimeAssets, RuntimeEntityAssets};
use bevy::{
    ecs::system::SystemParam,
    log::info,
    prelude::{DetectChangesMut, Local, MessageWriter, Res, ResMut, Resource, Time, Vec3},
    time::{Real, Virtual},
};
use chunk_pipeline::{
    CohortProgress, ViewCohortStatus, WorldMeshChange, WorldStream, WorldStreamPoll,
};
use client_world::CommittedControlEvent;

use client_audio::audio_ingress::{SequencedAudioEvent, drain_committed_audio};
use client_presentation::server_camera::{ServerCameraInstructions, drain_committed_camera};
use client_ui::block_cracks::reconcile_world_block_cracks;
use protocol::BlobCacheStats;
use render_scene::{
    ChunkBiomeTints, ChunkRenderQueue, ChunkUploadAcknowledgements, ChunkUploadBudget,
    ChunkUploadPriority, ChunkUploadToken, RuntimeStage, RuntimeStageProfiler,
    VisibilityDiagnosticsInput,
};

use client_shell::ui_resources::UiRuntimeResource;
use {
    crate::runtime::{
        shutdown::{FatalError, record_fatal_error},
        visibility::{CaveVisibilityCache, DiagnosticQuads},
    },
    client_interaction::movement::{
        LocalMovementEffectTimeline, LocalMovementSpeedAuthority, LocalPhysicsController,
        MovementTicker, PhysicsCollisionRegistries,
    },
    client_presentation::{
        camera::CameraSettingsAuthority,
        local_player::{
            InteractionOriginSnapshot, LocalPlayerFrameCarrier, LocalPlayerFrameReset,
            LocalViewPose,
        },
    },
    client_runtime::{
        ingress::OUTBOUND_SEND_BUDGET_PER_FRAME,
        publication::{PublicationController, PublicationFrameWork},
    },
};

#[cfg(feature = "acceptance")]
fn position_distance(from: [f32; 3], to: [f32; 3]) -> f32 {
    let delta = Vec3::from_array(to) - Vec3::from_array(from);
    delta.length()
}

#[derive(Resource, Debug, Default)]
pub(crate) struct WorldStreamFramePoll {
    pub(crate) report: WorldStreamPoll,
    /// Committed-view readiness while startup or diagnostics watch it.
    pub(crate) cohort_progress: Option<CohortProgress>,
    /// The full committed-view witness, only while acceptance or metrics consume it.
    pub(crate) cohort: Option<ViewCohortStatus>,
}

/// A latched, bounded server-directed transfer target awaiting the launcher's
/// replacement handoff. `None` once consumed.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct TransferNotice {
    pub(crate) host: String,
    pub(crate) port: u16,
}

#[derive(Resource)]
pub(crate) struct ClientWorld {
    pub(crate) stream: Option<WorldStream>,
    pub(crate) movement_colliders: crate::movement::MovementColliderCache,
    pub(crate) runtime_assets: Arc<RuntimeAssets>,
    pub(crate) entity_assets: Option<Arc<RuntimeEntityAssets>>,
    /// The session's server-pack entities, layered over `entity_assets`.
    pub(crate) pack_entities: Option<Arc<assets::SessionEntityPack>>,
    /// Worker-computed terrain inputs used to select the far-terrain cache namespace.
    pub(crate) terrain_resource_identity: [u8; 32],
    /// Worker-built pack pages, reused only while their base artwork and pack remain current.
    pub(crate) prepared_actor_artwork:
        Option<Arc<client_presentation::prepared_actor_artwork::PreparedActorArtwork>>,
    /// The session's custom item facts and pack icons for held and worn items.
    pub(crate) session_items: Option<Arc<client_presentation::session_assets::SessionItems>>,
    /// Item tag programs compiled with this session's block registries.
    pub(crate) mining_filters: Arc<pack_runtime::CustomMiningFilters>,
    pub(crate) pending_surface_spawn: Option<[i32; 2]>,
    pub(crate) dimension_transfer: dimension::DimensionTransfer,
    pub(crate) respawn: respawn::RespawnLifecycle,
    pub(crate) transfer_notice: Option<TransferNotice>,
    pub(crate) network_decode_errors: u64,
    pub(crate) client_blob_cache_enabled: bool,
    pub(crate) client_blob_cache: BlobCacheStats,
    /// StartGame's hashed world identity, absent when no level ID was supplied.
    pub(crate) level_identity: Option<u64>,
}

impl Default for ClientWorld {
    fn default() -> Self {
        Self::new(Arc::new(RuntimeAssets::diagnostic()))
    }
}

impl ClientWorld {
    pub(crate) fn new(runtime_assets: Arc<RuntimeAssets>) -> Self {
        Self {
            stream: None,
            movement_colliders: Default::default(),
            runtime_assets,
            entity_assets: None,
            pack_entities: None,
            terrain_resource_identity: [0; 32],
            prepared_actor_artwork: None,
            session_items: None,
            mining_filters: Arc::default(),
            pending_surface_spawn: None,
            dimension_transfer: dimension::DimensionTransfer::default(),
            respawn: respawn::RespawnLifecycle::default(),
            transfer_notice: None,
            network_decode_errors: 0,
            client_blob_cache_enabled: false,
            client_blob_cache: BlobCacheStats::default(),
            level_identity: None,
        }
    }

    pub(crate) fn new_with_entity_assets(
        runtime_assets: Arc<RuntimeAssets>,
        entity_assets: Arc<RuntimeEntityAssets>,
    ) -> Self {
        Self {
            entity_assets: Some(entity_assets),
            ..Self::new(runtime_assets)
        }
    }

    /// Unmapped block lookups on the assets the current session meshes with,
    /// which carry any server block overlay.
    pub(crate) fn missing_asset_count(&self) -> u64 {
        self.stream
            .as_ref()
            .map_or(&self.runtime_assets, |stream| stream.runtime_assets())
            .missing_count()
    }
}

#[derive(SystemParam)]
pub(crate) struct AppWorldState<'w> {
    pub(crate) client_world: ResMut<'w, ClientWorld>,
    pub(crate) fatal_error: ResMut<'w, FatalError>,
    pub(crate) clock: ResMut<'w, WorldClock>,
    pub(crate) weather: ResMut<'w, WeatherState>,
    pub(crate) movement: ResMut<'w, MovementTicker>,
    pub(crate) local_physics: ResMut<'w, LocalPhysicsController>,
    pub(crate) movement_effects: ResMut<'w, LocalMovementEffectTimeline>,
    pub(crate) movement_speed: ResMut<'w, LocalMovementSpeedAuthority>,
    pub(crate) collisions: ResMut<'w, PhysicsCollisionRegistries>,
    pub(crate) ui_runtime: ResMut<'w, UiRuntimeResource>,
    pub(crate) time: Res<'w, Time<Real>>,
}

pub(crate) fn startup_biome_tints(runtime_assets: &RuntimeAssets) -> ChunkBiomeTints {
    let resolved = runtime_assets
        .biome_assets()
        .resolve_live(&[])
        .expect("validated startup biome assets resolve without live definitions");
    ChunkBiomeTints::from_resolved(&resolved, 0)
}

pub(crate) fn synchronize_biome_tints(stream: &WorldStream, active: &mut ChunkBiomeTints) -> bool {
    let identity = stream.biome_tint_identity();
    if active.table_identity() == identity {
        return false;
    }
    let resolved = stream.resolved_biome_tints_snapshot();
    *active = ChunkBiomeTints::from_resolved_with_identity(&resolved, identity);
    true
}

/// Computes startup readiness from required columns and full diagnostics only when enabled.
/// Once startup releases, ordinary play scans neither.
pub(crate) fn frame_cohort_status(
    stream: &WorldStream,
    #[cfg(feature = "acceptance")] acceptance: &AcceptanceRun,
    startup_probe_enabled: bool,
) -> (Option<CohortProgress>, Option<ViewCohortStatus>) {
    let diagnostics_enabled = {
        #[cfg(feature = "acceptance")]
        {
            super::telemetry::publication_diagnostics_enabled(acceptance)
        }
        #[cfg(not(feature = "acceptance"))]
        {
            false
        }
    };
    let Some(target) = stream.committed_view_cohort() else {
        return (None, None);
    };
    if diagnostics_enabled {
        let status = stream.cohort_status(target);
        (Some(status.into()), Some(status))
    } else if startup_probe_enabled {
        (Some(stream.cohort_progress(target)), None)
    } else {
        (None, None)
    }
}

pub(crate) fn world_stream_fatal_message(error: chunk_pipeline::WorldStreamFatalError) -> String {
    format!("world stream fatal: {error}")
}

const MISSING_PUBLICATION_PERMIT_ERROR: &str =
    "world stream produced a render update without the required publication permit";

pub(crate) fn mesh_change_has_publication_permit(change: &WorldMeshChange) -> bool {
    match change {
        WorldMeshChange::Upsert { permit, .. } | WorldMeshChange::Remove { permit, .. } => {
            permit.is_some()
        }
    }
}

/// Diagnostics, player authority and per-system correction history used while polling the world.
#[derive(SystemParam)]
pub(crate) struct WorldPollInputs<'w, 's> {
    visibility_diagnostics: Option<Res<'w, VisibilityDiagnosticsInput>>,
    profiler: Option<Res<'w, RuntimeStageProfiler>>,
    player_runtime: ResMut<'w, client_state::PlayerRuntime>,
    server_camera_view: Option<Res<'w, client_presentation::camera::ServerCameraView>>,
    correction_log: Local<'s, gameplay::movement::CorrectionContextLog>,
    item_use: Option<ResMut<'w, client_interaction::item_use::ItemUseRuntime>>,
    locomotion: ResMut<'w, client_interaction::movement::LocomotionState>,
    look_smoother: ResMut<'w, client_presentation::camera::look::LookSmoother>,
    #[cfg(not(target_arch = "wasm32"))]
    menu: Option<Res<'w, client_shell::menu::MenuRuntime>>,
    #[cfg(target_arch = "wasm32")]
    options: Res<'w, crate::browser_input::BrowserOptions>,
    render_queue: Option<ResMut<'w, ChunkRenderQueue>>,
}

#[allow(clippy::too_many_arguments)]
pub(crate) fn reconcile_world_stream_before_physics(
    state: AppWorldState,
    network: Option<Res<NetworkHandle<PackApplication>>>,
    #[cfg(feature = "acceptance")] mut acceptance: ResMut<AcceptanceRun>,
    upload_budget: Res<ChunkUploadBudget>,
    #[cfg(feature = "acceptance")] model_witness_source: Res<ModelWitnessFileSource>,
    mut camera_settings: ResMut<CameraSettingsAuthority>,
    mut view: ResMut<LocalViewPose>,
    mut local_frame: ResMut<LocalPlayerFrameCarrier>,
    mut interaction: ResMut<InteractionOriginSnapshot>,
    #[cfg(feature = "acceptance")] mut movement_evidence: ResMut<MovementEvidenceEmitter>,
    mut frame_poll: ResMut<WorldStreamFramePoll>,
    mut audio: MessageWriter<SequencedAudioEvent>,
    mut server_camera: ResMut<ServerCameraInstructions>,
    mut camera_hurt: Option<ResMut<client_presentation::camera::CameraHurtState>>,
    mut particle_inbox: Option<ResMut<crate::particles::ParticleInbox>>,
    inputs: WorldPollInputs<'_, '_>,
) {
    let WorldPollInputs {
        visibility_diagnostics,
        profiler,
        mut player_runtime,
        server_camera_view,
        mut correction_log,
        mut item_use,
        mut locomotion,
        mut look_smoother,
        #[cfg(not(target_arch = "wasm32"))]
        menu,
        #[cfg(target_arch = "wasm32")]
        options,
        mut render_queue,
    } = inputs;
    let _timer = profiler
        .as_deref()
        .map(|profiler| profiler.time(RuntimeStage::WorldPoll));
    let AppWorldState {
        mut client_world,
        mut fatal_error,
        mut clock,
        mut weather,
        mut movement,
        mut local_physics,
        mut movement_effects,
        mut movement_speed,
        mut ui_runtime,
        collisions,
        time,
        ..
    } = state;
    let ClientWorld {
        stream,
        movement_colliders,
        pending_surface_spawn,
        dimension_transfer,
        respawn,
        ..
    } = &mut *client_world;
    let Some(stream) = stream.as_mut() else {
        *frame_poll = WorldStreamFramePoll::default();
        local_frame.reset(LocalPlayerFrameReset::Session);
        interaction.invalidate();
        return;
    };
    stream.set_view_forward((view.camera_rotation() * Vec3::NEG_Z).to_array());
    #[cfg(not(target_arch = "wasm32"))]
    let options = menu.as_ref().map(|menu| menu.settings_snapshot().0);
    #[cfg(target_arch = "wasm32")]
    let options = Some(Arc::clone(&options.0));
    let rendering = options
        .as_ref()
        .map_or_else(meshing::MeshRenderSettings::default, |options| {
            use launcher::menu::settings_options::SettingId;
            meshing::MeshRenderSettings {
                smooth_lighting: options.value(SettingId::SmoothLighting) != 0,
                transparent_leaves: options.value(SettingId::TransparentLeaves) != 0,
            }
        });
    if stream.set_mesh_render_settings(rendering, web_time::Instant::now())
        && let Some(queue) = render_queue.as_mut()
    {
        queue.discard_resource_work();
    }
    // Adds to the between-frames service's work already recorded for this frame.
    frame_poll.report.accumulate(stream.poll(
        view.eye_translation().to_array(),
        upload_budget.max_per_frame,
    ));
    (frame_poll.cohort_progress, frame_poll.cohort) = frame_cohort_status(
        stream,
        #[cfg(feature = "acceptance")]
        &acceptance,
        visibility_diagnostics.is_some_and(|diagnostics| diagnostics.startup_probe_enabled()),
    );
    drain_committed_audio(stream, |event| {
        audio.write(event);
    });
    if let Some(inbox) = particle_inbox.as_mut() {
        crate::particles::drain_committed_particles(stream, inbox);
    }
    drain_committed_camera(
        stream,
        clock.session_generation(),
        stream.current_dimension(),
        &mut server_camera,
    );
    if let Some(error) = stream.take_fatal_error() {
        movement.deactivate();
        local_physics.deactivate();
        local_frame.reset(LocalPlayerFrameReset::Session);
        interaction.invalidate();
        record_fatal_error(&mut fatal_error.0, world_stream_fatal_message(error));
        return;
    }

    respawn.synchronize_session(Some(clock.session_generation()));
    let controls = stream.take_committed_controls();
    respawn.retain_committed_input_hold(&controls);
    if network
        .as_ref()
        .is_some_and(|network| network.flush_latency_reply().is_err())
    {
        movement.set_control_fence_pending(true);
        stream.restore_committed_controls(controls.into_iter());
        return;
    }
    movement.set_control_fence_pending(false);
    refresh_player_list_cache_for_controls(stream, &mut ui_runtime, &controls);

    let mut controls = controls.into_iter();
    while let Some(control) = controls.next() {
        if let Err(error) = respawn.queue_completion(|packet| match network.as_ref() {
            Some(network) => network.send_dimension_packet(packet),
            None => Err(client_session::PacketSendError::Full(packet)),
        }) {
            if error.is_closed() {
                record_fatal_error(&mut fatal_error.0, format!("respawn completion: {error}"));
            }
            movement.set_control_fence_pending(true);
            stream.restore_committed_controls(std::iter::once(control).chain(controls));
            return;
        }
        if let CommittedControlEvent::ClientInputLocks { locks, .. } = control {
            input_locks::apply_committed_input_locks(
                clock.session_generation(),
                locks,
                &mut player_runtime.facts,
                &mut local_physics,
                &mut locomotion,
                &mut look_smoother,
            );
            continue;
        }
        if let CommittedControlEvent::DimensionChangeAck {
            dimension_epoch, ..
        } = control
        {
            dimension_transfer.acknowledge(dimension_epoch);
            continue;
        }
        if let CommittedControlEvent::NetworkStackLatency { creation_time, .. } = control {
            // Earlier controls already applied; the reply and later ones wait for older inputs to send.
            if movement.has_unsent_inputs() {
                movement.set_control_fence_pending(true);
                stream.restore_committed_controls(std::iter::once(control).chain(controls));
                return;
            }
            let Some(network) = network.as_ref() else {
                movement.set_control_fence_pending(true);
                stream.restore_committed_controls(std::iter::once(control).chain(controls));
                return;
            };
            match network.send_latency_reply(creation_time) {
                Ok(()) => {
                    movement.set_control_fence_pending(network.has_pending_latency_reply());
                    gameplay::movement::trace_server_control(&movement, &local_physics, &control)
                }
                Err(client_session::BatchSendError::Full) => {
                    movement.set_control_fence_pending(true);
                    stream.restore_committed_controls(std::iter::once(control).chain(controls));
                    return;
                }
                Err(client_session::BatchSendError::Closed) => return,
            }
            continue;
        }
        gameplay::movement::trace_server_control(&movement, &local_physics, &control);
        if let Some(packet) = gameplay::movement::CorrectionPacket::from_control(&control) {
            let camera = server_camera_view.as_deref();
            correction_log.note(
                movement.session_generation(),
                web_time::Instant::now(),
                packet,
                gameplay::movement::CorrectionContext::read(
                    &player_runtime.facts,
                    movement.physics_is_authorized(),
                    camera.and_then(|camera| camera.active_preset_name()),
                    camera.is_some_and(|camera| camera.has_pose_override()),
                ),
            );
        }
        if let CommittedControlEvent::LocalMovementFlags { tick, flags, .. } = control {
            if let Some(item_use) = item_use.as_deref_mut() {
                item_use.adopt_server_using_item(flags.using_item);
            }
            let previous = player_runtime.facts.is_immobile();
            player_runtime
                .facts
                .apply_local_movement_flags(clock.session_generation(), flags);
            if previous != player_runtime.facts.is_immobile() {
                info!(
                    immobile = player_runtime.facts.is_immobile(),
                    tick, "server changed local player immobility"
                );
            }
        }
        if let CommittedControlEvent::LocalMovementSpeed {
            air_drag_modifier: Some(current),
            ..
        } = control
        {
            player_runtime
                .facts
                .apply_air_drag_modifier(clock.session_generation(), current);
        }
        if respawn.consume_nonspatial_phase(
            clock.session_generation(),
            &control,
            stream.local_player_runtime_id(),
            &mut movement,
            &mut local_physics,
            item_use.as_deref_mut(),
        ) {
            continue;
        }
        if let CommittedControlEvent::ChangeDimension {
            sequence,
            change,
            resolved,
        } = control
        {
            dimension_transfer.begin(
                clock.session_generation(),
                sequence,
                protocol::ChangeDimensionEvent {
                    position: resolved.position,
                    ..change
                },
                stream.local_player_runtime_id(),
                time.elapsed(),
            );
        }
        let world = crate::movement::collision_world(
            stream,
            collisions.registry(stream.network_id_mode()),
            movement_colliders,
        );
        let disposition = gameplay::committed_control::CommittedGameplayState {
            movement: &mut movement,
            physics: &mut local_physics,
            effects: &mut movement_effects,
            speed: &mut movement_speed,
            session_generation: clock.session_generation(),
            dimension: stream.current_dimension(),
            dimension_transfer_active: dimension_transfer.active(),
        }
        .apply(control, &world, |observation| {
            use gameplay::committed_control::ControlObservation;
            match observation {
                ControlObservation::Hurt { source_direction } => {
                    if let Some(hurt) = camera_hurt.as_deref_mut() {
                        hurt.register(client_presentation::camera::LocalHurtEvent {
                            source_direction,
                            ..Default::default()
                        });
                    }
                }
                ControlObservation::Knockback { motion } => {
                    if let Some(hurt) = camera_hurt.as_deref_mut() {
                        hurt.note_knockback(motion[0], motion[2]);
                    }
                }
                #[cfg(feature = "acceptance")]
                ControlObservation::BeforeSpatial(control) => {
                    let _ =
                        refresh_mutation_anchor_from_committed_control(&mut acceptance, &control);
                }
                #[cfg(feature = "acceptance")]
                ControlObservation::Correction {
                    outcome,
                    previous,
                    position,
                } => {
                    movement_evidence
                        .note_correction(outcome, position_distance(previous, position));
                }
                #[cfg(feature = "acceptance")]
                ControlObservation::Dimension => {
                    movement_evidence.note_event(MovementEvidenceEventKind::Dimension);
                }
                #[cfg(not(feature = "acceptance"))]
                ControlObservation::BeforeSpatial(_)
                | ControlObservation::Correction { .. }
                | ControlObservation::Dimension => {}
            }
        });
        use gameplay::committed_control::{ControlDisposition, SpatialReset};
        let reset = match disposition {
            ControlDisposition::Handled => continue,
            ControlDisposition::Environment => {
                apply_environment_control(
                    control,
                    &mut clock,
                    &mut weather,
                    time.elapsed_secs_f64(),
                );
                continue;
            }
            ControlDisposition::Spatial(SpatialReset::Correction) => {
                LocalPlayerFrameReset::Correction
            }
            ControlDisposition::Spatial(SpatialReset::Dimension) => {
                LocalPlayerFrameReset::Dimension
            }
        };
        local_frame.reset(reset);
        interaction.invalidate();
        #[cfg(feature = "acceptance")]
        let _ = acceptance.observe_committed_full_view_control(&control);
        #[cfg(feature = "acceptance")]
        let camera_marker =
            model_gallery_camera_committed_marker(model_witness_source.configured(), &control);
        apply_committed_control(
            control,
            &mut view,
            &mut camera_settings,
            pending_surface_spawn,
        );
        #[cfg(feature = "acceptance")]
        if let Some(marker) = camera_marker {
            let mut stdout = diagnostics::console::stdout();
            write_stdout_marker(&mut stdout, &marker);
        }
    }
}

#[allow(clippy::too_many_arguments)]
pub(crate) fn drive_world_stream(
    network: Res<NetworkHandle<PackApplication>>,
    state: AppWorldState,
    #[cfg(feature = "acceptance")] mut acceptance: ResMut<AcceptanceRun>,
    #[cfg(feature = "acceptance")] mut metrics: ResMut<AppMetrics>,
    mut render_queue: ResMut<ChunkRenderQueue>,
    mut biome_tints: ResMut<ChunkBiomeTints>,
    mut diagnostic_quads: ResMut<DiagnosticQuads>,
    acknowledgements: Res<ChunkUploadAcknowledgements>,
    mut publication: ResMut<PublicationController>,
    mut view: ResMut<LocalViewPose>,
    mut frame_poll: ResMut<WorldStreamFramePoll>,
    mut rendered_session: Local<Option<(u64, u64)>>,
    mut visibility: ResMut<CaveVisibilityCache>,
    camera_publication: Option<
        Res<client_presentation::local_player_camera_receipt::CameraPublicationAttempt>,
    >,
    profiler: Option<Res<RuntimeStageProfiler>>,
    (frame, mut block_use, simulation_time): (
        Res<bevy::diagnostic::FrameCount>,
        ResMut<client_interaction::block_use::BlockUseRuntime>,
        Res<Time<Virtual>>,
    ),
) {
    let _timer = profiler
        .as_deref()
        .map(|profiler| profiler.time(RuntimeStage::WorldStream));
    let AppWorldState {
        mut client_world,
        mut fatal_error,
        mut local_physics,
        mut movement,
        mut ui_runtime,
        clock,
        time,
        ..
    } = state;
    let active_session = client_world
        .stream
        .as_ref()
        .map(WorldStream::terrain_generation);
    if synchronize_terrain_generation(
        &mut rendered_session,
        active_session,
        &mut render_queue,
        &acknowledgements,
    ) {
        *visibility = CaveVisibilityCache::default();
        diagnostic_quads.0.clear();
    }
    let Some(stream) = client_world.stream.as_mut() else {
        ui_runtime.clear_disconnected_block_cracks();
        return;
    };
    let prediction_budget = block_use.prediction_frames.publication_budget(frame.0);
    if prediction_budget != 0 {
        let report =
            stream.poll_prediction_jobs(view.eye_translation().to_array(), prediction_budget);
        frame_poll.report.light_results += report.light_results;
        frame_poll.report.light_jobs_dispatched += report.light_jobs_dispatched;
        frame_poll.report.mesh_results += report.mesh_results;
        frame_poll.report.mesh_jobs_dispatched += report.mesh_jobs_dispatched;
    }
    let profile = bridge::join_profile::JoinProfile::current();
    use bridge::join_profile::Milestone;
    if frame_poll.report.decoded_results > 0 {
        profile.mark_once(Milestone::ChunkDecoded);
    }
    if frame_poll.report.light_results > 0 {
        profile.mark_once(Milestone::LightingReady);
    }
    if frame_poll.report.mesh_results > 0 {
        profile.mark_once(Milestone::MeshReady);
    }
    local_retention::retain_completed_player_terrain(
        stream,
        &local_physics,
        camera_publication.as_deref(),
        clock.session_generation(),
    );
    // Marked changed only on a new table, so unchanged frames do not re-extract it.
    if synchronize_biome_tints(stream, biome_tints.bypass_change_detection()) {
        biome_tints.set_changed();
    }
    #[cfg(feature = "acceptance")]
    let mutation_cohort = frame_poll.cohort;
    for acknowledgement in acknowledgements.drain() {
        profile.mark_once(Milestone::MeshUploaded);
        block_use.prediction_frames.uploaded(
            frame.0,
            acknowledgement.key,
            acknowledgement.token.generation,
        );
        render_queue.record_gpu_upload_bytes(acknowledgement.uploaded_bytes);
        if let Some(trace) = bridge::latency_trace::active() {
            trace.record(
                bridge::latency_trace::kind::MESH_UPLOADED,
                &[
                    trace.at(acknowledgement.token.dirty_since),
                    trace.at(acknowledgement.applied_at),
                ],
            );
        }
        #[cfg(feature = "acceptance")]
        if let Some(latency) = acceptance.acknowledge_mutation(
            acknowledgement.key,
            acknowledgement.token.generation,
            acknowledgement.token.dirty_since,
            acknowledgement.applied_at,
            mutation_cohort,
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
    let poll_report = std::mem::take(&mut frame_poll.report);
    stream.advance_block_cracks(if simulation_time.is_paused() {
        0.0
    } else {
        time.delta_secs()
    });
    reconcile_world_block_cracks(&mut ui_runtime, stream);
    let camera_position = view.eye_translation();
    let resolved_surface_spawn = client_world.pending_surface_spawn.and_then(|anchor| {
        client_world
            .stream
            .as_ref()
            .and_then(|stream| stream.surface_eye_position(anchor[0], anchor[1]))
    });
    #[cfg(feature = "acceptance")]
    let resolved_mutation_coordinate = acceptance.mutation_surface_anchor().and_then(|anchor| {
        client_world.stream.as_ref().and_then(|stream| {
            stream
                .surface_eye_position(anchor[0], anchor[1])
                .map(|position| deterministic_mutation_coordinate(position, anchor))
        })
    });

    let send_error = client_world.stream.as_mut().and_then(|stream| {
        let actor_position = local_physics
            .network_position()
            .unwrap_or(stream.resolved_server_position().position);
        let center = sub_chunk_request_center(
            stream,
            camera_publication.as_deref(),
            clock.session_generation(),
            actor_position,
        )?;
        flush_sub_chunk_requests(
            stream,
            OUTBOUND_SEND_BUDGET_PER_FRAME,
            center,
            |receipt, packet| network.send_sub_chunk_request(receipt, packet),
        )
        .err()
    });
    let mut published_items = 0_usize;
    let mut published_payload_items = 0_usize;
    let mut published_bytes = 0_u64;
    if let Some(stream) = client_world.stream.as_mut() {
        // Block changes from this frame's interaction or ingress publish once their workers
        // finish, not one poll later.
        stream.service_urgent_work();
        while let Some(change) = stream.pop_mesh_change() {
            if !mesh_change_has_publication_permit(&change) {
                let restored = stream.retry_mesh_change_front(change).is_ok();
                record_fatal_error(
                    &mut fatal_error.0,
                    if restored {
                        MISSING_PUBLICATION_PERMIT_ERROR.to_owned()
                    } else {
                        format!(
                            "{MISSING_PUBLICATION_PERMIT_ERROR}; failed to restore the rejected update to the bounded world retry FIFO"
                        )
                    },
                );
                break;
            }
            let change_bytes = match &change {
                WorldMeshChange::Upsert { mesh, biome, .. } => {
                    ChunkRenderQueue::upload_byte_len(mesh, biome)
                }
                WorldMeshChange::Remove { .. } => 0,
            };
            let retry = match change {
                WorldMeshChange::Upsert {
                    output_permit,
                    key,
                    mesh,
                    biome,
                    tint_identity,
                    generation,
                    dirty_since,
                    urgent,
                    permit,
                } => {
                    // The mesh moves into the render queue, so keep a copy of its diagnostic
                    // geometry only when publishing it would change the tracker.
                    let diagnostic_geometry = (!diagnostic_quads
                        .0
                        .is_current(key, mesh.diagnostic_geometry()))
                    .then(|| mesh.diagnostic_geometry().clone());
                    let publication_permit =
                        permit.expect("publication permit was validated before render handoff");
                    match render_queue.try_update_tracked_with_biome_identity_permitted(
                        key,
                        mesh,
                        biome,
                        tint_identity,
                        if urgent {
                            ChunkUploadPriority::urgent()
                        } else {
                            ChunkUploadPriority::from_camera(key, camera_position)
                        },
                        ChunkUploadToken {
                            generation,
                            dirty_since,
                        },
                        publication_permit,
                    ) {
                        Ok(()) => {
                            block_use.prediction_frames.staged(frame.0, key, generation);
                            if let Some(diagnostic_geometry) = &diagnostic_geometry {
                                diagnostic_quads.0.upsert(key, diagnostic_geometry);
                            }
                            None
                        }
                        Err((mesh, biome, permit)) => Some(WorldMeshChange::Upsert {
                            output_permit,
                            key,
                            mesh,
                            biome,
                            tint_identity,
                            generation,
                            dirty_since,
                            urgent,
                            permit: Some(permit),
                        }),
                    }
                }
                WorldMeshChange::Remove {
                    key,
                    generation,
                    dirty_since,
                    urgent,
                    permit,
                } => {
                    let publication_permit =
                        permit.expect("publication permit was validated before render handoff");
                    match render_queue.try_remove_tracked_permitted(
                        key,
                        if urgent {
                            ChunkUploadPriority::urgent()
                        } else {
                            ChunkUploadPriority::from_camera(key, camera_position)
                        },
                        ChunkUploadToken {
                            generation,
                            dirty_since,
                        },
                        stream.terrain_removal_kind(key),
                        publication_permit,
                    ) {
                        Ok(()) => {
                            diagnostic_quads.0.remove(key);
                            None
                        }
                        Err((key, permit)) => Some(WorldMeshChange::Remove {
                            key,
                            generation,
                            dirty_since,
                            urgent,
                            permit: Some(permit),
                        }),
                    }
                }
            };
            let Some(retry) = retry else {
                published_items = published_items.saturating_add(1);
                if change_bytes != 0 {
                    published_payload_items = published_payload_items.saturating_add(1);
                    published_bytes = published_bytes.saturating_add(change_bytes);
                }
                continue;
            };
            if stream.retry_mesh_change_front(retry).is_err() {
                fatal_error.0 = Some(
                    "failed to restore a render update to the bounded world retry FIFO".to_owned(),
                );
            }
            break;
        }
    }
    if let Some(stream) = client_world.stream.as_ref() {
        let (pending_mesh_jobs, in_flight_mesh_jobs) = stream.mesh_job_counts();
        let previous = publication.diagnostics().last_work;
        let allowance = publication.allowance();
        publication.finish_frame(PublicationFrameWork {
            mesh_jobs_dispatched: poll_report.mesh_jobs_dispatched,
            mesh_changes_published: published_items,
            mesh_payloads_published: published_payload_items,
            mesh_bytes_published: published_bytes,
            pending_mesh_jobs,
            in_flight_mesh_jobs,
            upload_queue_items: render_queue.retained_len(),
            upload_queue_bytes: render_queue.pending_bytes(),
            allowance_live_permits: allowance.live_permits(),
            allowance_live_payload_bytes: allowance.live_payload_bytes(),
            stream_pending_mesh_changes: stream.pending_mesh_change_count(),
            ..previous
        });
    }
    if let Some(error) = send_error {
        record_fatal_error(&mut fatal_error.0, error);
    }
    if let Some(position) = resolved_surface_spawn {
        view.reanchor_camera();
        view.set_eye_translation(Vec3::from_array(position));
        let tick = local_physics.state().map_or(0, |state| state.tick);
        // World publication runs after local physics. The next frame's delta
        // starts after this anchor, so it must remain eligible for simulation.
        movement.reanchor_surface_spawn(tick, position);
        local_physics.reanchor_network_position(position, tick, true);
        movement.enforce_local_physics_authority(&mut local_physics);
        client_world.pending_surface_spawn = None;
        info!(position = ?position, "resolved temporary Bedrock spawn from packed terrain");
    }
    #[cfg(feature = "acceptance")]
    if let Some(coordinate) = resolved_mutation_coordinate {
        acceptance.set_mutation_coordinate(coordinate);
    }
}
