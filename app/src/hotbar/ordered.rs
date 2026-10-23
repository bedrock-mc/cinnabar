//! Ordered selection is local until a tick or a same-frame hand action announces it.

use super::*;
use bevy::prelude::In;

/// Selects locally; the tick path or a later hand edge in this frame announces it.
pub(crate) fn select(
    In(action): In<semantic_input::Action>,
    mut player: ResMut<client_state::PlayerRuntime>,
    mut item_use: ResMut<client_interaction::item_use::ItemUseRuntime>,
) {
    let current = player.selected_hotbar_slot().unwrap_or(0);
    let Some(target) = client_interaction::hotbar::slot_for_action(action, current) else {
        return;
    };
    if current == target {
        return;
    }
    let game_mode = player.facts.player_game_mode();
    player
        .inventory
        .queue_local_hotbar_selection(target, game_mode);
    item_use.retire_reselected_use(target);
}

/// Finishes a pre-existing selection before an ordered press attempts to capture its item.
pub(crate) fn flush_existing(
    mut player: ResMut<client_state::PlayerRuntime>,
    network: Res<NetworkHandle<PackApplication>>,
    mut fatal: ResMut<FatalError>,
) -> bool {
    flush_existing_with(
        &mut player,
        &mut fatal.0,
        |packet| network.send_hotbar_packet(packet),
        || network.closed_command_has_pending_control(),
    )
}

/// Flushes the earlier selection while allowing terminal controls to classify a closure.
pub(super) fn flush_existing_with(
    player: &mut client_state::PlayerRuntime,
    fatal: &mut Option<String>,
    mut send: impl FnMut(Packet) -> Result<(), PacketSendError>,
    mut pending_control: impl FnMut() -> bool,
) -> bool {
    if pending_control() {
        return false;
    }
    flush_pending_hotbar_selection(player, fatal, |packet| match send(packet) {
        Err(PacketSendError::Closed(packet)) if pending_control() => {
            Err(PacketSendError::Full(packet))
        }
        result => result,
    });
    player.inventory.pending_hotbar_selection().is_none()
}
