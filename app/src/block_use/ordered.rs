//! Per-edge build admission; held repeats remain in the frame maintenance system.

use super::*;
use bevy::prelude::In;

/// Attempts this edge once, returning whether item use may resolve its air-use suffix.
pub(crate) fn press(
    In(input): In<crate::ordered_interactions::InteractionInput>,
    mut player: ResMut<client_state::PlayerRuntime>,
    mut context: BlockUseContext,
    mut runtime: ResMut<BlockUseRuntime>,
    mut swings: ResMut<SwingTracker>,
    mut item_use: ResMut<client_interaction::item_use::ItemUseRuntime>,
    movement: Res<MovementTicker>,
) -> bool {
    let pressed = input.edge.pressed;
    let attacking = input.edge.buttons.held(Action::Attack);
    let now = u64::try_from(context.time.elapsed().as_millis()).unwrap_or(u64::MAX);
    runtime.synchronize(movement.interaction_authority_identity());
    if pressed {
        runtime.forget_press_resolution();
    }
    let resolved = (|| {
        if (runtime.stopping() || runtime.last_success_destination().is_some())
            && !stop_block_use(&mut runtime, &context, pressed && !attacking)
        {
            return false;
        }
        if !pressed || attacking || context.melee.blocks_use_at(now) {
            return true;
        }
        if !movement.accepts_block_interactions()
            || movement.has_held_release()
            || movement.between_ticks_sample().is_none()
        {
            return false;
        }
        let Some(selection) = verified_use_selection(&player, &context.ui) else {
            return false;
        };
        runtime.selection_changed(&selection);
        runtime.observe_use(input.edge.buttons.held(Action::Use), true, false, true);
        let (Some(state), Some(pick)) = (
            movement.build_action_state(),
            runtime.pick(movement.interaction_authority_identity()),
        ) else {
            return false;
        };
        let game_mode = player.facts.player_game_mode();
        let Some(caps) = player.facts.game_mode_capabilities() else {
            return false;
        };
        if !caps.can_use_blocks() {
            return true;
        }
        let clock = RepeatClock::for_state(context.time.elapsed(), &state, game_mode);
        let mode = protocol_input_mode(input.context.mode);
        if let Some(mut actor) = actor_use::prepare(
            &context,
            &input.context.origin,
            &selection,
            &player,
            pick,
            &state,
            mode,
            clock.now,
            clock,
        ) {
            return actor_use::advance(&context, &mut runtime, &mut actor);
        }
        let Some(observed) = observe_use_target(
            &player,
            &context,
            &input.context.origin,
            mode,
            clock.survival,
            input.context.identity,
            movement.interaction_authority_identity().1,
            &runtime,
            Some(pick),
            ItemUseTrigger::PlayerInput,
            &state,
        ) else {
            runtime.record(
                ItemUseTrigger::PlayerInput,
                clock.now,
                state.tick.saturating_add(1),
                LocalUse::Nothing,
                clock,
            );
            return true;
        };
        let mut build = build::BuildInput::new(
            observed,
            state,
            ItemUseTrigger::PlayerInput,
            clock.now,
            clock,
            input.context.view,
        );
        build::advance(
            &mut player,
            &mut context,
            &mut runtime,
            &mut item_use,
            &mut swings,
            &movement,
            &mut build,
        )
    })();
    runtime.clear_press();
    resolved
}

/// Keeps an accepted block stop before the next action, including an attacking interruption.
pub(crate) fn fence(
    In(attack): In<bool>,
    mut runtime: ResMut<BlockUseRuntime>,
    context: BlockUseContext,
) -> bool {
    if runtime.stopping() || (attack && runtime.last_success_destination().is_some()) {
        stop_block_use(&mut runtime, &context, false)
    } else {
        true
    }
}
