use protocol::{InventoryEvent, ItemRegistryEvent};
use tokio::sync::{mpsc, watch};
use tracing::warn;

use super::{NetworkControlEvent, NetworkFailureOrigin, send_control_event_or_cancel};

pub(super) fn start_game_inventory_authority(game_data: &protocol::GameData) -> InventoryEvent {
    InventoryEvent::Authority(game_data.settings.inventory_authority)
}

pub(super) fn start_game_item_registry(
    game_data: &protocol::GameData,
) -> Result<Option<ItemRegistryEvent>, protocol::WorldPacketError> {
    match &game_data.items.registry {
        Ok(registry) => Ok(Some(registry.clone())),
        // A semantically unsupported registry removes merge authority but does
        // not turn an otherwise valid StartGame into a transport failure.
        Err(protocol::WorldPacketError::Item(error)) => {
            warn!(
                %error, "StartGame item registry was semantically unsupported; merge authority unavailable"
            );
            Ok(None)
        }
        Err(error) => Err(error.clone()),
    }
}

pub(super) async fn send_startup_failure<P>(
    control_events: &mpsc::Sender<NetworkControlEvent<P>>,
    shutdown: &mut watch::Receiver<bool>,
    error: impl std::fmt::Display,
    server_disconnect: Option<protocol::ServerDisconnectEvent>,
) {
    let _ = send_control_event_or_cancel(
        control_events,
        shutdown,
        NetworkControlEvent::Failed {
            message: error.to_string(),
            decode_error_count: 0,
            server_disconnect,
            origin: NetworkFailureOrigin::Startup,
        },
    )
    .await;
}
