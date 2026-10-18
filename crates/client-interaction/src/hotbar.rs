//! Samples semantic hotbar selection before transport admission.
use bevy::{
    input::mouse::AccumulatedMouseScroll,
    prelude::{Res, ResMut},
};
use client_state::{PlayerRuntime, ScreenFocus, SemanticInputSnapshot};
use protocol::HOTBAR_SLOT_COUNT;
use semantic_input::{Action, HOTBAR_DIGIT_ACTIONS};

/// Resolves one selection action using the same slot identities as semantic hotbar input.
pub fn slot_for_action(action: Action, current: u8) -> Option<u8> {
    if let Some(index) = HOTBAR_DIGIT_ACTIONS
        .iter()
        .position(|candidate| *candidate == action)
    {
        return u8::try_from(index).ok();
    }
    let delta = cycle_delta(action)?;
    Some((i32::from(current) + delta).rem_euclid(i32::from(HOTBAR_SLOT_COUNT)) as u8)
}

/// Queues number-key, controller and mouse-wheel selection using the current screen authority.
pub fn queue_hotbar_selection(
    mut player_runtime: ResMut<PlayerRuntime>,
    input: Res<SemanticInputSnapshot>,
    scroll: Res<AccumulatedMouseScroll>,
    focus: Res<ScreenFocus>,
) {
    if input
        .snapshot()
        .is_some_and(|snapshot| snapshot.ordered.is_some())
    {
        return;
    }
    // Direct number-key selection. The router only resolves Hotbar1..9 in the Gameplay context,
    // so digits typed while chat is focused never reach this snapshot.
    let mut requested: Option<u8> = None;
    for &action in HOTBAR_DIGIT_ACTIONS {
        if input.phase(action).pressed {
            requested = slot_for_action(action, 0);
        }
    }

    // Controller actions are router-gated; raw wheel input needs the same screen ownership.
    let mut cycle: i32 = 0;
    if input.phase(Action::HotbarNext).pressed {
        cycle += cycle_delta(Action::HotbarNext).expect("cycle action");
    }
    if input.phase(Action::HotbarPrevious).pressed {
        cycle += cycle_delta(Action::HotbarPrevious).expect("cycle action");
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

/// Supplies one direction policy to both frame and ordered selection consumers.
fn cycle_delta(action: Action) -> Option<i32> {
    match action {
        Action::HotbarNext => Some(1),
        Action::HotbarPrevious => Some(-1),
        _ => None,
    }
}
