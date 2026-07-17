use std::{
    sync::{
        Arc,
        atomic::{AtomicU8, Ordering},
    },
    thread,
    time::Duration,
};

use assets::RuntimeAssets;
use bevy::{
    app::AppExit,
    ecs::system::SystemParam,
    log::{debug, info},
    prelude::{MessageReader, Query, Res, ResMut, Resource, Time, Transform, Vec3, With},
    time::Real,
};
use client_world::{CommittedControlEvent, CommittedUiEvent, WorldMeshChange, WorldStream};
use meshing::CameraMedium;
use render::{
    ChunkBiomeTints, ChunkRenderQueue, ChunkUploadAcknowledgements, ChunkUploadBudget,
    ChunkUploadPriority, ChunkUploadToken,
};

use crate::{
    acceptance::{
        AcceptanceRun,
        markers::{
            CAMERA_COMMITTED, SHUTDOWN_WATCHDOG_ARMED_MARKER, SHUTDOWN_WATCHDOG_FIRED_MARKER,
        },
        model_witness::ModelWitnessFileSource,
        mutation::{deterministic_mutation_coordinate, write_stdout_marker},
    },
    camera::FlyCamera,
    environment::{self, WeatherState, WorldClock, apply_environment_control},
    movement::{LocalPhysicsController, MovementTicker},
    runtime::{
        network::{NetworkHandle, OUTBOUND_SEND_BUDGET_PER_FRAME},
        publication::{PublicationController, PublicationFrameWork},
        shutdown::record_fatal_error,
        telemetry::bedrock_camera_rotation,
        visibility::{AppMetrics, DiagnosticQuads},
    },
    ui_runtime::{SequencedBlockCrackEvent, SequencedUiEvent, UiRuntime},
};

pub(crate) const SHUTDOWN_WATCHDOG_TIMEOUT: Duration = Duration::from_secs(2);

#[derive(Resource)]
pub(crate) struct ClientWorld {
    pub(crate) stream: Option<WorldStream>,
    pub(crate) runtime_assets: Arc<RuntimeAssets>,
    pub(crate) pending_surface_spawn: Option<[i32; 2]>,
    pub(crate) fatal_error: Option<String>,
    pub(crate) network_decode_errors: u64,
    pub(crate) reported_decode_errors: u64,
}

pub(crate) const SHUTDOWN_WATCHDOG_IDLE: u8 = 0;
pub(crate) const SHUTDOWN_WATCHDOG_ARMED: u8 = 1;
pub(crate) const SHUTDOWN_WATCHDOG_COMPLETED: u8 = 2;
pub(crate) const SHUTDOWN_WATCHDOG_FIRED: u8 = 3;

pub(crate) type ShutdownTerminator = Arc<dyn Fn(i32) + Send + Sync + 'static>;

#[derive(Resource, Clone)]
pub(crate) struct ShutdownWatchdog {
    pub(crate) state: Arc<AtomicU8>,
    pub(crate) timeout: Duration,
    pub(crate) terminate: ShutdownTerminator,
}

impl ShutdownWatchdog {
    pub(crate) fn process(timeout: Duration) -> Self {
        Self::new(timeout, |code| std::process::exit(code))
    }

    pub(crate) fn new<F>(timeout: Duration, terminate: F) -> Self
    where
        F: Fn(i32) + Send + Sync + 'static,
    {
        Self {
            state: Arc::new(AtomicU8::new(SHUTDOWN_WATCHDOG_IDLE)),
            timeout,
            terminate: Arc::new(terminate),
        }
    }

    pub(crate) fn arm(&self, exit: AppExit) -> bool {
        if self
            .state
            .compare_exchange(
                SHUTDOWN_WATCHDOG_IDLE,
                SHUTDOWN_WATCHDOG_ARMED,
                Ordering::AcqRel,
                Ordering::Acquire,
            )
            .is_err()
        {
            return false;
        }
        let state = Arc::clone(&self.state);
        let terminate = Arc::clone(&self.terminate);
        let timeout = self.timeout;
        let exit_code = app_exit_code(&exit);
        let spawned = thread::Builder::new()
            .name("bedrock-shutdown-watchdog".to_owned())
            .spawn(move || {
                thread::sleep(timeout);
                if state
                    .compare_exchange(
                        SHUTDOWN_WATCHDOG_ARMED,
                        SHUTDOWN_WATCHDOG_FIRED,
                        Ordering::AcqRel,
                        Ordering::Acquire,
                    )
                    .is_ok()
                {
                    eprintln!(
                        "{SHUTDOWN_WATCHDOG_FIRED_MARKER} timeout_ms={} exit_code={exit_code}",
                        timeout.as_millis()
                    );
                    terminate(exit_code);
                }
            });
        if spawned.is_err() {
            self.state.store(SHUTDOWN_WATCHDOG_FIRED, Ordering::Release);
            (self.terminate)(exit_code);
        }
        true
    }

    pub(crate) fn complete(&self) {
        self.state
            .store(SHUTDOWN_WATCHDOG_COMPLETED, Ordering::Release);
    }
}

pub(crate) fn app_exit_code(exit: &AppExit) -> i32 {
    match exit {
        AppExit::Success => 0,
        AppExit::Error(code) => i32::from(code.get()),
    }
}

pub(crate) fn begin_bounded_shutdown(watchdog: &ShutdownWatchdog, exit: &AppExit) {
    if watchdog.arm(exit.clone()) {
        eprintln!(
            "{SHUTDOWN_WATCHDOG_ARMED_MARKER} timeout_ms={} exit_code={}",
            watchdog.timeout.as_millis(),
            app_exit_code(exit)
        );
    }
}

pub(crate) fn arm_shutdown_watchdog(
    mut exits: MessageReader<AppExit>,
    watchdog: Res<ShutdownWatchdog>,
) {
    let requested = exits.read().cloned().reduce(
        |selected, next| {
            if selected.is_error() { selected } else { next }
        },
    );
    if let Some(exit) = requested {
        begin_bounded_shutdown(&watchdog, &exit);
    }
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
            pending_surface_spawn: None,
            fatal_error: None,
            network_decode_errors: 0,
            reported_decode_errors: 0,
        }
    }
}

#[derive(SystemParam)]
pub(crate) struct AppWorldState<'w> {
    pub(crate) client_world: ResMut<'w, ClientWorld>,
    pub(crate) clock: ResMut<'w, WorldClock>,
    pub(crate) weather: ResMut<'w, WeatherState>,
    pub(crate) movement: ResMut<'w, MovementTicker>,
    pub(crate) local_physics: ResMut<'w, LocalPhysicsController>,
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
) {
    let Some((stream, camera)) = client_world.stream.as_ref().zip(camera.single().ok()) else {
        medium.0 = CameraMedium::Air;
        *context = environment::EnvironmentContext::default();
        return;
    };
    let position = camera.translation.to_array();
    medium.0 = stream.camera_medium(position);
    let camera_biome_identifier = stream
        .camera_biome_id(camera.translation.to_array())
        .and_then(|raw_id| {
            client_world
                .runtime_assets
                .biome_assets()
                .rules
                .binary_search_by_key(&raw_id, |rule| rule.id)
                .ok()
                .map(|index| {
                    client_world.runtime_assets.biome_assets().rules[index]
                        .name
                        .clone()
                })
        });
    *context = environment::EnvironmentContext {
        dimension: stream.current_dimension(),
        camera_biome_identifier,
        render_distance_blocks: Some(stream.render_distance_blocks()),
    };
}

pub(crate) fn world_stream_fatal_message(error: client_world::WorldStreamFatalError) -> String {
    format!("world stream fatal: {error}")
}

#[allow(clippy::too_many_arguments)]
pub(crate) fn drive_world_stream(
    network: Res<NetworkHandle>,
    state: AppWorldState,
    mut acceptance: ResMut<AcceptanceRun>,
    mut metrics: ResMut<AppMetrics>,
    mut render_queue: ResMut<ChunkRenderQueue>,
    mut biome_tints: ResMut<ChunkBiomeTints>,
    mut diagnostic_quads: ResMut<DiagnosticQuads>,
    acknowledgements: Res<ChunkUploadAcknowledgements>,
    upload_budget: Res<ChunkUploadBudget>,
    mut publication: ResMut<PublicationController>,
    model_witness_source: Res<ModelWitnessFileSource>,
    mut camera: Query<&mut Transform, With<FlyCamera>>,
) {
    let AppWorldState {
        mut client_world,
        mut clock,
        mut weather,
        mut movement,
        mut local_physics,
        mut ui_runtime,
        time,
    } = state;
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
    let (controls, committed_ui, stream_fatal, poll_report) = {
        let stream = client_world
            .stream
            .as_mut()
            .expect("stream presence was checked before camera access");
        let report = stream.poll(camera.translation.to_array(), upload_budget.max_per_frame);
        (
            stream.take_committed_controls(),
            stream.take_committed_ui(),
            stream.take_fatal_error(),
            report,
        )
    };
    if let Some(error) = stream_fatal {
        record_fatal_error(
            &mut client_world.fatal_error,
            world_stream_fatal_message(error),
        );
        return;
    }
    let local_millis = u64::try_from(time.elapsed().as_millis()).unwrap_or(u64::MAX);
    for committed in committed_ui {
        let result = match committed {
            CommittedUiEvent::Ui { sequence, event } => ui_runtime
                .apply(SequencedUiEvent {
                    session_id: clock.session_generation(),
                    fifo_sequence: sequence,
                    local_millis,
                    server_tick: None,
                    event,
                })
                .map(|_| ()),
            CommittedUiEvent::BlockCrack {
                sequence,
                dimension,
                event,
            } => ui_runtime.retain_block_crack(SequencedBlockCrackEvent {
                session_id: clock.session_generation(),
                fifo_sequence: sequence,
                dimension,
                event,
            }),
        };
        if let Err(error) = result {
            record_fatal_error(
                &mut client_world.fatal_error,
                format!("committed UI/gameplay event rejected: {error:?}"),
            );
            return;
        }
    }
    if let Some(stream) = client_world.stream.as_ref()
        && let Some(local_actor) = stream.actor(stream.local_player_runtime_id())
    {
        ui_runtime.sync_local_attributes(&local_actor.attributes);
    }
    for control in controls {
        if apply_environment_control(control, &mut clock, &mut weather, time.elapsed_secs_f64()) {
            continue;
        }
        match &control {
            CommittedControlEvent::PlayerMovementCorrection {
                correction,
                resolved,
                ..
            } => {
                movement.apply_server_correction(correction.tick, resolved.position);
                local_physics.reanchor_network_position(
                    resolved.position,
                    correction.tick,
                    correction.on_ground,
                );
            }
            CommittedControlEvent::MovePlayer {
                movement: correction,
                resolved,
                ..
            } => {
                movement.reanchor_position(resolved.position);
                local_physics.reanchor_network_position(
                    resolved.position,
                    u64::try_from(correction.source_tick).unwrap_or(0),
                    correction.on_ground,
                );
            }
            CommittedControlEvent::ChangeDimension { resolved, .. } => {
                movement.reanchor_position(resolved.position);
                local_physics.reanchor_network_position(resolved.position, 0, false);
            }
            CommittedControlEvent::SetTime { .. }
            | CommittedControlEvent::DaylightCycle { .. }
            | CommittedControlEvent::Weather { .. } => {}
        }
        let _ = acceptance.observe_committed_full_view_control(&control);
        let camera_marker =
            model_gallery_camera_committed_marker(model_witness_source.configured(), &control);
        apply_committed_control(
            control,
            &mut camera,
            &mut client_world.pending_surface_spawn,
        );
        if let Some(marker) = camera_marker {
            let mut stdout = std::io::stdout().lock();
            write_stdout_marker(&mut stdout, &marker);
        }
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
    let mut published_items = 0;
    let mut published_bytes = 0_u64;
    if let Some(stream) = client_world.stream.as_mut() {
        while let Some(change) = stream.pop_mesh_change() {
            let change_bytes = match &change {
                WorldMeshChange::Upsert { mesh, biome, .. } => {
                    ChunkRenderQueue::upload_byte_len(mesh, biome)
                }
                WorldMeshChange::Remove { .. } => 0,
            };
            if !upload_budget.can_fit(published_items, published_bytes, 1, change_bytes) {
                if stream.retry_mesh_change_front(change).is_err() {
                    client_world.fatal_error = Some(
                        "failed to restore a budget-deferred render update to the bounded world retry FIFO"
                            .to_owned(),
                    );
                }
                break;
            }
            let retry = match change {
                WorldMeshChange::Upsert {
                    key,
                    mesh,
                    biome,
                    tint_identity,
                    generation,
                    dirty_since,
                } => {
                    let diagnostic_geometry = mesh.diagnostic_geometry().clone();
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
                            diagnostic_quads.0.upsert(key, diagnostic_geometry);
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
                published_items = published_items.saturating_add(1);
                published_bytes = published_bytes.saturating_add(change_bytes);
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
        publication.finish_frame(PublicationFrameWork {
            mesh_jobs_dispatched: poll_report.mesh_jobs_dispatched,
            mesh_changes_published: published_items,
            mesh_bytes_published: published_bytes,
            pending_mesh_jobs: stats.pending_mesh_jobs,
            in_flight_mesh_jobs: stats.in_flight_mesh_jobs,
            upload_queue_items: render_queue.retained_len(),
            upload_queue_bytes: render_queue.pending_bytes(),
            ..previous
        });
    }
    if let Some(error) = send_error {
        record_fatal_error(&mut client_world.fatal_error, error);
    }
    if let Some(position) = resolved_surface_spawn {
        camera.translation = Vec3::from_array(position);
        let tick = local_physics.state().map_or(0, |state| state.tick);
        local_physics.reanchor_network_position(position, tick, true);
        client_world.pending_surface_spawn = None;
        info!(position = ?position, "resolved temporary Bedrock spawn from packed terrain");
    }
    if let Some(coordinate) = resolved_mutation_coordinate {
        acceptance.set_mutation_coordinate(coordinate);
    }
}

pub(crate) fn flush_sub_chunk_requests(
    stream: &mut WorldStream,
    budget: usize,
    mut send: impl FnMut(
        world::ChunkKey,
        i32,
        usize,
        protocol::Packet,
    ) -> Result<(), crate::runtime::network::session::PacketSendError>,
) -> Result<usize, String> {
    let mut sent = 0;
    for _ in 0..budget {
        let Some(request) = stream.pop_next_request() else {
            break;
        };
        let client_world::PendingSubChunkRequest {
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
                let retry = client_world::PendingSubChunkRequest {
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

pub(crate) fn apply_committed_control(
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
        CommittedControlEvent::PlayerMovementCorrection {
            correction,
            resolved,
            ..
        } => {
            info!(
                tick = correction.tick,
                position = ?correction.position,
                "applying committed server-authoritative movement correction"
            );
            if correction.yaw.is_finite() && correction.pitch.is_finite() {
                camera.rotation = bedrock_camera_rotation(correction.yaw, correction.pitch);
            }
            resolved
        }
        CommittedControlEvent::ChangeDimension { resolved, .. } => resolved,
        CommittedControlEvent::SetTime { .. }
        | CommittedControlEvent::DaylightCycle { .. }
        | CommittedControlEvent::Weather { .. } => return,
    };
    camera.translation = Vec3::from_array(resolved.position);
    *pending_surface_spawn = resolved.surface_anchor;
}

pub(crate) fn model_gallery_camera_committed_marker(
    configured: bool,
    control: &CommittedControlEvent,
) -> Option<String> {
    if !configured {
        return None;
    }
    let CommittedControlEvent::MovePlayer {
        sequence,
        movement,
        resolved,
        ..
    } = control
    else {
        return None;
    };
    let [x, y, z] = resolved.position;
    Some(format!(
        "{CAMERA_COMMITTED} sequence={sequence} position={x:.5},{y:.5},{z:.5} yaw={:.5} pitch={:.5}",
        movement.yaw, movement.pitch
    ))
}
