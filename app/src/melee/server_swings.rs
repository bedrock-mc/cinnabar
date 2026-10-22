//! Echo admitted server swings before local input and simulation consume the frame.

use super::*;
use crate::runtime::shutdown::{FatalError, record_fatal_error};
use bevy::prelude::Local;
use gameplay::melee::{ServerSwingContext, ServerSwingEcho};

/// Drains local callbacks after ingress and retries them until the network accepts each echo.
#[allow(
    clippy::too_many_arguments,
    reason = "ECS borrows the independent gameplay owners."
)]
pub(crate) fn echo(
    mut world: ResMut<ClientWorld>,
    player: Res<client_state::PlayerRuntime>,
    movement: Res<MovementTicker>,
    effects: Res<LocalMovementEffectTimeline>,
    mut swings: ResMut<SwingTracker>,
    network: Res<NetworkHandle<PackApplication>>,
    mut fatal: ResMut<FatalError>,
    mut pending: Local<ServerSwingEcho>,
) {
    pending.synchronize(movement.interaction_authority_identity());
    let item_duration =
        selected_attack_timing(&player, &world).and_then(|timing| timing.swing_duration_ticks);
    let Some(stream) = world.stream.as_mut() else {
        return;
    };
    swings.sync_ticks_for_item(
        movement.interaction_authority_identity(),
        movement.completed_tick(),
        &effects,
        item_duration,
    );
    pending.extend(stream.take_local_server_swings());
    let result = pending.flush(
        ServerSwingContext {
            tick: movement.completed_tick().saturating_add(1),
            runtime_id: stream.local_player_runtime_id(),
            effects: effects.mining_effects(),
            item_duration,
        },
        &mut swings,
        |packet| network.send_inventory_packets(vec![packet]),
    );
    if result == Err(gameplay::BatchSendError::Closed)
        && !network.closed_command_has_pending_control()
    {
        record_fatal_error(
            &mut fatal.0,
            "server swing echo failed because the network command channel closed".to_owned(),
        );
    }
}
