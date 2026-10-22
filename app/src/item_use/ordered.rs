//! Attempts each item-use edge once; accepted held uses keep their gameplay state.

use super::*;
use bevy::prelude::In;
use client_presentation::local_player::LocalViewPose;

/// Records whether the use button holds an accepted use before any
/// admission can reject this edge, so a rejected press cannot swallow the
/// release of a use the button now owns.
pub(crate) fn observe_button(
    In(input): In<crate::ordered_interactions::InteractionInput>,
    player: Res<client_state::PlayerRuntime>,
    mut runtime: ResMut<ItemUseRuntime>,
) {
    let allowed = player
        .facts
        .game_mode_capabilities()
        .is_none_or(|caps| caps.can_use_items);
    runtime.observe_use_button(allowed && input.edge.buttons.held(Action::Use));
}

/// Resolves one input edge without retaining its pose or unadmitted press.
pub(crate) fn press(
    In(input): In<crate::ordered_interactions::InteractionInput>,
    mut player: ResMut<client_state::PlayerRuntime>,
    mut context: ItemUseContext,
    mut runtime: ResMut<ItemUseRuntime>,
    mut movement: ResMut<MovementTicker>,
    mut swings: ResMut<SwingTracker>,
    mut view: ResMut<LocalViewPose>,
) {
    runtime.synchronize(context.ui.session_id());
    if context.client_world.respawn.input_held() {
        runtime.cancel_for_respawn();
        return;
    }
    swings.sync_ticks_for_item(
        movement.interaction_authority_identity(),
        movement.completed_tick(),
        &context.effects,
        crate::melee::selected_attack_timing(&player, &context.client_world)
            .and_then(|timing| timing.swing_duration_ticks),
    );
    let allowed = player
        .facts
        .game_mode_capabilities()
        .is_none_or(|caps| caps.can_use_items);
    let held = allowed && input.edge.buttons.held(Action::Use);
    runtime.observe_use_button(held);
    let scope = context.client_world.stream.as_ref().map(|stream| {
        (
            stream.authority().actor_session_id(),
            stream.current_dimension(),
        )
    });
    let delay_fix_enabled = scope.is_some()
        && context
            .delay_fix
            .as_ref()
            .is_some_and(|policy| policy.scope == scope);
    runtime.set_delay_fix(delay_fix_enabled);
    runtime.observe_selected_slot(player.selected_hotbar_slot());
    let mut pending = gameplay::item_use::PendingUseInput::new(allowed && input.edge.pressed);
    runtime.with_pending_input(&mut pending, |runtime| {
        if !runtime.has_work(held) {
            return;
        }
        let release_facing = runtime
            .riptide_release_facing()
            .unwrap_or_else(|| riptide::release_facing(input.context.aim, &input.context.view));
        let policy = frame::FramePolicy {
            held,
            before_flush: release_facing.assisted && runtime.is_releasing(held),
            delay_fix_enabled,
            aim_rotation: input.context.aim,
            release_facing,
            sample: None,
        };
        if let Some(proposal) =
            frame::observe(&player, &context, runtime, &movement, &mut view, policy)
        {
            frame::admit(
                &mut player,
                &mut context,
                runtime,
                &mut movement,
                &mut swings,
                &mut view,
                &proposal,
            );
        }
    });
}

/// Delivers an already-admitted release carrier before the next mutating event.
pub(crate) fn flush_release(
    mut movement: ResMut<MovementTicker>,
    network: Res<NetworkHandle<PackApplication>>,
) {
    movement.send_held_release(|packets| network.send_inventory_packets(packets));
}
