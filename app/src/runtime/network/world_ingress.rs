use super::{
    equipment::{EquipmentIngress, route_equipment_ingress},
    resource_packs::PackApplication,
};
use crate::runtime::{shutdown::record_fatal_error, world::ClientWorld};
use bevy::log::info;
use bevy::prelude::ResMut;
use chunk_pipeline::WorldStream;
use client_runtime::{
    ingress::{WORLD_INGRESS_DRAIN_BUDGET, WorldIngressDrain},
    inventory::{
        route_crafting_rules_ingress, route_inventory_ingress, route_item_registry_ingress,
    },
    network::NetworkHandle,
};
use client_shell::ui_resources::UiRuntimeResource;
use client_state::PlayerRuntime;
use protocol::WorldEvent;
use web_time::Instant;
#[cfg(feature = "acceptance")]
use {
    crate::runtime::visibility::AppMetrics,
    acceptance::{
        AcceptanceRun,
        model_witness::ModelWitnessFileSource,
        mutation::{
            accepted_move_player_ingress_marker, move_player_ingress_marker,
            write_move_player_ingress_before_source_capture,
        },
    },
    diagnostics::write_stdout_marker,
};

/// Records a world event's commit in the latency trace; installed only while tracing.
pub(super) fn trace_committed(fifo: u64) {
    if let Some(trace) = bridge::latency_trace::active() {
        trace.record(
            bridge::latency_trace::kind::COMMITTED,
            &[fifo as i64, trace.now()],
        );
    }
}

/// Drains available lanes while the stream preserves dependencies and transfer barriers.
#[allow(clippy::too_many_arguments)]
pub(super) fn drain(
    network: &mut NetworkHandle<PackApplication>,
    client_world: &mut ClientWorld,
    fatal_error: &mut Option<String>,
    player_runtime: &mut ResMut<PlayerRuntime>,
    ui_runtime: &mut ResMut<UiRuntimeResource>,
    #[cfg(feature = "acceptance")] acceptance: &mut ResMut<AcceptanceRun>,
    #[cfg(feature = "acceptance")] metrics: &AppMetrics,
    #[cfg(feature = "acceptance")] model_witness_source: &ModelWitnessFileSource,
) {
    let mut drain = WorldIngressDrain::new(Instant::now() + WORLD_INGRESS_DRAIN_BUDGET);
    loop {
        let (receiver, pending) = network.world_ingress_mut();
        let ingress = match drain.next(
            receiver,
            pending,
            client_world.stream.as_mut(),
            Instant::now,
        ) {
            Ok(Some(ingress)) => ingress,
            Ok(None) => break,
            Err(error) => {
                record_fatal_error(fatal_error, format!("world ingress rejected data: {error}"));
                break;
            }
        };
        if let Some(trace) = bridge::latency_trace::active() {
            let (session, fifo) = ingress.identity();
            trace.record(
                bridge::latency_trace::kind::DRAINED,
                &[session as i64, fifo as i64, trace.now()],
            );
        }
        let sequenced = match ingress {
            client_session::WorldIngress::Event(sequenced) => {
                network.record_readiness_event_consumed(&sequenced.event);
                sequenced
            }
            client_session::WorldIngress::LevelChunk {
                session_generation,
                sequence,
                event,
                payload,
            } => {
                network.record_level_chunk_consumed();
                let session_generation = session_generation.get();
                let sequence = sequence.get();
                if session_generation != ui_runtime.session_id() {
                    record_fatal_error(
                        fatal_error,
                        format!(
                            "world ingress crossed a session boundary: expected {}, got {session_generation}",
                            ui_runtime.session_id()
                        ),
                    );
                    continue;
                }
                let Some(stream) = client_world.stream.as_mut() else {
                    record_fatal_error(
                        fatal_error,
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
                        fatal_error,
                        format!("world FIFO rejected LevelChunk: {error}"),
                    );
                }
                continue;
            }
            client_session::WorldIngress::FastTransferBarrier {
                session_generation,
                sequence,
                action_sequence,
            } => {
                let session_generation = session_generation.get();
                let sequence = sequence.get();
                if session_generation != ui_runtime.session_id() {
                    record_fatal_error(
                        fatal_error,
                        format!(
                            "fast-transfer barrier crossed a session boundary: expected {}, got {session_generation}",
                            ui_runtime.session_id()
                        ),
                    );
                    continue;
                }
                let Some(stream) = client_world.stream.as_mut() else {
                    record_fatal_error(
                        fatal_error,
                        "received fast-transfer barrier before StartGame bootstrap".to_owned(),
                    );
                    continue;
                };
                if let Err(error) = stream.commit_barrier(sequence) {
                    record_fatal_error(
                        fatal_error,
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
        if sequenced.session_generation.get() != ui_runtime.session_id() {
            record_fatal_error(
                fatal_error,
                format!(
                    "world ingress crossed a session boundary: expected {}, got {}",
                    ui_runtime.session_id(),
                    sequenced.session_generation.get()
                ),
            );
            continue;
        }
        let Some(stream) = client_world.stream.as_mut() else {
            *fatal_error = Some("received world data before StartGame bootstrap".to_owned());
            continue;
        };
        let sequenced = match route_player_ingress(player_runtime, ui_runtime, stream, sequenced) {
            Ok(Some(sequenced)) => sequenced,
            Ok(None) => continue,
            Err(error) => {
                record_fatal_error(fatal_error, error);
                continue;
            }
        };
        if matches!(&sequenced.event, WorldEvent::Actor(protocol::ActorEvent::Status(status))
            if status.runtime_id == stream.authority().local_player_runtime_id()
                && status.kind == protocol::ActorStatusKind::TotemActivate)
        {
            ui_runtime.retain_totem_snapshot(
                player_runtime,
                sequenced.session_generation.get(),
                sequenced.sequence.get(),
            );
        }
        #[cfg(feature = "acceptance")]
        observe_event(
            acceptance,
            metrics,
            model_witness_source,
            stream,
            &sequenced,
        );
        let submitted = stream.submit(sequenced.sequence.get(), sequenced.event);
        stream.dispatch_ingress_decode();
        if let Err(error) = submitted {
            *fatal_error = Some(format!("world FIFO rejected data: {error}"));
        }
    }
    if let Some(stream) = client_world.stream.as_mut() {
        client_runtime::item_diagnostics::equipment(stream.take_equipment_notices());
    }
}

/// Commits local inventory events, returning only events that still need world submission.
fn route_player_ingress(
    player_runtime: &mut ResMut<PlayerRuntime>,
    ui_runtime: &mut ResMut<UiRuntimeResource>,
    stream: &mut WorldStream,
    sequenced: client_session::SequencedWorldEvent,
) -> Result<Option<client_session::SequencedWorldEvent>, String> {
    if matches!(&sequenced.event, WorldEvent::Equipment(_)) {
        match route_equipment_ingress(player_runtime, ui_runtime, sequenced) {
            Ok(EquipmentIngress::ActorPresentation(sequenced)) => Ok(Some(*sequenced)),
            Ok(EquipmentIngress::CommitOnly { fifo_sequence }) => {
                if let Err(error) = stream.commit(fifo_sequence) {
                    return Err(format!(
                        "world FIFO rejected local equipment commit: {error}"
                    ));
                }
                Ok(None)
            }
            Ok(EquipmentIngress::Buffered) => Ok(None),
            Err(error) => Err(format!("equipment ingress rejected: {error:?}")),
        }
    } else if matches!(&sequenced.event, WorldEvent::Inventory(_)) {
        let commit_sequence = match route_inventory_ingress(player_runtime, ui_runtime, sequenced) {
            Ok(sequence) => sequence,
            Err(error) => {
                return Err(format!("inventory ingress rejected: {error:?}"));
            }
        };
        if let Err(error) = stream.commit(commit_sequence) {
            return Err(format!("world FIFO rejected inventory commit: {error}"));
        }
        Ok(None)
    } else {
        if matches!(&sequenced.event, WorldEvent::GameRules(_))
            && let Err(error) = route_crafting_rules_ingress(player_runtime, ui_runtime, &sequenced)
        {
            return Err(format!("crafting rule ingress rejected: {error:?}"));
        }
        if matches!(
            &sequenced.event,
            WorldEvent::ItemActor(protocol::ItemActorEvent::Registry(_))
        ) && let Err(error) = route_item_registry_ingress(player_runtime, ui_runtime, &sequenced)
        {
            return Err(format!("item registry ingress rejected: {error:?}"));
        }
        Ok(Some(sequenced))
    }
}

/// Records ordinary ingress and schedules any requested source capture before submitting it.
#[cfg(feature = "acceptance")]
fn observe_event(
    acceptance: &mut AcceptanceRun,
    metrics: &AppMetrics,
    model_witness_source: &ModelWitnessFileSource,
    stream: &mut WorldStream,
    sequenced: &client_session::SequencedWorldEvent,
) {
    let observed_at = Instant::now();
    if model_witness_source.configured()
        && let protocol::WorldEvent::MovePlayer(movement) = &sequenced.event
        && let Some(marker) =
            move_player_ingress_marker(sequenced.sequence.get(), movement.position)
    {
        let mut stdout = diagnostics::console::stdout();
        write_stdout_marker(&mut stdout, &marker);
    }
    acceptance.observe_mutation(&sequenced.event, observed_at);
    let accepted_binding_ingress = acceptance.observe_full_view_teleport_ingress(
        &sequenced.event,
        sequenced.sequence.get(),
        observed_at,
        stream.current_dimension(),
        metrics.0.frame_count(),
    );
    if accepted_binding_ingress {
        if let Some(ingress_marker) = accepted_move_player_ingress_marker(
            accepted_binding_ingress,
            sequenced.sequence.get(),
            &sequenced.event,
        ) {
            let mut stdout = diagnostics::console::stdout();
            write_move_player_ingress_before_source_capture(&mut stdout, &ingress_marker, || {
                stream.schedule_source_capture(sequenced.sequence.get())
            });
        } else {
            stream.schedule_source_capture(sequenced.sequence.get());
        }
    }
}

#[cfg(test)]
mod crafting_rule_tests {
    use super::route_crafting_rules_ingress;
    use client_session::SequencedWorldEvent;
    use client_ui::ui_runtime::UiRuntime;
    use protocol::WorldEvent;

    #[test]
    fn crafting_rules_route_to_inventory_at_the_world_commit_position() {
        let mut player = client_state::PlayerRuntime(player_state::PlayerState::new(1));
        let mut runtime = UiRuntime::new(1);
        let event = SequencedWorldEvent {
            session_generation: simulation_clock::identity::SessionGeneration::new(1),
            sequence: protocol::identity::FifoSequence::new(5),
            event: WorldEvent::GameRules(protocol::GameRulesEvent {
                daylight_cycle: None,
                weather_cycle: None,
                hud: Default::default(),
                death: Default::default(),
                crafting: protocol::CraftingRules {
                    recipes_unlock: None,
                    limited_crafting: Some(true),
                },
            }),
        };
        route_crafting_rules_ingress(&mut player, &mut runtime, &event).unwrap();
        player
            .inventory
            .synchronize_crafting_frontier(1, Some((1, 0, Some(4))));
        player.inventory.drain_pending_inventory();
        assert!(
            !player
                .inventory
                .crafting_rules()
                .unwrap()
                .requires_manual_unlock()
        );
        player
            .inventory
            .synchronize_crafting_frontier(1, Some((1, 0, Some(5))));
        player.inventory.finish_drain();
        assert!(
            player
                .inventory
                .crafting_rules()
                .unwrap()
                .requires_manual_unlock()
        );
    }
}
