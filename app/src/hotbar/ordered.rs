//! Ordered selection publishes only after its immediate transport admission.

use super::*;
use bevy::prelude::In;

/// Attempts one selection before publishing it to later edges in this frame.
pub(crate) fn select(
    In(action): In<semantic_input::Action>,
    mut player: ResMut<client_state::PlayerRuntime>,
    network: Res<NetworkHandle<PackApplication>>,
    mut fatal: ResMut<FatalError>,
    mut item_use: ResMut<client_interaction::item_use::ItemUseRuntime>,
    mut movement: ResMut<client_interaction::movement::MovementTicker>,
) {
    let current = player.selected_hotbar_slot().unwrap_or(0);
    let Some(target) = client_interaction::hotbar::slot_for_action(action, current) else {
        return;
    };
    if current == target {
        return;
    }
    let proposal = match player.inventory.prepare_hotbar_selection(target) {
        Ok(Some(proposal)) => proposal,
        Ok(None) => return,
        Err(error) => {
            record_fatal_error(
                &mut fatal.0,
                format!("hotbar selection packet validation failed: {error}"),
            );
            return;
        }
    };
    match network.send_hotbar_packet(proposal.packet().clone()) {
        Ok(()) => {
            player.inventory.set_local_selected_slot(target);
            player.inventory.clear_pending_hotbar_selection(target);
            item_use.retire_reselected_use(target);
            if let Some(tick) = item_use.take_withdrawn_start() {
                movement.withdraw_started_using_item(tick);
            }
        }
        Err(PacketSendError::Full(_)) => {}
        Err(PacketSendError::Closed(_)) if network.closed_command_has_pending_control() => {}
        Err(PacketSendError::Closed(_)) => record_fatal_error(
            &mut fatal.0,
            "hotbar selection send failed because the network command channel closed".to_owned(),
        ),
    }
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
