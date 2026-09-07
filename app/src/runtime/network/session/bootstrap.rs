use bevy::log::warn;
use protocol::{
    InventoryEvent, ItemActorEvent, ItemRegistryEvent, WorldEvent, into_world_event,
    normalize_authority,
};
use tokio::sync::{mpsc, watch};

use super::{NetworkControlEvent, NetworkFailureOrigin, send_control_event_or_cancel};

pub(super) fn start_game_inventory_authority(game_data: &protocol::GameData) -> InventoryEvent {
    normalize_authority(game_data.start_game.enable_item_stack_net_manager)
}

pub(super) fn start_game_item_registry(
    game_data: &protocol::GameData,
    current_dimension: i32,
) -> Result<Option<ItemRegistryEvent>, protocol::WorldPacketError> {
    match into_world_event(game_data.item_registry.clone().into(), current_dimension) {
        Ok(Some(WorldEvent::ItemActor(ItemActorEvent::Registry(registry)))) => Ok(Some(registry)),
        // A semantically unsupported registry removes merge authority but does
        // not turn an otherwise valid StartGame into a transport failure.
        Ok(_) => Ok(None),
        Err(protocol::WorldPacketError::Item(_)) => {
            warn!(
                "StartGame item registry was semantically unsupported; merge authority unavailable"
            );
            Ok(None)
        }
        Err(error @ protocol::WorldPacketError::Wire(_)) => Err(error),
        Err(error) => Err(error),
    }
}

pub(super) async fn send_startup_failure(
    control_events: &mpsc::Sender<NetworkControlEvent>,
    shutdown: &mut watch::Receiver<bool>,
    error: impl std::fmt::Display,
) {
    let _ = send_control_event_or_cancel(
        control_events,
        shutdown,
        NetworkControlEvent::Failed {
            message: error.to_string(),
            decode_error_count: 0,
            server_disconnect: None,
            origin: NetworkFailureOrigin::Startup,
        },
    )
    .await;
}
