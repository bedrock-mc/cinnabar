#[cfg(feature = "acceptance")]
use crate::acceptance::{
    AcceptanceRun,
    model_witness::ModelWitnessFileSource,
    mutation::{
        accepted_move_player_ingress_marker, move_player_ingress_marker,
        write_move_player_ingress_before_source_capture, write_stdout_marker,
    },
};
#[cfg(feature = "acceptance")]
use crate::runtime::phase3_evidence::{Phase3EvidenceEmitter, Phase3EvidenceEventKind};
#[cfg(feature = "acceptance")]
use crate::runtime::visibility::AppMetrics;
use std::sync::Arc;
use std::time::{Duration, Instant};

use bevy::{
    ecs::system::SystemParam,
    log::{debug, error, info, warn},
    prelude::{Res, ResMut},
};
use chunk_pipeline::WorldStream;
use client_world::SAFE_SERVER_HEIGHT;
use protocol::WorldEvent;
use render::{ChunkTextureAssets, ChunkUploadAcknowledgements, RuntimeStage, RuntimeStageProfiler};

use crate::{
    camera::{AutoFly, CameraSettingsAuthority},
    environment::{bind_session_generation, replace_session},
    local_player::{
        InteractionOriginSnapshot, LocalAvatarPresentation, LocalPlayerFrameCarrier,
        LocalPlayerFrameReset, LocalViewPose, reset_local_player_session,
    },
    movement::{MovementSource, PhysicsAuthorityGate, reset_start_game_prediction},
    runtime::{
        publication::PublicationController,
        shutdown::record_fatal_error,
        world::{AppWorldState, TransferNotice},
    },
    session::quiesce_local_player,
};
use client_ui::ui_runtime::{
    UiRuntime,
    inventory_router::{EquipmentRoute, EquipmentRouteResult, InventoryRouterError},
};

#[cfg(test)]
pub(crate) use client_session::WORLD_EVENT_CAPACITY;
pub(crate) use inventory::{
    publish_bootstrap_inventory, route_inventory_ingress, route_item_registry_ingress,
};
pub(crate) use pack_reload::{PackReload, reload_resource_packs};
#[cfg(test)]
pub(crate) use resource_packs::PackApplication;
pub(crate) use resource_packs::ui_catalog::PackUiCatalog;
pub(crate) use resource_packs::{
    BootstrapGenerationDisposition, ResourcePackAdmissionState, active_language_code,
    classify_bootstrap_generation, set_active_language, set_base_material_keys,
    set_base_terrain_catalog, set_compile_cache_dir,
};
pub(crate) use session::{
    BatchSendError, NetworkConfig, NetworkControlEvent, NetworkFailureOrigin, NetworkHandle,
    PacketSendError, SessionTransferTarget, session_failure_display, spawn_network,
};

/// One frame's world ingress time; terrain is bounded by admission, not by this.
pub(crate) const WORLD_INGRESS_DRAIN_BUDGET: Duration = Duration::from_millis(2);
pub(crate) const OUTBOUND_SEND_BUDGET_PER_FRAME: usize = 16;

#[derive(SystemParam)]
pub(crate) struct NetworkLocalPlayerState<'w> {
    view: ResMut<'w, LocalViewPose>,
    avatar: ResMut<'w, LocalAvatarPresentation>,
    settings: ResMut<'w, CameraSettingsAuthority>,
    frame: ResMut<'w, LocalPlayerFrameCarrier>,
    interaction: ResMut<'w, InteractionOriginSnapshot>,
    #[cfg(feature = "acceptance")]
    evidence: ResMut<'w, Phase3EvidenceEmitter>,
    authority: Res<'w, PhysicsAuthorityGate>,
    auto_fly: Res<'w, AutoFly>,
}

#[cfg(test)]
pub(crate) use client_presentation::actor_clock::{
    ActorFrameClock, authoritative_local_actor_eye, publish_local_actor_visibility,
};

/// Why a network session ended; each reason latches its own follow-up.
enum SessionEnd {
    /// `remote_close`: the receive side terminated, so the server closed it.
    Failed {
        failure: String,
        remote_close: bool,
    },
    /// The client ends the session to follow the server's transfer.
    Transferred(TransferNotice),
    Stopped,
}

#[derive(Debug, Clone, PartialEq)]
pub(crate) enum EquipmentIngress {
    Buffered,
    CommitOnly { fifo_sequence: u64 },
    ActorPresentation(Box<session::SequencedWorldEvent>),
}

pub(crate) fn publish_equipment_identity(
    player_runtime: &mut crate::player_runtime::PlayerRuntime,
    runtime: &mut UiRuntime,
    session_id: u64,
    runtime_id: u64,
) -> Result<Vec<EquipmentIngress>, InventoryRouterError> {
    let routes = runtime
        .publish_local_runtime_id(player_runtime, session_id, runtime_id)?
        .into_iter()
        .map(|route| consume_equipment_route(player_runtime, runtime, session_id, route))
        .collect();
    Ok(routes)
}

pub(crate) fn route_equipment_ingress(
    player_runtime: &mut crate::player_runtime::PlayerRuntime,
    runtime: &mut UiRuntime,
    sequenced: session::SequencedWorldEvent,
) -> Result<EquipmentIngress, InventoryRouterError> {
    let session_id = sequenced.session_generation;
    let WorldEvent::Equipment(event) = sequenced.event else {
        unreachable!("equipment routing accepts only equipment world events")
    };
    match runtime.route_equipment(player_runtime, session_id, sequenced.sequence, event)? {
        EquipmentRouteResult::Buffered => Ok(EquipmentIngress::Buffered),
        EquipmentRouteResult::Routed(route) => Ok(consume_equipment_route(
            player_runtime,
            runtime,
            session_id,
            route,
        )),
    }
}

fn consume_equipment_route(
    player_runtime: &mut crate::player_runtime::PlayerRuntime,
    runtime: &mut UiRuntime,
    session_generation: u64,
    route: EquipmentRoute,
) -> EquipmentIngress {
    match route {
        EquipmentRoute::LocalSelected {
            fifo_sequence,
            event,
        } => {
            runtime.retain_local_selected_equipment(player_runtime, fifo_sequence, event);
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
    mut player_runtime: bevy::prelude::ResMut<crate::player_runtime::PlayerRuntime>,
    mut network: ResMut<NetworkHandle>,
    mut resource_pack_admission: ResMut<ResourcePackAdmissionState>,
    mut pack_reload: Option<ResMut<PackReload>>,
    mut chunk_textures: Option<ResMut<ChunkTextureAssets>>,
    state: AppWorldState,
    #[cfg(feature = "acceptance")] mut acceptance: ResMut<AcceptanceRun>,
    #[cfg(feature = "acceptance")] metrics: Res<AppMetrics>,
    acknowledgements: Res<ChunkUploadAcknowledgements>,
    #[cfg(feature = "acceptance")] model_witness_source: Res<ModelWitnessFileSource>,
    publication: Res<PublicationController>,
    local_player: NetworkLocalPlayerState,
    profiler: Option<Res<RuntimeStageProfiler>>,
) {
    let _timer = profiler
        .as_deref()
        .map(|profiler| profiler.time(RuntimeStage::NetworkIngestion));
    let NetworkLocalPlayerState {
        mut view,
        mut avatar,
        mut settings,
        mut frame,
        mut interaction,
        #[cfg(feature = "acceptance")]
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
        mut movement_effects,
        mut movement_speed,
        mut collisions,
        mut ui_runtime,
        time,
    } = state;
    let display_interval = profiler
        .as_deref()
        .map_or(Duration::ZERO, RuntimeStageProfiler::frame_interval);
    if let Some(stream) = client_world.stream.as_mut() {
        stream.set_display_interval(display_interval);
        stream.begin_frame_work();
    }
    let controls =
        drain_network_controls(network.control_events_mut(), OUTBOUND_SEND_BUDGET_PER_FRAME);
    for control in controls {
        let (end, decode_error_count) = match control {
            NetworkControlEvent::Bootstrap {
                session_generation,
                world: bootstrap,
                environment,
                custom_blocks,
                inventory,
                item_registry,
                player_game_mode,
                world_default_game_mode,
                player_game_mode_uses_world_default,
                server_authoritative_block_breaking,
                rewind_history_size,
                hardcore,
                hud_rules,
                death_rules,
                packs,
                terrain_before_spawn,
            } => {
                match classify_bootstrap_generation(
                    ui_runtime.session_id(),
                    clock.session_generation(),
                    session_generation,
                ) {
                    BootstrapGenerationDisposition::Expected => {}
                    BootstrapGenerationDisposition::Stale => continue,
                    BootstrapGenerationDisposition::Unexpected => {
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
                }
                ui_runtime.set_server_lang(None);
                player_runtime.facts.clear_block_breaking_mode();
                player_runtime.facts.clear_local_abilities();
                acknowledgements.clear();
                frame.reset(LocalPlayerFrameReset::Session);
                interaction.invalidate();
                #[cfg(feature = "acceptance")]
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
                bind_session_generation(&mut clock, &mut weather, session_generation);
                crate::session::begin_session(
                    &mut ui_runtime,
                    &mut player_runtime,
                    session_generation,
                );
                movement_effects.begin_session(session_generation);
                movement_speed.begin_session(session_generation, bootstrap.dimension);
                item_diagnostics::session_registry(item_registry.as_ref());
                let world_item_registry = item_registry.clone();
                if !publish_bootstrap_inventory(
                    &mut player_runtime,
                    &mut ui_runtime,
                    item_registry,
                    inventory,
                ) {
                    record_fatal_error(
                        &mut client_world.fatal_error,
                        "StartGame inventory fanout was not an authority event".to_owned(),
                    );
                    continue;
                }
                if let Some(reload) = pack_reload.as_mut() {
                    reload.begin_session(session_generation, &packs);
                }
                resource_pack_admission.replace_for_generation(session_generation, packs.admission);
                ui_runtime.experiences.marker = packs.extension_marker;
                player_runtime.facts.publish_bootstrap_game_modes(
                    player_game_mode,
                    world_default_game_mode
                        .hud_mode()
                        .unwrap_or(protocol::PlayerGameMode::Unknown),
                    player_game_mode_uses_world_default,
                );
                ui_runtime.set_hardcore(hardcore);
                ui_runtime.apply_hud_rules(hud_rules);
                ui_runtime.apply_death_rules(death_rules);
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
                let hashed_ids = bootstrap.block_network_ids_are_hashes;
                let mut id_remap = assets::SequentialIdRemap::default();
                let custom_block_ids = if hashed_ids {
                    collisions.begin_session_custom_blocks(&protocol::CustomBlocks::default());
                    if collisions
                        .begin_session_hashed_custom_blocks(&custom_blocks)
                        .is_none()
                    {
                        warn!("server custom blocks have no collision base in hashed id mode");
                    }
                    None
                } else {
                    collisions
                        .begin_session_custom_blocks(&custom_blocks)
                        .map(|(range, remap)| {
                            id_remap = remap;
                            range
                        })
                };
                if !hashed_ids && custom_block_ids.is_none() && !custom_blocks.blocks.is_empty() {
                    warn!(
                        count = custom_blocks.blocks.len(),
                        "server custom blocks are unsupported in this id ordering"
                    );
                }
                // Hashed sessions append overlay visuals after the base and index them by hash.
                let overlay_ids = if hashed_ids {
                    packs.block_overlay.as_ref().map(|compiled| {
                        let first = client_world.runtime_assets.visual_count() as u32;
                        first..first + compiled.overlay.visuals.len() as u32
                    })
                } else {
                    custom_block_ids.clone()
                };
                let session_assets = resource_packs::session_runtime_assets(
                    &client_world.runtime_assets,
                    overlay_ids.as_ref(),
                    packs.block_overlay.as_deref(),
                );
                if let Some(textures) = chunk_textures.as_mut() {
                    resource_packs::install_chunk_textures(textures, &session_assets);
                }
                let mut stream = if let Some(entity_assets) = client_world.entity_assets.as_ref() {
                    WorldStream::new_with_asset_sets(
                        bootstrap,
                        Arc::clone(&session_assets),
                        Arc::clone(entity_assets),
                        current,
                        client_world.pending_surface_spawn,
                    )
                } else {
                    WorldStream::new_with_assets(
                        bootstrap,
                        session_assets,
                        current,
                        client_world.pending_surface_spawn,
                    )
                };
                if custom_blocks.skipped != 0 {
                    warn!(
                        skipped = custom_blocks.skipped,
                        "skipped malformed server block definitions"
                    );
                }
                stream.set_server_animation_compiler(server_animation::compile_stop);
                stream.set_world_default_game_mode(world_default_game_mode);
                stream.set_display_interval(display_interval);
                stream.begin_frame_work();
                stream.set_startup_priority(true);
                stream.set_startup_terrain_announced(terrain_before_spawn);
                stream.set_custom_block_ids(custom_block_ids.unwrap_or_default());
                stream.set_sequential_id_remap(id_remap);
                stream.set_custom_block_identities(&custom_blocks);
                stream.set_light_diagnostic_custom_blocks(custom_blocks.clone());
                stream.set_pack_entities(packs.entities.as_ref().map(|pack| {
                    (
                        Arc::clone(&pack.assets),
                        pack.bindings
                            .iter()
                            .map(|binding| binding.geometry_candidate)
                            .collect(),
                    )
                }));
                stream.seed_property_defaults(&packs.property_defaults);
                client_world.pack_entities = packs.entities.clone();
                client_world.prepared_actor_artwork = packs.prepared_actor_artwork.clone();
                client_world.session_items = Some(Arc::new(entity_pack::SessionItems {
                    components: packs.item_components.clone().unwrap_or_default(),
                    icons: packs.item_icons.clone(),
                }));
                if let Some(registry) = world_item_registry
                    && !stream.seed_item_registry(registry)
                {
                    warn!("StartGame item registry was refused by the world stream");
                }
                stream.set_publication_allowance(publication.allowance());
                let resolved = stream.resolved_server_position();
                #[cfg(feature = "acceptance")]
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
                local_physics.set_rewind_history_size(rewind_history_size);
                reset_start_game_prediction(
                    &mut movement,
                    &mut local_physics,
                    clock.session_generation(),
                    resolved.position,
                );
                if let Err(fault) = physics_authority.apply_start_game(
                    auto_fly.enabled(),
                    collisions.is_complete(),
                    &mut movement,
                    &mut local_physics,
                ) {
                    movement.set_source(MovementSource::FreeCamera);
                    local_physics.deactivate();
                    record_fatal_error(
                        &mut client_world.fatal_error,
                        format!("local Physics authority failed closed: {fault:?}"),
                    );
                }
                client_world.pending_surface_spawn = resolved.surface_anchor;
                client_world.stream = Some(stream);
                let routed = match publish_equipment_identity(
                    &mut player_runtime,
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
                resource_packs::install_server_language(
                    &mut ui_runtime,
                    session_generation,
                    packs.server_lang,
                    client_world.fatal_error.is_none(),
                );
                resource_packs::install_session_icons(
                    &mut ui_runtime,
                    session_generation,
                    packs.item_icons,
                    packs.item_components,
                    client_world.fatal_error.is_none(),
                );
                resource_packs::install_server_ui(
                    &mut ui_runtime,
                    session_generation,
                    packs.server_ui,
                    client_world.fatal_error.is_none(),
                );
                resource_packs::install_session_glyphs(
                    &mut ui_runtime,
                    session_generation,
                    packs.glyph_sheets,
                    client_world.fatal_error.is_none(),
                );
                crate::audio::publish_server_sounds(packs.server_sounds);
                player_runtime.facts.install_block_breaking_mode(
                    session_generation,
                    server_authoritative_block_breaking,
                    client_world.fatal_error.is_none(),
                );
                if let Some(stream) = client_world.stream.as_ref() {
                    player_runtime.facts.bind_local_abilities(
                        session_generation,
                        stream.biome_tint_identity().stream(),
                        bootstrap.local_player_unique_id,
                        client_world.fatal_error.is_none(),
                    );
                }
                continue;
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
                continue;
            }
            NetworkControlEvent::ChatPacketSent { session, sequence } => {
                if !ui_runtime.acknowledge_chat_send(session, sequence) {
                    warn!(
                        session,
                        sequence, "ignored unrelated chat send acknowledgement"
                    );
                }
                continue;
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
                continue;
            }
            NetworkControlEvent::PhysicsPacketSent { identity } => {
                if !movement.acknowledge_physics_send(identity) {
                    warn!(
                        session_generation = identity.session_generation,
                        tick = identity.tick,
                        admission_id = identity.admission_id,
                        reanchor_epoch = identity.reanchor_epoch,
                        "ignored stale, duplicate, or out-of-order physics send acknowledgement"
                    );
                }
                continue;
            }
            NetworkControlEvent::PhysicsPacketCancelled {
                identity,
                definitely_unsent,
            } => {
                if !movement.resolve_cancelled_physics_send(identity, definitely_unsent) {
                    warn!(
                        session_generation = identity.session_generation,
                        tick = identity.tick,
                        admission_id = identity.admission_id,
                        reanchor_epoch = identity.reanchor_epoch,
                        definitely_unsent,
                        "ignored stale, duplicate, or out-of-order physics cancellation"
                    );
                }
                continue;
            }
            NetworkControlEvent::BlobCacheTelemetry { enabled, stats } => {
                client_world.client_blob_cache_enabled = enabled;
                client_world.client_blob_cache = stats;
                continue;
            }
            NetworkControlEvent::Failed {
                message,
                decode_error_count,
                server_disconnect,
                origin,
            } => (
                SessionEnd::Failed {
                    failure: session_failure_display(&message, server_disconnect.as_ref()),
                    remote_close: origin == NetworkFailureOrigin::Receive,
                },
                decode_error_count,
            ),
            NetworkControlEvent::Transferred {
                target: SessionTransferTarget { host, port },
                decode_error_count,
            } => (
                SessionEnd::Transferred(TransferNotice { host, port }),
                decode_error_count,
            ),
            NetworkControlEvent::Stopped { decode_error_count } => {
                (SessionEnd::Stopped, decode_error_count)
            }
        };
        // Every terminal event retires the same session-owned state.
        UiRuntime::retire_crafting_observation();
        render::ViewmodelCompletionGate::retire_observation();
        resource_pack_admission.clear_current();
        if let Some(reload) = pack_reload.as_mut() {
            reload.end_session();
        }
        ui_runtime.set_server_lang(None);
        player_runtime.facts.clear_block_breaking_mode();
        player_runtime.facts.clear_local_abilities();
        // Only a receive-side termination is a remote-initiated close; latch it
        // while the ticker still reports the live session.
        if matches!(
            end,
            SessionEnd::Failed {
                remote_close: true,
                ..
            }
        ) {
            movement.note_remote_session_close();
        }
        quiesce_local_player(
            &mut movement,
            &mut local_physics,
            &mut frame,
            &mut interaction,
        );
        avatar.clear();
        client_world.network_decode_errors = decode_error_count;
        match end {
            SessionEnd::Failed { failure, .. } => {
                error!(decode_error_count, "{failure}");
                record_fatal_error(&mut client_world.fatal_error, failure);
            }
            SessionEnd::Transferred(notice) => {
                info!(
                    host = notice.host.as_str(),
                    port = notice.port,
                    "server transferred the session"
                );
                client_world.transfer_notice = Some(notice);
            }
            SessionEnd::Stopped => {
                if client_world.fatal_error.is_none() {
                    client_world.fatal_error = Some("network session stopped unexpectedly".into());
                }
            }
        }
    }

    let mut drain = WorldIngressDrain::new(Instant::now() + WORLD_INGRESS_DRAIN_BUDGET);
    loop {
        let admission_capacity = client_world
            .stream
            .as_ref()
            .map_or(1, WorldStream::remaining_admission_capacity);
        let Some(ingress) = drain.next(
            network.world_events_mut(),
            admission_capacity,
            Instant::now(),
        ) else {
            break;
        };
        let sequenced = match ingress {
            session::WorldIngress::Event(sequenced) => {
                network.record_readiness_event_consumed(&sequenced.event);
                sequenced
            }
            session::WorldIngress::LevelChunk {
                session_generation,
                sequence,
                event,
                payload,
            } => {
                network.record_level_chunk_consumed();
                if session_generation != ui_runtime.session_id() {
                    record_fatal_error(
                        &mut client_world.fatal_error,
                        format!(
                            "world ingress crossed a session boundary: expected {}, got {session_generation}",
                            ui_runtime.session_id()
                        ),
                    );
                    continue;
                }
                let Some(stream) = client_world.stream.as_mut() else {
                    record_fatal_error(
                        &mut client_world.fatal_error,
                        "received LevelChunk before StartGame bootstrap".to_owned(),
                    );
                    continue;
                };
                #[cfg(feature = "acceptance")]
                let observed_at = Instant::now();
                #[cfg(feature = "acceptance")]
                let metadata = WorldEvent::LevelChunk(event.clone());
                #[cfg(feature = "acceptance")]
                acceptance.observe_mutation(&metadata, observed_at);
                #[cfg(feature = "acceptance")]
                if acceptance.observe_full_view_teleport_ingress(
                    &metadata,
                    sequence,
                    observed_at,
                    stream.current_dimension(),
                    metrics.0.frame_count(),
                ) {
                    stream.schedule_source_capture(sequence);
                }
                let submitted = stream.submit_level_chunk_bytes(sequence, event, payload);
                stream.dispatch_ingress_decode();
                if let Err(error) = submitted {
                    record_fatal_error(
                        &mut client_world.fatal_error,
                        format!("world FIFO rejected LevelChunk: {error}"),
                    );
                }
                continue;
            }
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
                if let Err(error) = stream.commit_barrier(sequence) {
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
            match route_equipment_ingress(&mut player_runtime, &mut ui_runtime, sequenced) {
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
            let commit_sequence =
                match route_inventory_ingress(&mut player_runtime, &mut ui_runtime, sequenced) {
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
            if matches!(
                &sequenced.event,
                WorldEvent::ItemActor(protocol::ItemActorEvent::Registry(_))
            ) && let Err(error) =
                route_item_registry_ingress(&mut player_runtime, &mut ui_runtime, &sequenced)
            {
                record_fatal_error(
                    &mut client_world.fatal_error,
                    format!("item registry ingress rejected: {error:?}"),
                );
                continue;
            }
            sequenced
        };
        #[cfg(feature = "acceptance")]
        let observed_at = Instant::now();
        #[cfg(feature = "acceptance")]
        if model_witness_source.configured()
            && let protocol::WorldEvent::MovePlayer(movement) = &sequenced.event
            && let Some(marker) = move_player_ingress_marker(sequenced.sequence, movement.position)
        {
            let mut stdout = std::io::stdout().lock();
            write_stdout_marker(&mut stdout, &marker);
        }
        #[cfg(feature = "acceptance")]
        acceptance.observe_mutation(&sequenced.event, observed_at);
        #[cfg(feature = "acceptance")]
        let accepted_binding_ingress = acceptance.observe_full_view_teleport_ingress(
            &sequenced.event,
            sequenced.sequence,
            observed_at,
            stream.current_dimension(),
            metrics.0.frame_count(),
        );
        #[cfg(feature = "acceptance")]
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
        let submitted = stream.submit(sequenced.sequence, sequenced.event);
        stream.dispatch_ingress_decode();
        if let Err(error) = submitted {
            client_world.fatal_error = Some(format!("world FIFO rejected data: {error}"));
        }
    }
    if let Some(stream) = client_world.stream.as_mut() {
        item_diagnostics::equipment(stream.take_equipment_notices());
    }
}

#[cfg(test)]
pub(crate) use client_presentation::actor_publication::PreparedActorPublication;
#[cfg(test)]
mod actor_test_support;
#[cfg(test)]
pub(crate) use actor_test_support::{actor_render_source, update_actor_render_scene};

mod actor_publication;
mod block_overlay;
mod drain;
pub(crate) mod entity_pack;
mod entity_texture_reload;
mod glyph_sheets;
mod inventory;
mod item_diagnostics;
mod item_icons;
mod server_animation;
pub(crate) use item_icons::set_vanilla_item_paths;
#[cfg(test)]
mod local_pack;
mod pack_reload;
mod pack_reload_diff;
mod pack_reload_geometry;
#[cfg(test)]
mod pack_reload_tests;
#[cfg(test)]
mod pack_reload_world_witness;
pub(crate) use entity_texture_reload::set_base_actor_artwork;
pub(crate) mod reload_environment;
mod resource_packs;
pub(crate) mod session;
pub(crate) use actor_publication::{
    ActorFramePartialTick, HandRigBuilder, advance_actor_frame, prepare_actor_render_frame,
    publish_actor_render_frame, publish_entity_shadows, publish_local_actor_damage,
};

#[cfg(test)]
pub(crate) use drain::drain_network_ingress;
pub(crate) use drain::{WorldIngressDrain, drain_network_controls};

#[cfg(feature = "acceptance")]
pub(crate) use acceptance::committed_control::acceptance_surface_anchor;
