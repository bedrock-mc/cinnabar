//! One selection's descriptor remains stable until its transport handoff succeeds.

use super::*;
use crate::ordered_interactions::{Progress, Step};
use bevy::prelude::In;

/// A selection packet and target retained behind the shared interaction cursor.
#[derive(Default)]
pub(crate) struct SelectionAttempt {
    target: Option<u8>,
    proposal: Option<inventory::PreparedHotbarSelection>,
}

/// Admits one selection before publishing its prediction to later ordered actions.
pub(crate) fn step(
    In((mut attempt, action)): In<(SelectionAttempt, semantic_input::Action)>,
    mut player: ResMut<client_state::PlayerRuntime>,
    network: Res<NetworkHandle<PackApplication>>,
    mut fatal: ResMut<FatalError>,
    mut item_use: ResMut<client_interaction::item_use::ItemUseRuntime>,
) -> Step<SelectionAttempt> {
    if attempt.target.is_none() {
        let current = player.selected_hotbar_slot().unwrap_or(0);
        let Some(target) = client_interaction::hotbar::slot_for_action(action, current) else {
            return Step::new(attempt, Progress::Complete);
        };
        if current == target {
            return Step::new(attempt, Progress::Complete);
        }
        attempt.target = Some(target);
    }
    if attempt.proposal.is_none() {
        match player
            .inventory
            .prepare_hotbar_selection(attempt.target.expect("selection target"))
        {
            Ok(Some(proposal)) => {
                attempt.proposal = Some(proposal);
            }
            Ok(None) => return Step::new(attempt, Progress::Pending),
            Err(error) => {
                record_fatal_error(
                    &mut fatal.0,
                    format!("hotbar selection packet validation failed: {error}"),
                );
                return Step::new(attempt, Progress::Complete);
            }
        }
    }
    let proposal = attempt
        .proposal
        .as_ref()
        .expect("prepared selection proposal");
    if !player.inventory.hotbar_selection_current(proposal) {
        return Step::new(attempt, Progress::Complete);
    }
    let packet = proposal.packet().clone();
    let progress = match network.send_hotbar_packet(packet) {
        Ok(()) => {
            let target = attempt.target.expect("selection target");
            player.inventory.set_local_selected_slot(target);
            item_use.retire_reselected_use(target);
            Progress::Complete
        }
        Err(PacketSendError::Full(_)) => Progress::Pending,
        Err(PacketSendError::Closed(_)) if network.closed_command_has_pending_control() => {
            Progress::Pending
        }
        Err(PacketSendError::Closed(_)) => {
            record_fatal_error(
                &mut fatal.0,
                "hotbar selection send failed because the network command channel closed"
                    .to_owned(),
            );
            Progress::Complete
        }
    };
    Step::new(attempt, progress)
}

/// Finishes a pre-existing selection before an ordered press attempts to capture its item.
pub(crate) fn flush_existing(
    mut player: ResMut<client_state::PlayerRuntime>,
    network: Res<NetworkHandle<PackApplication>>,
    mut fatal: ResMut<FatalError>,
) -> bool {
    if network.closed_command_has_pending_control() {
        return false;
    }
    flush_pending_hotbar_selection(&mut player, &mut fatal.0, |packet| {
        network.send_hotbar_packet(packet)
    });
    player.inventory.pending_hotbar_selection().is_none()
}
