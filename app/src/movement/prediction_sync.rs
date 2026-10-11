//! Adapts the ordered prediction-sync phase to its session transport.
use super::LocalPhysicsController;
use crate::runtime::{network::NetworkHandle, world::ClientWorld};
use bevy::prelude::{Local, Res, ResMut};
use gameplay::movement::PredictionSyncState;

/// Keeps the existing system slot while gameplay owns countdown and packet construction.
pub(crate) fn send_movement_prediction_sync(
    mut physics: ResMut<LocalPhysicsController>,
    client_world: Res<ClientWorld>,
    player_runtime: Res<crate::player_runtime::PlayerRuntime>,
    network: Option<Res<NetworkHandle>>,
    mut state: Local<PredictionSyncState>,
) {
    let (Some(network), Some(stream)) = (network.as_deref(), client_world.stream.as_ref()) else {
        return;
    };
    gameplay::movement::send_movement_prediction_sync(
        &mut physics,
        &super::GameplayWorldView(stream),
        player_runtime.facts.air_drag_modifier(),
        &mut state,
        |packet| network.send_movement_packet(packet).is_ok(),
    );
}
