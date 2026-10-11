//! Samples semantic hotbar selection before transport admission.
use bevy::{
    input::mouse::AccumulatedMouseScroll,
    prelude::{Res, ResMut},
};
use client_state::{PlayerRuntime, ScreenFocus, SemanticInputSnapshot};
use protocol::HOTBAR_SLOT_COUNT;
use semantic_input::Action;

pub const HOTBAR_DIGIT_ACTIONS: [Action; HOTBAR_SLOT_COUNT as usize] = [
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

/// Queues number-key, controller and mouse-wheel selection using the current screen authority.
pub fn queue_hotbar_selection(
    mut player_runtime: ResMut<PlayerRuntime>,
    input: Res<SemanticInputSnapshot>,
    scroll: Res<AccumulatedMouseScroll>,
    focus: Res<ScreenFocus>,
) {
    // Direct number-key selection. The router only resolves Hotbar1..9 in the Gameplay context,
    // so digits typed while chat is focused never reach this snapshot.
    let mut requested: Option<u8> = None;
    for (index, action) in HOTBAR_DIGIT_ACTIONS.iter().enumerate() {
        if input.phase(*action).pressed {
            requested = Some(index as u8);
        }
    }

    // Controller actions are router-gated; raw wheel input needs the same screen ownership.
    let mut cycle: i32 = 0;
    if input.phase(Action::HotbarNext).pressed {
        cycle += 1;
    }
    if input.phase(Action::HotbarPrevious).pressed {
        cycle -= 1;
    }
    if !focus.absorbs_input {
        // One slot per frame: scrolling up selects the previous slot.
        if scroll.delta.y > 0.0 {
            cycle -= 1;
        } else if scroll.delta.y < 0.0 {
            cycle += 1;
        }
    }

    if requested.is_none() && cycle != 0 {
        let current = i32::from(
            player_runtime
                .inventory
                .selected_hotbar_slot(player_runtime.facts.player_game_mode())
                .unwrap_or(0)
                .min(HOTBAR_SLOT_COUNT - 1),
        );
        let slots = i32::from(HOTBAR_SLOT_COUNT);
        requested = Some((((current + cycle) % slots + slots) % slots) as u8);
    }

    if let Some(target) = requested {
        let game_mode = player_runtime.facts.player_game_mode();
        player_runtime
            .inventory
            .queue_local_hotbar_selection(target, game_mode);
    }
}
