use client_ui::ui_runtime::UiRuntime;
use inventory::inventory_router::{EquipmentRoute, EquipmentRouteResult, InventoryRouterError};
use protocol::WorldEvent;
use protocol::identity::FifoSequence;
use simulation_clock::identity::SessionGeneration;

#[derive(Debug, Clone, PartialEq)]
pub(crate) enum EquipmentIngress {
    Buffered,
    CommitOnly { fifo_sequence: u64 },
    ActorPresentation(Box<client_session::SequencedWorldEvent>),
}

/// Publishes the local identity and routes equipment buffered before it was known.
pub(crate) fn publish_equipment_identity(
    player_runtime: &mut client_state::PlayerRuntime,
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

/// Routes local equipment to inventory and leaves remote equipment for the world FIFO.
pub(crate) fn route_equipment_ingress(
    player_runtime: &mut client_state::PlayerRuntime,
    runtime: &mut UiRuntime,
    sequenced: client_session::SequencedWorldEvent,
) -> Result<EquipmentIngress, InventoryRouterError> {
    let session_id = sequenced.session_generation.get();
    let WorldEvent::Equipment(event) = sequenced.event else {
        unreachable!("equipment routing accepts only equipment world events")
    };
    match runtime.route_equipment(player_runtime, session_id, sequenced.sequence.get(), event)? {
        EquipmentRouteResult::Buffered => Ok(EquipmentIngress::Buffered),
        EquipmentRouteResult::Routed(route) => Ok(consume_equipment_route(
            player_runtime,
            runtime,
            session_id,
            route,
        )),
    }
}

/// Applies an inventory route or returns the actor event with its original sequence.
fn consume_equipment_route(
    player_runtime: &mut client_state::PlayerRuntime,
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
        } => EquipmentIngress::ActorPresentation(Box::new(client_session::SequencedWorldEvent {
            session_generation: SessionGeneration::new(session_generation),
            sequence: FifoSequence::new(fifo_sequence),
            event: WorldEvent::Equipment(event),
        })),
    }
}

/// Releases buffered equipment after the stream exists; identity failure stops bootstrap.
/// A FIFO rejection records the failure but still runs the presentation checks.
pub(super) fn publish_bootstrap_equipment(
    player_runtime: &mut client_state::PlayerRuntime,
    ui_runtime: &mut UiRuntime,
    client_world: &mut crate::runtime::world::ClientWorld,
    fatal_error: &mut Option<String>,
    session_generation: u64,
    runtime_id: u64,
) -> bool {
    use crate::runtime::shutdown::record_fatal_error;
    let routed = match publish_equipment_identity(
        player_runtime,
        ui_runtime,
        session_generation,
        runtime_id,
    ) {
        Ok(routed) => routed,
        Err(error) => {
            record_fatal_error(
                fatal_error,
                format!("inventory identity publication failed: {error:?}"),
            );
            return false;
        }
    };
    for route in routed {
        let result = match route {
            EquipmentIngress::ActorPresentation(sequenced) => {
                let sequenced = *sequenced;
                client_world
                    .stream
                    .as_mut()
                    .map(|stream| stream.submit(sequenced.sequence.get(), sequenced.event))
            }
            EquipmentIngress::CommitOnly { fifo_sequence } => client_world
                .stream
                .as_mut()
                .map(|stream| stream.commit(fifo_sequence)),
            EquipmentIngress::Buffered => None,
        };
        if let Some(Err(error)) = result {
            record_fatal_error(
                fatal_error,
                format!("world FIFO rejected buffered equipment: {error}"),
            );
            break;
        }
    }
    true
}
