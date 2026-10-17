//! Per-edge build admission; held repeats remain in the frame maintenance system.

use super::*;
use crate::ordered_interactions::{Progress, Step};
use bevy::prelude::In;

/// One use edge retains its selected stack and resolved proposal across Full.
#[derive(Default)]
pub(crate) struct BlockAttempt {
    initialized: bool,
    cancelled: bool,
    selection: Option<client_interaction::mining::SelectionObservation>,
    proposal: Option<Proposal>,
    fence: Option<gameplay::movement::InteractionFence>,
}

impl BlockAttempt {
    /// Reports an invalidated proposal whose air-use suffix must not be retargeted.
    pub(crate) fn cancelled(&self) -> bool {
        self.cancelled
    }
}

enum Proposal {
    Actor(actor_use::ActorUseAttempt),
    Block(build::FrozenBuild),
}

/// Attempts only this edge, returning the same proposal when transport or evidence must wait.
pub(crate) fn step(
    In((mut attempt, (pick, input))): In<(
        BlockAttempt,
        (
            Option<FramePick>,
            crate::ordered_interactions::InteractionInput,
        ),
    )>,
    mut player: bevy::prelude::ResMut<client_state::PlayerRuntime>,
    mut context: BlockUseContext,
    mut runtime: ResMut<BlockUseRuntime>,
    mut swings: ResMut<SwingTracker>,
    mut item_use: ResMut<client_interaction::item_use::ItemUseRuntime>,
    mut movement: ResMut<MovementTicker>,
) -> Step<BlockAttempt> {
    let pressed = input.edge.pressed;
    let held = input.edge.buttons.held(Action::Use);
    let attacking = input.edge.buttons.held(Action::Attack);
    let now = u64::try_from(context.time.elapsed().as_millis()).unwrap_or(u64::MAX);
    runtime.synchronize(movement.interaction_authority_identity());
    if !attempt.initialized {
        if runtime.stopping() || runtime.last_success_destination().is_some() {
            if !stop_block_use(&mut runtime, &context, pressed && !attacking) {
                return Step::new(attempt, Progress::Pending);
            }
        }
        if !pressed || attacking || context.melee.blocks_use_at(now) {
            runtime.clear_press();
            return Step::new(attempt, Progress::Complete);
        }
        runtime.forget_press_resolution();
        runtime.observe_use(held, true, false, movement.accepts_block_interactions());
        attempt.initialized = true;
    }
    if !movement.accepts_block_interactions()
        || movement.has_held_release()
        || attempt.fence.as_ref().map_or_else(
            || movement.between_ticks_sample().is_none(),
            |fence| !movement.interaction_tick_available(fence.before_tick()),
        )
    {
        return Step::new(attempt, Progress::Pending);
    }
    if let Some(selection) = &attempt.selection {
        if !selection.current(&player, &context.ui) {
            attempt.cancelled = true;
            runtime.clear_press();
            return Step::new(attempt, Progress::Complete);
        }
    } else {
        attempt.selection =
            client_interaction::mining::SelectionObservation::for_use(&player, &context.ui);
    }
    let Some(observation) = attempt.selection.as_ref() else {
        return Step::new(attempt, Progress::Pending);
    };
    let selection = observation.selection.clone();
    runtime.selection_changed(&selection);
    if attempt.proposal.is_none() {
        let (Some(state), Some(pick)) = (movement.build_action_state(), pick) else {
            return Step::new(attempt, Progress::Pending);
        };
        let game_mode = player.facts.player_game_mode();
        let Some(caps) = player.facts.game_mode_capabilities() else {
            runtime.clear_press();
            return Step::new(attempt, Progress::Complete);
        };
        if !caps.can_use_blocks() {
            runtime.clear_press();
            return Step::new(attempt, Progress::Complete);
        }
        let clock = RepeatClock::for_state(context.time.elapsed(), &state, game_mode);
        let mode = protocol_input_mode(input.mode);
        if let Some(actor) = actor_use::prepare(
            &context,
            &input.origin,
            &selection,
            game_mode,
            pick,
            &state,
            mode,
            clock.now,
            clock,
        ) {
            attempt.proposal = Some(Proposal::Actor(actor));
        } else if let Some(observed) = observe_use_target(
            &player,
            &context,
            &input.origin,
            mode,
            clock.survival,
            input.identity,
            movement.interaction_authority_identity().1,
            &runtime,
            Some(pick),
            ItemUseTrigger::PlayerInput,
            &state,
        ) {
            attempt.proposal = Some(Proposal::Block(build::FrozenBuild::new(
                observed,
                state,
                ItemUseTrigger::PlayerInput,
                clock.now,
                clock,
                input.view,
            )));
        } else {
            runtime.record(
                ItemUseTrigger::PlayerInput,
                clock.now,
                state.tick.saturating_add(1),
                LocalUse::Nothing,
                clock,
            );
            return Step::new(attempt, Progress::Complete);
        }
    }
    if let Some(Proposal::Block(build)) = &attempt.proposal {
        if !crate::interaction_authority::world_identity_current(
            &context.client_world,
            &context.collisions,
            &build.observation().target.identity,
        ) {
            attempt.cancelled = true;
            runtime.clear_press();
            return Step::new(attempt, Progress::Complete);
        }
    }
    if attempt.fence.is_none() {
        // Both proposal kinds were prepared from the same pre-tick movement state.
        let tick = movement
            .between_ticks_sample()
            .expect("new proposal requires admitted older movement")
            .tick;
        attempt.fence = Some(movement.reserve_interaction_before(tick));
    }
    let done = match attempt.proposal.as_mut().expect("prepared build proposal") {
        Proposal::Actor(actor) => {
            attempt.cancelled = !actor.current(&context);
            actor_use::advance(&context, &mut runtime, actor)
        }
        Proposal::Block(build) => build::advance(
            &mut player,
            &mut context,
            &mut runtime,
            &mut item_use,
            &mut swings,
            &movement,
            build,
        ),
    };
    if done {
        attempt.fence = None;
    }
    Step::new(
        attempt,
        if done {
            Progress::Complete
        } else {
            Progress::Pending
        },
    )
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
