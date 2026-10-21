//! Adapts the ordered prediction-sync phase to its session transport.
use crate::runtime::network::resource_packs::PackApplication;
use crate::runtime::world::ClientWorld;
use bevy::prelude::{Res, ResMut};
use client_interaction::movement::LocalPhysicsController;
use client_runtime::network::NetworkHandle;

/// Keeps the existing system slot while gameplay owns countdown and packet construction.
pub(crate) fn send_movement_prediction_sync(
    mut physics: ResMut<LocalPhysicsController>,
    client_world: Res<ClientWorld>,
    player_runtime: Res<client_state::PlayerRuntime>,
    network: Option<Res<NetworkHandle<PackApplication>>>,
) {
    let (Some(network), Some(stream)) = (network.as_deref(), client_world.stream.as_ref()) else {
        return;
    };
    gameplay::movement::send_movement_prediction_sync(
        &mut physics,
        &client_interaction::movement::world_view::GameplayWorldView(stream),
        player_runtime.facts.air_drag_modifier(),
        |packet| network.send_movement_packet(packet).is_ok(),
    );
}
