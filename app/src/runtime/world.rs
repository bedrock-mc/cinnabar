#[cfg(feature = "acceptance")]
use crate::acceptance::{
    AcceptanceRun,
    model_witness::ModelWitnessFileSource,
    mutation::{deterministic_mutation_coordinate, write_stdout_marker},
};
#[cfg(feature = "acceptance")]
use crate::runtime::phase3_evidence::{Phase3EvidenceEmitter, Phase3EvidenceEventKind};
#[cfg(feature = "acceptance")]
use crate::runtime::visibility::AppMetrics;
mod committed_ui;
mod control_apply;
mod dimension;
mod local_retention;
mod respawn;
pub(crate) use committed_ui::drain_committed_ui_before_authority;
use committed_ui::refresh_player_list_cache_for_controls;
pub(crate) use dimension::advance_dimension_transfer;
#[cfg(test)]
mod player_list_tests;
mod shutdown_watchdog;
mod sub_chunk_requests;
pub(crate) use sub_chunk_requests::flush_sub_chunk_requests;

#[cfg(feature = "acceptance")]
use acceptance::committed_control::{
    model_gallery_camera_committed_marker, refresh_mutation_anchor_from_committed_control,
};
pub(crate) use control_apply::apply_committed_control;
pub(crate) use shutdown_watchdog::{
    SHUTDOWN_WATCHDOG_TIMEOUT, ShutdownWatchdog, TeardownWatchdog, app_exit_code,
    arm_shutdown_watchdog, begin_bounded_shutdown,
};

use std::sync::Arc;

use assets::{RuntimeAssets, RuntimeEntityAssets};
use bevy::{
    ecs::system::SystemParam,
    log::info,
    prelude::{Local, MessageWriter, Query, Res, ResMut, Resource, Time, Transform, Vec3, With},
    time::Real,
};
use chunk_pipeline::{ViewCohortStatus, WorldMeshChange, WorldStream, WorldStreamPoll};
use client_world::CommittedControlEvent;

use client_presentation::audio_ingress::{SequencedAudioEvent, drain_committed_audio};
use client_presentation::server_camera::{ServerCameraInstructions, drain_committed_camera};
use client_ui::block_cracks::reconcile_world_block_cracks;
use meshing::CameraMedium;
use protocol::BlobCacheStats;
use render::{
    ChunkBiomeTints, ChunkRenderQueue, ChunkUploadAcknowledgements, ChunkUploadBudget,
    ChunkUploadPriority, ChunkUploadToken, RuntimeStage, RuntimeStageProfiler,
    VisibilityDiagnosticsInput,
};

use crate::{
    camera::{CameraSettingsAuthority, FlyCamera},
    environment::{self, WeatherState, WorldClock, apply_environment_control},
    local_player::{
        InteractionOriginSnapshot, LocalPlayerFrameCarrier, LocalPlayerFrameReset, LocalViewPose,
    },
    movement::{
        LocalMovementEffectTimeline, LocalMovementSpeedAuthority, LocalPhysicsController,
        MovementTicker, PhysicsCollisionRegistries,
    },
    runtime::{
        network::{NetworkHandle, OUTBOUND_SEND_BUDGET_PER_FRAME},
        publication::{PublicationController, PublicationFrameWork},
        shutdown::record_fatal_error,
        visibility::{CaveVisibilityCache, DiagnosticQuads},
    },
};
use client_ui::ui_runtime::UiRuntime;

#[cfg(feature = "acceptance")]
fn position_distance(from: [f32; 3], to: [f32; 3]) -> f32 {
    let delta = Vec3::from_array(to) - Vec3::from_array(from);
    delta.length()
}

#[derive(Resource, Debug, Default)]
pub(crate) struct WorldStreamFramePoll {
    pub(crate) report: WorldStreamPoll,
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
    pub(crate) runtime_assets: Arc<RuntimeAssets>,
    pub(crate) entity_assets: Option<Arc<RuntimeEntityAssets>>,
    /// The session's server-pack entities, layered over `entity_assets`.
    pub(crate) pack_entities: Option<Arc<crate::runtime::network::entity_pack::SessionEntityPack>>,
    /// Worker-built pack pages, reused only while their base artwork and pack remain current.
    pub(crate) prepared_actor_artwork:
        Option<Arc<client_presentation::prepared_actor_artwork::PreparedActorArtwork>>,
    /// The session's custom item facts and pack icons for held and worn items.
    pub(crate) session_items: Option<Arc<crate::runtime::network::entity_pack::SessionItems>>,
    pub(crate) pending_surface_spawn: Option<[i32; 2]>,
    pub(crate) dimension_transfer: dimension::DimensionTransfer,
    pub(crate) respawn: respawn::RespawnLifecycle,
    pub(crate) fatal_error: Option<String>,
    pub(crate) transfer_notice: Option<TransferNotice>,
    pub(crate) network_decode_errors: u64,
    pub(crate) reported_decode_errors: u64,
    pub(crate) client_blob_cache_enabled: bool,
    pub(crate) client_blob_cache: BlobCacheStats,
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
            runtime_assets,
            entity_assets: None,
            pack_entities: None,
            prepared_actor_artwork: None,
            session_items: None,
            pending_surface_spawn: None,
            dimension_transfer: dimension::DimensionTransfer::default(),
            respawn: respawn::RespawnLifecycle::default(),
            fatal_error: None,
            transfer_notice: None,
            network_decode_errors: 0,
            reported_decode_errors: 0,
            client_blob_cache_enabled: false,
            client_blob_cache: BlobCacheStats::default(),
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
    pub(crate) clock: ResMut<'w, WorldClock>,
    pub(crate) weather: ResMut<'w, WeatherState>,
    pub(crate) movement: ResMut<'w, MovementTicker>,
    pub(crate) local_physics: ResMut<'w, LocalPhysicsController>,
    pub(crate) movement_effects: ResMut<'w, LocalMovementEffectTimeline>,
    pub(crate) movement_speed: ResMut<'w, LocalMovementSpeedAuthority>,
    pub(crate) collisions: ResMut<'w, PhysicsCollisionRegistries>,
    pub(crate) ui_runtime: ResMut<'w, UiRuntime>,
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

pub(crate) fn update_camera_medium(
    client_world: Res<ClientWorld>,
    camera: Query<&Transform, With<FlyCamera>>,
    mut medium: ResMut<environment::CameraMediumState>,
    mut context: ResMut<environment::EnvironmentContext>,
    mut precipitation: Local<environment::FogPrecipitationSamples>,
) {
    let Some((stream, camera)) = client_world.stream.as_ref().zip(camera.single().ok()) else {
        medium.0 = CameraMedium::Air;
        *context = environment::EnvironmentContext::default();
        return;
    };
    let position = camera.translation.to_array();
    medium.0 = stream.camera_medium(position);
    let camera_biome = stream
        .camera_biome_id(camera.translation.to_array())
        .and_then(|raw_id| {
            let rules = &client_world.runtime_assets.biome_assets().rules;
            rules
                .binary_search_by_key(&raw_id, |rule| rule.id)
                .ok()
                .map(|index| &rules[index])
        });
    *context = environment::EnvironmentContext {
        dimension: stream.current_dimension(),
        fog_biomes: environment::fog_biome_samples(stream, &client_world.runtime_assets, position),
        precipitation_sample_count: precipitation.count(
            stream,
            &client_world.runtime_assets,
            position,
        ),
        camera_biome_identifier: camera_biome.map(|rule| rule.name.clone()),
        camera_biome_temperature: camera_biome.map(|rule| rule.temperature()),
        render_distance_blocks: Some(stream.render_distance_blocks()),
    };
}

/// Full-world cohort witness for startup, acceptance and metrics. Normal play
/// stops scanning retained columns and sub-chunks once startup releases.
pub(crate) fn frame_cohort_status(
    stream: &WorldStream,
    #[cfg(feature = "acceptance")] acceptance: &AcceptanceRun,
    startup_probe_enabled: bool,
) -> Option<ViewCohortStatus> {
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
    if !startup_probe_enabled && !diagnostics_enabled {
        return None;
    }
    stream
        .committed_view_cohort()
        .map(|target| stream.cohort_status(target))
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

#[allow(clippy::too_many_arguments)]
pub(crate) fn reconcile_world_stream_before_physics(
    state: AppWorldState,
    network: Option<Res<NetworkHandle>>,
    #[cfg(feature = "acceptance")] mut acceptance: ResMut<AcceptanceRun>,
    upload_budget: Res<ChunkUploadBudget>,
    #[cfg(feature = "acceptance")] model_witness_source: Res<ModelWitnessFileSource>,
    mut camera_settings: ResMut<CameraSettingsAuthority>,
    mut view: ResMut<LocalViewPose>,
    mut local_frame: ResMut<LocalPlayerFrameCarrier>,
    mut interaction: ResMut<InteractionOriginSnapshot>,
    #[cfg(feature = "acceptance")] mut phase3_evidence: ResMut<Phase3EvidenceEmitter>,
    mut frame_poll: ResMut<WorldStreamFramePoll>,
    mut audio: MessageWriter<SequencedAudioEvent>,
    mut server_camera: ResMut<ServerCameraInstructions>,
    mut camera_hurt: Option<ResMut<crate::camera::CameraHurtState>>,
    mut particle_inbox: Option<ResMut<crate::particles::ParticleInbox>>,
    (visibility_diagnostics, profiler, mut player_runtime): (
        Option<Res<VisibilityDiagnosticsInput>>,
        Option<Res<RuntimeStageProfiler>>,
        ResMut<crate::player_runtime::PlayerRuntime>,
    ),
) {
    let _timer = profiler
        .as_deref()
        .map(|profiler| profiler.time(RuntimeStage::WorldPoll));
    let AppWorldState {
        mut client_world,
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
        pending_surface_spawn,
        dimension_transfer,
        respawn,
        fatal_error,
        ..
    } = &mut *client_world;
    let Some(stream) = stream.as_mut() else {
        *frame_poll = WorldStreamFramePoll::default();
        local_frame.reset(LocalPlayerFrameReset::Session);
        interaction.invalidate();
        return;
    };
    stream.set_view_forward((view.rotation() * Vec3::NEG_Z).to_array());
    frame_poll.report = stream.poll(
        view.eye_translation().to_array(),
        upload_budget.max_per_frame,
    );
    frame_poll.cohort = frame_cohort_status(
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
        record_fatal_error(fatal_error, world_stream_fatal_message(error));
        return;
    }

    if network
        .as_ref()
        .is_some_and(|network| network.flush_latency_reply().is_err())
    {
        movement.set_control_fence_pending(true);
        return;
    }
    movement.set_control_fence_pending(false);
    let controls = stream.take_committed_controls();
    if movement.has_unsent_inputs()
        && controls
            .iter()
            .any(|control| matches!(control, CommittedControlEvent::NetworkStackLatency { .. }))
    {
        movement.set_control_fence_pending(true);
        stream.restore_committed_controls(controls.into_iter());
        return;
    }
    refresh_player_list_cache_for_controls(stream, &mut ui_runtime, &controls);

    let mut controls = controls.into_iter();
    while let Some(control) = controls.next() {
        if let CommittedControlEvent::DimensionChangeAck {
            dimension_epoch, ..
        } = control
        {
            dimension_transfer.acknowledge(dimension_epoch);
            continue;
        }
        if let CommittedControlEvent::NetworkStackLatency { creation_time, .. } = control {
            let Some(network) = network.as_ref() else {
                movement.set_control_fence_pending(true);
                stream.restore_committed_controls(std::iter::once(control).chain(controls));
                return;
            };
            match network.send_latency_reply(creation_time) {
                Ok(()) => {
                    movement.set_control_fence_pending(network.has_pending_latency_reply());
                    crate::movement::trace_server_control(&movement, &local_physics, &control)
                }
                Err(super::network::BatchSendError::Full) => {
                    movement.set_control_fence_pending(true);
                    stream.restore_committed_controls(std::iter::once(control).chain(controls));
                    return;
                }
                Err(super::network::BatchSendError::Closed) => return,
            }
            continue;
        }
        crate::movement::trace_server_control(&movement, &local_physics, &control);
        if let CommittedControlEvent::LocalMovementFlags { tick, flags, .. } = control {
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
        if let CommittedControlEvent::LocalAirDragModifier { current, .. } = control {
            player_runtime
                .facts
                .apply_air_drag_modifier(clock.session_generation(), current);
        }
        if respawn.consume_nonspatial_phase(
            clock.session_generation(),
            &control,
            stream.local_player_runtime_id(),
            &mut movement,
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
        let world = sim::PaletteWorld::new(
            stream.collision_store(),
            collisions.registry(stream.network_id_mode()),
            stream.current_dimension(),
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
                        hurt.register(crate::camera::LocalHurtEvent {
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
                    phase3_evidence.note_correction(outcome, position_distance(previous, position));
                }
                #[cfg(feature = "acceptance")]
                ControlObservation::Dimension => {
                    phase3_evidence.note_event(Phase3EvidenceEventKind::Dimension);
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
            let mut stdout = std::io::stdout().lock();
            write_stdout_marker(&mut stdout, &marker);
        }
    }
}

#[allow(clippy::too_many_arguments)]
pub(crate) fn drive_world_stream(
    network: Res<NetworkHandle>,
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
    mut rendered_session: Local<Option<u64>>,
    mut visibility: ResMut<CaveVisibilityCache>,
    camera_publication: Option<
        Res<client_presentation::local_player_camera_receipt::CameraPublicationAttempt>,
    >,
    profiler: Option<Res<RuntimeStageProfiler>>,
) {
    let _timer = profiler
        .as_deref()
        .map(|profiler| profiler.time(RuntimeStage::WorldStream));
    let AppWorldState {
        mut client_world,
        mut local_physics,
        mut movement,
        mut ui_runtime,
        clock,
        ..
    } = state;
    let active_session = client_world
        .stream
        .as_ref()
        .map(|stream| stream.authority().actor_session_id());
    if *rendered_session != active_session {
        render_queue.reset_session();
        acknowledgements.clear();
        *visibility = CaveVisibilityCache::default();
        diagnostic_quads.0.clear();
        *rendered_session = active_session;
    }
    let Some(stream) = client_world.stream.as_mut() else {
        ui_runtime.clear_disconnected_block_cracks();
        return;
    };
    local_retention::retain_completed_player_terrain(
        stream,
        &local_physics,
        camera_publication.as_deref(),
        clock.session_generation(),
    );
    synchronize_biome_tints(stream, &mut biome_tints);
    #[cfg(feature = "acceptance")]
    let mutation_cohort = frame_poll.cohort;
    for acknowledgement in acknowledgements.drain() {
        render_queue.record_gpu_upload_bytes(acknowledgement.uploaded_bytes);
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
        flush_sub_chunk_requests(
            stream,
            OUTBOUND_SEND_BUDGET_PER_FRAME,
            |chunk, base_sub_chunk_y, count, packet| {
                network.send_sub_chunk_request(chunk, base_sub_chunk_y, count, packet)
            },
        )
        .err()
    });
    let mut published_items = 0_usize;
    let mut published_payload_items = 0_usize;
    let mut published_bytes = 0_u64;
    if let Some(stream) = client_world.stream.as_mut() {
        while let Some(change) = stream.pop_mesh_change() {
            if !mesh_change_has_publication_permit(&change) {
                let restored = stream.retry_mesh_change_front(change).is_ok();
                record_fatal_error(
                    &mut client_world.fatal_error,
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
                    let diagnostic_geometry = mesh.diagnostic_geometry().clone();
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
                            diagnostic_quads.0.upsert(key, diagnostic_geometry);
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
                client_world.fatal_error = Some(
                    "failed to restore a render update to the bounded world retry FIFO".to_owned(),
                );
            }
            break;
        }
    }
    if let Some(stream) = client_world.stream.as_ref() {
        let stats = stream.stats();
        let previous = publication.diagnostics().last_work;
        let allowance = publication.allowance();
        publication.finish_frame(PublicationFrameWork {
            mesh_jobs_dispatched: poll_report.mesh_jobs_dispatched,
            mesh_changes_published: published_items,
            mesh_payloads_published: published_payload_items,
            mesh_bytes_published: published_bytes,
            pending_mesh_jobs: stats.pending_mesh_jobs,
            in_flight_mesh_jobs: stats.in_flight_mesh_jobs,
            upload_queue_items: render_queue.retained_len(),
            upload_queue_bytes: render_queue.pending_bytes(),
            allowance_live_permits: allowance.live_permits(),
            allowance_live_payload_bytes: allowance.live_payload_bytes(),
            stream_pending_mesh_changes: stream.pending_mesh_change_count(),
            ..previous
        });
    }
    if let Some(error) = send_error {
        record_fatal_error(&mut client_world.fatal_error, error);
    }
    if let Some(position) = resolved_surface_spawn {
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
