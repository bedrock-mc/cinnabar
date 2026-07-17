use std::{
    sync::Arc,
    time::{Duration, Instant},
};

use bevy::{
    camera::Projection,
    log::{debug, error, info},
    prelude::{Local, Query, Res, ResMut, Time, Transform, Vec3, With},
    time::Real,
};
use client_world::{ActorSnapshot, PlayerProfile, SAFE_SERVER_HEIGHT, WorldStream};
use render::{
    ActorCullView, ActorRenderFrame, ActorRenderScene, ActorRenderSource, ActorSkinPixels,
    ChunkUploadAcknowledgements, MAX_ACTOR_RENDER_DISTANCE_BLOCKS,
};

use crate::{
    acceptance::{
        AcceptanceRun,
        model_witness::ModelWitnessFileSource,
        mutation::{
            accepted_move_player_ingress_marker, move_player_ingress_marker,
            write_move_player_ingress_before_source_capture, write_stdout_marker,
        },
    },
    camera::FlyCamera,
    environment::replace_session,
    movement::MovementSource,
    runtime::{
        shutdown::record_fatal_error,
        visibility::AppMetrics,
        world::{AppWorldState, ClientWorld},
    },
};

pub(crate) use session::{
    NetworkConfig, NetworkControlEvent, NetworkHandle, WORLD_EVENT_CAPACITY, spawn_network,
};

pub(crate) const NETWORK_INGRESS_BUDGET_PER_FRAME: usize = 32;
pub(crate) const OUTBOUND_SEND_BUDGET_PER_FRAME: usize = 16;
const ACTOR_TICK_NANOS: u128 = 50_000_000;
const _: () = assert!(WORLD_EVENT_CAPACITY >= NETWORK_INGRESS_BUDGET_PER_FRAME);
const _: () = assert!(NETWORK_INGRESS_BUDGET_PER_FRAME == client_world::MAX_ADMITTED_HEAVY_EVENTS);

#[derive(Debug, Clone, Copy, PartialEq)]
pub(crate) struct ActorFrameStep {
    pub(crate) ticks: u32,
    pub(crate) partial_tick: f32,
}

#[derive(Debug, Default)]
pub(crate) struct ActorFrameClock {
    accumulated_nanos: u128,
}

impl ActorFrameClock {
    pub(crate) fn advance(&mut self, delta: Duration) -> ActorFrameStep {
        self.accumulated_nanos = self.accumulated_nanos.saturating_add(delta.as_nanos());
        let elapsed_ticks = self.accumulated_nanos / ACTOR_TICK_NANOS;
        self.accumulated_nanos %= ACTOR_TICK_NANOS;
        ActorFrameStep {
            ticks: u32::try_from(elapsed_ticks).unwrap_or(u32::MAX),
            partial_tick: self.accumulated_nanos as f32 / ACTOR_TICK_NANOS as f32,
        }
    }

    pub(crate) fn reset(&mut self) {
        self.accumulated_nanos = 0;
    }
}

pub(crate) fn receive_network_events(
    mut network: ResMut<NetworkHandle>,
    state: AppWorldState,
    mut acceptance: ResMut<AcceptanceRun>,
    metrics: Res<AppMetrics>,
    acknowledgements: Res<ChunkUploadAcknowledgements>,
    model_witness_source: Res<ModelWitnessFileSource>,
    mut cameras: Query<&mut Transform, With<FlyCamera>>,
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
    let controls =
        drain_network_controls(network.control_events_mut(), OUTBOUND_SEND_BUDGET_PER_FRAME);
    for control in controls {
        match control {
            NetworkControlEvent::Bootstrap {
                world: bootstrap,
                environment,
            } => {
                acknowledgements.clear();
                info!(
                    runtime_id = bootstrap.local_player_runtime_id,
                    position = ?bootstrap.player_position,
                    world_spawn = ?bootstrap.world_spawn_position,
                    "received StartGame bootstrap"
                );
                let replacing_session = clock.session_generation() != 0;
                replace_session(
                    &mut clock,
                    &mut weather,
                    environment,
                    time.elapsed_secs_f64(),
                );
                ui_runtime.begin_session(clock.session_generation());
                if replacing_session {
                    debug!("replaced StartGame environment session");
                }
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
                // StartGame initializes movement timing, but the current app
                // still has only an independent fly camera. A future physics
                // system must explicitly replace this non-authoritative source.
                movement.set_source(MovementSource::FreeCamera);
                movement.reset(
                    clock.session_generation(),
                    u64::try_from(environment.initial_time).unwrap_or(0),
                    resolved.position,
                );
                local_physics.reanchor_network_position(resolved.position, 0, false);
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
                movement.deactivate();
                local_physics.deactivate();
                error!(decode_error_count, "network session failed: {message}");
                client_world.network_decode_errors = decode_error_count;
                record_fatal_error(
                    &mut client_world.fatal_error,
                    format!("network session failed: {message}"),
                );
            }
            NetworkControlEvent::Stopped { decode_error_count } => {
                movement.deactivate();
                local_physics.deactivate();
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
        if model_witness_source.configured()
            && let protocol::WorldEvent::MovePlayer(movement) = &sequenced.event
            && let Some(marker) = move_player_ingress_marker(sequenced.sequence, movement.position)
        {
            let mut stdout = std::io::stdout().lock();
            write_stdout_marker(&mut stdout, &marker);
        }
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

pub(crate) fn drain_network_controls<T>(
    receiver: &mut tokio::sync::mpsc::Receiver<T>,
    budget: usize,
) -> Vec<T> {
    drain_network_ingress(receiver, budget)
}

pub(crate) fn drain_network_ingress<T>(
    receiver: &mut tokio::sync::mpsc::Receiver<T>,
    budget: usize,
) -> Vec<T> {
    std::iter::from_fn(|| receiver.try_recv().ok())
        .take(budget)
        .collect()
}

pub(crate) fn actor_render_source(
    actor: &ActorSnapshot,
    profile: Option<&PlayerProfile>,
) -> ActorRenderSource {
    let skin = profile.and_then(|profile| match &profile.skin {
        protocol::PlayerSkin::Standard(skin) => Some(ActorSkinPixels {
            width: skin.width,
            height: skin.height,
            rgba8: Arc::clone(&skin.rgba8),
        }),
        protocol::PlayerSkin::Unavailable(_) => None,
    });
    ActorRenderSource {
        runtime_id: actor.runtime_id,
        unique_id: actor.unique_id,
        spawn_revision: actor.spawn_revision,
        movement_revision: actor.movement_revision,
        previous_position: actor.previous_pose.position,
        previous_pitch_degrees: actor.previous_pose.pitch,
        previous_yaw_degrees: actor.previous_pose.yaw,
        previous_head_yaw_degrees: actor.previous_pose.head_yaw,
        position: actor.position,
        pitch_degrees: actor.pitch,
        yaw_degrees: actor.yaw,
        head_yaw_degrees: actor.head_yaw,
        teleported: actor.teleported,
        skin,
    }
}

pub(crate) fn publish_actor_render_frame(
    mut client_world: ResMut<ClientWorld>,
    time: Res<Time<Real>>,
    mut scene: ResMut<ActorRenderScene>,
    mut frame: ResMut<ActorRenderFrame>,
    mut published_session: Local<Option<u64>>,
    mut actor_clock: Local<ActorFrameClock>,
    camera: Query<(&Transform, &Projection), With<FlyCamera>>,
) {
    let session_id = client_world
        .stream
        .as_ref()
        .map(WorldStream::actor_session_id);
    if *published_session != session_id {
        scene.reset();
        actor_clock.reset();
        *published_session = session_id;
    }
    let step = actor_clock.advance(time.delta());
    if let Some(stream) = client_world.stream.as_mut() {
        stream.advance_actor_interpolation_ticks(step.ticks);
    }
    let sources = client_world
        .stream
        .as_ref()
        .map(|stream| {
            stream
                .render_players()
                .into_iter()
                .map(|(actor, profile)| actor_render_source(actor, profile))
                .collect::<Vec<_>>()
        })
        .unwrap_or_default();
    let cull_view = camera
        .single()
        .ok()
        .map(|(transform, projection)| ActorCullView {
            clip_from_world: projection.get_clip_from_view() * transform.to_matrix().inverse(),
            camera_position: transform.translation,
            max_distance: MAX_ACTOR_RENDER_DISTANCE_BLOCKS,
        });
    *frame = scene.update(step.partial_tick, cull_view, sources).clone();
}

pub(crate) mod session;
