//! Local hotbar slot selection.
//!
//! Bedrock owns hotbar-slot selection on the client: number keys, the mouse wheel, and the
//! controller cycle buttons change the held slot immediately (predicted locally so the HUD
//! highlight follows input without waiting for the server), and the choice is announced upstream
//! with a `PlayerHotbar` packet.

use bevy::{
    input::mouse::AccumulatedMouseScroll,
    prelude::{Res, ResMut},
};
use protocol::{HOTBAR_SLOT_COUNT, select_hotbar_slot_packet};
use semantic_input::Action;

use crate::{
    runtime::{
        network::{NetworkHandle, PacketSendError},
        shutdown::record_fatal_error,
        world::ClientWorld,
    },
    semantic_controls::SemanticInputSnapshot,
    ui_runtime::UiRuntime,
};

const HOTBAR_DIGIT_ACTIONS: [Action; 9] = [
    Action::Hotbar1,
    Action::Hotbar2,
    Action::Hotbar3,
    Action::Hotbar4,
    Action::Hotbar5,
    Action::Hotbar6,
    Action::Hotbar7,
    Action::Hotbar8,
    Action::Hotbar9,
];

/// Applies number-key, mouse-wheel, and controller hotbar selection to the local prediction and
/// notifies the server. Runs after semantic input is finalized and before UI publication.
pub(crate) fn select_hotbar_slot(
    input: Res<SemanticInputSnapshot>,
    scroll: Res<AccumulatedMouseScroll>,
    mut runtime: ResMut<UiRuntime>,
    network: Res<NetworkHandle>,
    mut client_world: ResMut<ClientWorld>,
) {
    // Direct number-key selection. The router only resolves Hotbar1..9 in the Gameplay context,
    // so digits typed while chat is focused never reach this snapshot.
    let mut requested: Option<u8> = None;
    for (index, action) in HOTBAR_DIGIT_ACTIONS.iter().enumerate() {
        if input.phase(*action).pressed {
            requested = Some(index as u8);
        }
    }

    // Relative cycling: controller buttons (router-gated) and the mouse wheel (gated on chat
    // focus here, since the wheel is read directly rather than through the router).
    let mut cycle: i32 = 0;
    if input.phase(Action::HotbarNext).pressed {
        cycle += 1;
    }
    if input.phase(Action::HotbarPrevious).pressed {
        cycle -= 1;
    }
    if !runtime.chat_focused() {
        // One slot per scroll frame. Scroll up selects the previous slot, scroll down the next
        // (matches vanilla). The wheel is read directly, so it is gated on chat focus here.
        if scroll.delta.y > 0.0 {
            cycle -= 1;
        } else if scroll.delta.y < 0.0 {
            cycle += 1;
        }
    }

    if requested.is_none() && cycle != 0 {
        let current = i32::from(
            runtime
                .selected_hotbar_slot()
                .unwrap_or(0)
                .min(HOTBAR_SLOT_COUNT - 1),
        );
        let slots = i32::from(HOTBAR_SLOT_COUNT);
        requested = Some((((current + cycle) % slots + slots) % slots) as u8);
    }

    let Some(target) = requested else {
        return;
    };

    if runtime.selected_hotbar_slot() == Some(target) {
        // The highlight is already on this slot; keep the local prediction sticky but skip a
        // redundant network packet.
        runtime.set_local_selected_slot(target);
        return;
    }
    runtime.set_local_selected_slot(target);

    // Notify the server with the vanilla held-slot packet (MobEquipment). It must address the
    // local player by its StartGame runtime id; before that is known we predict locally only.
    let Some(runtime_id) = runtime.local_runtime_id() else {
        return;
    };
    match network.send_packet(select_hotbar_slot_packet(runtime_id, target)) {
        // A dropped selection under backpressure is tolerable: the local prediction still moved
        // the highlight, and the next selection supersedes it.
        Ok(()) | Err(PacketSendError::Full(_)) => {}
        Err(PacketSendError::Closed(_)) => record_fatal_error(
            &mut client_world.fatal_error,
            "hotbar selection send failed because the network command channel closed".to_owned(),
        ),
    }
}
