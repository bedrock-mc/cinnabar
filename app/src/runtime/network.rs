use std::{
    sync::Arc,
    time::{Duration, Instant},
};

use bevy::{
    camera::Projection,
    ecs::system::SystemParam,
    log::{debug, error, info, warn},
    prelude::{Local, Query, Res, ResMut, Time, Transform, With},
    time::Real,
};
#[cfg(test)]
use client_world::{ActorSnapshot, PlayerProfile};
use client_world::{SAFE_SERVER_HEIGHT, WorldStream};
use protocol::WorldEvent;
use render::{
    ActorCullView, ActorMainWitness, ActorRenderFrame, ActorRenderScene, ActorRuntimeWitness,
    ChunkUploadAcknowledgements, MAX_ACTOR_RENDER_DISTANCE_BLOCKS,
};
#[cfg(test)]
use render::{ActorRenderSource, ActorSkinPixels};

use crate::{
    acceptance::{
        AcceptanceRun,
        model_witness::ModelWitnessFileSource,
        mutation::{
            accepted_move_player_ingress_marker, move_player_ingress_marker,
            write_move_player_ingress_before_source_capture, write_stdout_marker,
        },
    },
    camera::{AutoFly, CameraSettingsAuthority, FlyCamera},
    environment::replace_session,
    local_player::{
        InteractionOriginSnapshot, LocalAvatarPresentation, LocalAvatarVisibilityCarrier,
        LocalPlayerFrameCarrier, LocalPlayerFrameReset, LocalViewPose, reset_local_player_session,
    },
    movement::{MovementSource, PhysicsAuthorityGate},
    presentation::actors::{
        actor_rig_presentation, local_actor_presentation_for_visibility,
        local_diagnostic_presentation, select_actor_presentations_for_view, update_actor_rig_scene,
    },
    runtime::{
        phase3_evidence::{Phase3EvidenceEmitter, Phase3EvidenceEventKind},
        publication::PublicationController,
        shutdown::record_fatal_error,
        visibility::AppMetrics,
        world::{AppWorldState, ClientWorld},
    },
    ui_runtime::{
        UiRuntime, UiRuntimeError,
        inventory_router::{EquipmentRoute, EquipmentRouteResult, InventoryRouterError},
    },
};

#[cfg(test)]
use crate::local_player::FrozenLocalAvatarVisibility;

pub(crate) use session::{
    NetworkConfig, NetworkControlEvent, NetworkHandle, PacketSendError, WORLD_EVENT_CAPACITY,
    spawn_network,
};

pub(crate) const NETWORK_INGRESS_BUDGET_PER_FRAME: usize = 32;
pub(crate) const OUTBOUND_SEND_BUDGET_PER_FRAME: usize = 16;
const ACTOR_TICK_NANOS: u128 = 50_000_000;
const _: () = assert!(WORLD_EVENT_CAPACITY >= NETWORK_INGRESS_BUDGET_PER_FRAME);
const _: () = assert!(NETWORK_INGRESS_BUDGET_PER_FRAME == client_world::MAX_ADMITTED_HEAVY_EVENTS);

#[derive(SystemParam)]
pub(crate) struct NetworkLocalPlayerState<'w> {
    view: ResMut<'w, LocalViewPose>,
    avatar: ResMut<'w, LocalAvatarPresentation>,
    settings: ResMut<'w, CameraSettingsAuthority>,
    frame: ResMut<'w, LocalPlayerFrameCarrier>,
    interaction: ResMut<'w, InteractionOriginSnapshot>,
    evidence: ResMut<'w, Phase3EvidenceEmitter>,
    authority: Res<'w, PhysicsAuthorityGate>,
    auto_fly: Res<'w, AutoFly>,
}

#[derive(SystemParam)]
pub(crate) struct ActorPresentationState<'w, 's> {
    avatar: Res<'w, LocalAvatarPresentation>,
    local_frame: Res<'w, LocalPlayerFrameCarrier>,
    local_visibility: ResMut<'w, LocalAvatarVisibilityCarrier>,
    settings: Res<'w, CameraSettingsAuthority>,
    view: Res<'w, LocalViewPose>,
    witness: Res<'w, ActorRuntimeWitness>,
    camera: Query<'w, 's, (&'static Transform, &'static Projection), With<FlyCamera>>,
}

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

#[derive(Debug, Clone, PartialEq)]
pub(crate) enum EquipmentIngress {
    Buffered,
    CommitOnly { fifo_sequence: u64 },
    ActorPresentation(Box<session::SequencedWorldEvent>),
}

pub(crate) fn publish_equipment_identity(
    runtime: &mut UiRuntime,
    session_id: u64,
    runtime_id: u64,
) -> Result<Vec<EquipmentIngress>, InventoryRouterError> {
    let routes = runtime
        .publish_local_runtime_id(session_id, runtime_id)?
        .into_iter()
        .map(|route| consume_equipment_route(runtime, session_id, route))
        .collect();
    Ok(routes)
}

pub(crate) fn route_equipment_ingress(
    runtime: &mut UiRuntime,
    sequenced: session::SequencedWorldEvent,
) -> Result<EquipmentIngress, InventoryRouterError> {
    let session_id = sequenced.session_generation;
    let WorldEvent::Equipment(event) = sequenced.event else {
        unreachable!("equipment routing accepts only equipment world events")
    };
    match runtime.route_equipment(session_id, sequenced.sequence, event)? {
        EquipmentRouteResult::Buffered => Ok(EquipmentIngress::Buffered),
        EquipmentRouteResult::Routed(route) => {
            Ok(consume_equipment_route(runtime, session_id, route))
        }
    }
}

pub(crate) fn route_inventory_ingress(
    runtime: &mut UiRuntime,
    sequenced: session::SequencedWorldEvent,
) -> Result<u64, UiRuntimeError> {
    let session::SequencedWorldEvent {
        session_generation,
        sequence,
        event: WorldEvent::Inventory(event),
    } = sequenced
    else {
        unreachable!("inventory routing accepts only inventory world events")
    };
    runtime.enqueue_inventory_event(session_generation, sequence, event)?;
    Ok(sequence)
}

pub(crate) const fn bootstrap_session_generation_is_expected(
    ui_session_generation: u64,
    world_session_generation: u64,
    incoming_session_generation: u64,
) -> bool {
    ui_session_generation == world_session_generation
        && matches!(
            world_session_generation.checked_add(1),
            Some(expected) if expected == incoming_session_generation
        )
}

fn consume_equipment_route(
    runtime: &mut UiRuntime,
    session_generation: u64,
    route: EquipmentRoute,
) -> EquipmentIngress {
    match route {
        EquipmentRoute::LocalSelected {
            fifo_sequence,
            event,
        } => {
            runtime.retain_local_selected_equipment(fifo_sequence, event);
            EquipmentIngress::CommitOnly { fifo_sequence }
        }
        EquipmentRoute::ActorPresentation {
            fifo_sequence,
            event,
        } => EquipmentIngress::ActorPresentation(Box::new(session::SequencedWorldEvent {
            session_generation,
            sequence: fifo_sequence,
            event: WorldEvent::Equipment(event),
        })),
    }
}

// These resources have distinct Bevy access modes and lifetimes; keeping them
// explicit lets the scheduler validate conflicts while bootstrap wires the
// shared publication allowance into each newly created world stream.
#[allow(clippy::too_many_arguments)]
pub(crate) fn receive_network_events(
    mut network: ResMut<NetworkHandle>,
    state: AppWorldState,
    mut acceptance: ResMut<AcceptanceRun>,
    metrics: Res<AppMetrics>,
    acknowledgements: Res<ChunkUploadAcknowledgements>,
    model_witness_source: Res<ModelWitnessFileSource>,
    publication: Res<PublicationController>,
    local_player: NetworkLocalPlayerState,
) {
    let NetworkLocalPlayerState {
        mut view,
        mut avatar,
        mut settings,
        mut frame,
        mut interaction,
        mut evidence,
        authority: physics_authority,
        auto_fly,
    } = local_player;
    let AppWorldState {
        mut client_world,
        mut clock,
        mut weather,
        mut movement,
        mut local_physics,
        collisions,
        mut ui_runtime,
        time,
    } = state;
    let controls =
        drain_network_controls(network.control_events_mut(), OUTBOUND_SEND_BUDGET_PER_FRAME);
    for control in controls {
        match control {
            NetworkControlEvent::Bootstrap {
                session_generation,
                world: bootstrap,
                environment,
                inventory,
            } => {
                if !bootstrap_session_generation_is_expected(
                    ui_runtime.session_id(),
                    clock.session_generation(),
                    session_generation,
                ) {
                    record_fatal_error(
                        &mut client_world.fatal_error,
                        format!(
                            "unexpected StartGame session generation: UI {}, world {}, incoming {session_generation}",
                            ui_runtime.session_id(),
                            clock.session_generation()
                        ),
                    );
                    continue;
                }
                acknowledgements.clear();
                frame.reset(LocalPlayerFrameReset::Session);
                interaction.invalidate();
                evidence.note_event(Phase3EvidenceEventKind::Session);
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
                ui_runtime.begin_session(session_generation);
                let protocol::InventoryEvent::Authority(authority) = inventory else {
                    record_fatal_error(
                        &mut client_world.fatal_error,
                        "StartGame inventory fanout was not an authority event".to_owned(),
                    );
                    continue;
                };
                ui_runtime.publish_inventory_authority(authority);
                if replacing_session {
                    debug!("replaced StartGame environment session");
                }
                let current = if view.eye_translation().is_finite() {
                    view.eye_translation().to_array()
                } else {
                    [
                        bootstrap.world_spawn_position[0] as f32 + 0.5,
                        SAFE_SERVER_HEIGHT,
                        bootstrap.world_spawn_position[2] as f32 + 0.5,
                    ]
                };
                let mut stream = if let Some(entity_assets) = client_world.entity_assets.as_ref() {
                    WorldStream::new_with_asset_sets(
                        bootstrap,
                        Arc::clone(&client_world.runtime_assets),
                        Arc::clone(entity_assets),
                        current,
                        client_world.pending_surface_spawn,
                    )
                } else {
                    WorldStream::new_with_assets(
                        bootstrap,
                        Arc::clone(&client_world.runtime_assets),
                        current,
                        client_world.pending_surface_spawn,
                    )
                };
                stream.set_publication_allowance(publication.allowance());
                let resolved = stream.resolved_server_position();
                if acceptance.enabled() {
                    acceptance
                        .set_mutation_surface_anchor(acceptance_surface_anchor(resolved.position));
                }
                reset_local_player_session(
                    session_generation,
                    bootstrap.local_player_runtime_id,
                    resolved.position,
                    &mut settings,
                    &mut view,
                    &mut avatar,
                );
                movement.set_source(MovementSource::FreeCamera);
                let initial_tick = u64::try_from(environment.initial_time).unwrap_or(0);
                movement.reset(clock.session_generation(), initial_tick, resolved.position);
                local_physics.reanchor_network_position_before_advance(
                    resolved.position,
                    initial_tick,
                    false,
                );
                match physics_authority.authorize(auto_fly.enabled(), collisions.is_complete()) {
                    Ok(source) => movement.set_source(source),
                    Err(fault) => {
                        movement.set_source(MovementSource::FreeCamera);
                        local_physics.deactivate();
                        record_fatal_error(
                            &mut client_world.fatal_error,
                            format!("candidate Physics authority failed closed: {fault:?}"),
                        );
                    }
                }
                client_world.pending_surface_spawn = resolved.surface_anchor;
                client_world.stream = Some(stream);
                let routed = match publish_equipment_identity(
                    &mut ui_runtime,
                    session_generation,
                    bootstrap.local_player_runtime_id,
                ) {
                    Ok(routed) => routed,
                    Err(error) => {
                        record_fatal_error(
                            &mut client_world.fatal_error,
                            format!("inventory identity publication failed: {error:?}"),
                        );
                        continue;
                    }
                };
                for route in routed {
                    let result = match route {
                        EquipmentIngress::ActorPresentation(sequenced) => {
                            let sequenced = *sequenced;
                            client_world
                                .stream
                                .as_mut()
                                .map(|stream| stream.submit(sequenced.sequence, sequenced.event))
                        }
                        EquipmentIngress::CommitOnly { fifo_sequence } => client_world
                            .stream
                            .as_mut()
                            .map(|stream| stream.commit(fifo_sequence)),
                        EquipmentIngress::Buffered => None,
                    };
                    if let Some(Err(error)) = result {
                        record_fatal_error(
                            &mut client_world.fatal_error,
                            format!("world FIFO rejected buffered equipment: {error}"),
                        );
                        break;
                    }
                }
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
            NetworkControlEvent::ChatPacketSent { session, sequence } => {
                if !ui_runtime.acknowledge_chat_send(session, sequence) {
                    warn!(
                        session,
                        sequence, "ignored unrelated chat send acknowledgement"
                    );
                }
            }
            NetworkControlEvent::ChatPacketSendFailed {
                session,
                sequence,
                message,
            } => {
                if ui_runtime.fail_chat_send(session, sequence) {
                    error!(session, sequence, "chat packet send failed: {message}");
                } else {
                    warn!(
                        session,
                        sequence, "ignored unrelated chat send failure: {message}"
                    );
                }
            }
            NetworkControlEvent::BlobCacheTelemetry { enabled, stats } => {
                client_world.client_blob_cache_enabled = enabled;
                client_world.client_blob_cache = stats;
            }
            NetworkControlEvent::Failed {
                message,
                decode_error_count,
            } => {
                movement.deactivate();
                local_physics.deactivate();
                avatar.clear();
                frame.reset(LocalPlayerFrameReset::Session);
                interaction.invalidate();
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
                avatar.clear();
                frame.reset(LocalPlayerFrameReset::Session);
                interaction.invalidate();
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
    let events = drain_world_ingress_until_barrier(
        network.world_events_mut(),
        NETWORK_INGRESS_BUDGET_PER_FRAME.min(admission_capacity),
    );
    for ingress in events {
        let sequenced = match ingress {
            session::WorldIngress::Event(sequenced) => sequenced,
            session::WorldIngress::FastTransferBarrier {
                session_generation,
                sequence,
                action_sequence,
            } => {
                if session_generation != ui_runtime.session_id() {
                    record_fatal_error(
                        &mut client_world.fatal_error,
                        format!(
                            "fast-transfer barrier crossed a session boundary: expected {}, got {session_generation}",
                            ui_runtime.session_id()
                        ),
                    );
                    continue;
                }
                let Some(stream) = client_world.stream.as_mut() else {
                    record_fatal_error(
                        &mut client_world.fatal_error,
                        "received fast-transfer barrier before StartGame bootstrap".to_owned(),
                    );
                    continue;
                };
                if let Err(error) = stream.commit(sequence) {
                    record_fatal_error(
                        &mut client_world.fatal_error,
                        format!("fast-transfer FIFO marker was rejected: {error}"),
                    );
                    continue;
                }
                info!(
                    session_generation,
                    action_sequence, sequence, "committed fast-transfer FIFO marker"
                );
                continue;
            }
        };
        if sequenced.session_generation != ui_runtime.session_id() {
            record_fatal_error(
                &mut client_world.fatal_error,
                format!(
                    "world ingress crossed a session boundary: expected {}, got {}",
                    ui_runtime.session_id(),
                    sequenced.session_generation
                ),
            );
            continue;
        }
        let Some(stream) = client_world.stream.as_mut() else {
            client_world.fatal_error =
                Some("received world data before StartGame bootstrap".to_owned());
            continue;
        };
        let sequenced = if matches!(&sequenced.event, WorldEvent::Equipment(_)) {
            match route_equipment_ingress(&mut ui_runtime, sequenced) {
                Ok(EquipmentIngress::ActorPresentation(sequenced)) => *sequenced,
                Ok(EquipmentIngress::CommitOnly { fifo_sequence }) => {
                    if let Err(error) = stream.commit(fifo_sequence) {
                        record_fatal_error(
                            &mut client_world.fatal_error,
                            format!("world FIFO rejected local equipment commit: {error}"),
                        );
                    }
                    continue;
                }
                Ok(EquipmentIngress::Buffered) => continue,
                Err(error) => {
                    record_fatal_error(
                        &mut client_world.fatal_error,
                        format!("equipment ingress rejected: {error:?}"),
                    );
                    continue;
                }
            }
        } else if matches!(&sequenced.event, WorldEvent::Inventory(_)) {
            let commit_sequence = match route_inventory_ingress(&mut ui_runtime, sequenced) {
                Ok(sequence) => sequence,
                Err(error) => {
                    record_fatal_error(
                        &mut client_world.fatal_error,
                        format!("inventory ingress rejected: {error:?}"),
                    );
                    continue;
                }
            };
            if let Err(error) = stream.commit(commit_sequence) {
                record_fatal_error(
                    &mut client_world.fatal_error,
                    format!("world FIFO rejected inventory commit: {error}"),
                );
            }
            continue;
        } else {
            sequenced
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

pub(crate) fn acceptance_surface_anchor(position: [f32; 3]) -> [i32; 2] {
    [position[0].floor() as i32, position[2].floor() as i32]
}

pub(crate) fn drain_network_controls<T>(
    receiver: &mut tokio::sync::mpsc::Receiver<T>,
    budget: usize,
) -> Vec<T> {
    drain_network_ingress(receiver, budget)
}

pub(crate) fn drain_world_ingress_until_barrier(
    receiver: &mut tokio::sync::mpsc::Receiver<session::WorldIngress>,
    budget: usize,
) -> Vec<session::WorldIngress> {
    let mut drained = Vec::with_capacity(budget);
    for _ in 0..budget {
        let Ok(ingress) = receiver.try_recv() else {
            break;
        };
        let is_barrier = matches!(ingress, session::WorldIngress::FastTransferBarrier { .. });
        drained.push(ingress);
        if is_barrier {
            break;
        }
    }
    drained
}

pub(crate) fn drain_network_ingress<T>(
    receiver: &mut tokio::sync::mpsc::Receiver<T>,
    budget: usize,
) -> Vec<T> {
    std::iter::from_fn(|| receiver.try_recv().ok())
        .take(budget)
        .collect()
}

#[cfg(test)]
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

#[cfg(test)]
pub(crate) fn update_actor_render_scene<'a>(
    scene: &'a mut ActorRenderScene,
    partial_tick: f32,
    cull_view: Option<ActorCullView>,
    mut remote_sources: Vec<ActorRenderSource>,
    local: Option<&FrozenLocalAvatarVisibility>,
) -> &'a ActorRenderFrame {
    if let Some(local) = local {
        remote_sources.retain(|source| source.runtime_id != local.runtime_id());
    }
    let local = local.filter(|local| local.visible()).map(|local| {
        let (yaw, pitch, _) = local.rotation().to_euler(bevy::math::EulerRot::YXZ);
        let yaw_degrees = (180.0 - yaw.to_degrees()).rem_euclid(360.0);
        let pitch_degrees = -pitch.to_degrees();
        let mut position = local.eye();
        position.y -= crate::local_player::LOCAL_AVATAR_EYE_HEIGHT_BLOCKS;
        ActorRenderSource {
            runtime_id: local.runtime_id(),
            unique_id: i64::try_from(local.runtime_id()).unwrap_or(i64::MAX),
            spawn_revision: local.session_generation(),
            movement_revision: local.pose_generation(),
            previous_position: position.to_array(),
            previous_pitch_degrees: pitch_degrees,
            previous_yaw_degrees: yaw_degrees,
            previous_head_yaw_degrees: yaw_degrees,
            position: position.to_array(),
            pitch_degrees,
            yaw_degrees,
            head_yaw_degrees: yaw_degrees,
            teleported: false,
            skin: None,
        }
    });
    scene.update_with_local(partial_tick, cull_view, remote_sources, local)
}

pub(crate) fn publish_actor_render_frame(
    mut client_world: ResMut<ClientWorld>,
    time: Res<Time<Real>>,
    mut scene: ResMut<ActorRenderScene>,
    mut frame: ResMut<ActorRenderFrame>,
    mut published_session: Local<Option<u64>>,
    mut actor_clock: Local<ActorFrameClock>,
    presentation: ActorPresentationState,
) {
    let ActorPresentationState {
        avatar,
        local_frame,
        mut local_visibility,
        settings,
        view,
        witness,
        camera,
    } = presentation;
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
    if let Some(local_frame) = local_frame.snapshot() {
        avatar.publish_visibility(local_frame, &mut local_visibility);
    } else {
        avatar.publish_view_visibility(
            settings.perspective(),
            view.eye_translation(),
            view.rotation(),
            &mut local_visibility,
        );
    }
    let cull_view = camera
        .single()
        .ok()
        .map(|(transform, projection)| ActorCullView {
            clip_from_world: projection.get_clip_from_view() * transform.to_matrix().inverse(),
            camera_position: transform.translation,
            max_distance: MAX_ACTOR_RENDER_DISTANCE_BLOCKS,
        });
    let (local_runtime_id, actor_session_id, dimension, remotes, canonical_local) = client_world
        .stream
        .as_ref()
        .map(|stream| {
            let local_runtime_id = stream.local_player_runtime_id();
            let mut remotes = Vec::new();
            let mut canonical_local = None;
            for rig in stream.actor_rigs() {
                let Some(actor) = stream.actor(rig.actor.runtime_id) else {
                    continue;
                };
                let profile = stream.actor_player_profile(rig.actor.runtime_id);
                let Some(presentation) =
                    actor_rig_presentation(&rig, actor, profile, step.partial_tick)
                else {
                    continue;
                };
                if rig.actor.runtime_id == local_runtime_id {
                    canonical_local = Some(presentation);
                } else {
                    remotes.push(presentation);
                }
            }
            (
                local_runtime_id,
                stream.actor_session_id(),
                stream.current_dimension(),
                remotes,
                canonical_local,
            )
        })
        .unwrap_or((0, 0, 0, Vec::new(), None));
    let visibility_snapshot = local_visibility.snapshot().copied();
    let (local_visible, local) = visibility_snapshot.map_or((false, None), |visibility| {
        if visibility.runtime_id() != local_runtime_id {
            return (false, None);
        }
        let (yaw, pitch, _) = visibility.rotation().to_euler(bevy::math::EulerRot::YXZ);
        let yaw_degrees = (180.0 - yaw.to_degrees()).rem_euclid(360.0);
        let pitch_degrees = -pitch.to_degrees();
        let mut position = visibility.eye();
        position.y -= crate::local_player::LOCAL_AVATAR_EYE_HEIGHT_BLOCKS;
        let diagnostic = local_diagnostic_presentation(
            actor_session_id,
            dimension,
            visibility.runtime_id(),
            visibility.pose_generation(),
            position.to_array(),
            yaw_degrees,
            pitch_degrees,
        );
        let local = local_actor_presentation_for_visibility(
            local_runtime_id,
            visibility.runtime_id(),
            canonical_local,
            diagnostic,
        );
        (visibility.visible(), local)
    });
    let batch = select_actor_presentations_for_view(
        local_runtime_id,
        local_visible,
        local,
        remotes,
        cull_view,
    );
    let selected_count = batch.submissions.len();
    *frame = update_actor_rig_scene(&mut scene, step.partial_tick, batch).clone();
    witness.observe_main(ActorMainWitness {
        local_snapshot: visibility_snapshot.is_some(),
        local_visible,
        expected_runtime_id: local_runtime_id,
        visibility_runtime_id: visibility_snapshot.map_or(0, |snapshot| snapshot.runtime_id()),
        selected_count,
        local_route: frame
            .rig
            .manifest
            .iter()
            .find(|entry| entry.identity.runtime_id == local_runtime_id)
            .map(|entry| entry.route),
        frame_instances: frame.rig.instances.len(),
        frame_manifest: frame.rig.manifest.len(),
        skin_bytes: frame.skins_rgba8.len(),
        rejects: frame.rig.rejects,
    });
}

pub(crate) mod session;
