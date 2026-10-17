//! Per-edge build admission; held repeats remain in the frame maintenance system.

use super::*;
use crate::ordered_interactions::{Progress, Step};
use bevy::prelude::In;
use gameplay::mining::FrozenMiningSelection;

/// One use edge retains its selected stack and resolved proposal across Full.
#[derive(Default)]
pub(crate) struct BlockAttempt {
    initialized: bool,
    cancelled: bool,
    selection: Option<(FrozenMiningSelection, Option<u64>)>,
    proposal: Option<Proposal>,
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
    In((mut attempt, pick)): In<(BlockAttempt, Option<FramePick>)>,
    mut player: bevy::prelude::ResMut<client_state::PlayerRuntime>,
    mut context: BlockUseContext,
    mut runtime: ResMut<BlockUseRuntime>,
    mut swings: ResMut<SwingTracker>,
    mut item_use: ResMut<client_interaction::item_use::ItemUseRuntime>,
    movement: Res<MovementTicker>,
) -> Step<BlockAttempt> {
    let phase = context.input.phase(Action::Use);
    let attacking = context.input.phase(Action::Attack).held;
    let now = u64::try_from(context.time.elapsed().as_millis()).unwrap_or(u64::MAX);
    runtime.synchronize(movement.interaction_authority_identity());
    if !attempt.initialized {
        if runtime.stopping() || runtime.last_success_destination().is_some() {
            if !stop_block_use(&mut runtime, &context, phase.pressed && !attacking) {
                return Step::new(attempt, Progress::Pending);
            }
        }
        if !phase.pressed || attacking || context.melee.blocks_use_at(now) {
            runtime.clear_press();
            return Step::new(attempt, Progress::Complete);
        }
        runtime.forget_press_resolution();
        runtime.observe_use(
            phase.held,
            true,
            false,
            movement.accepts_block_interactions(),
        );
        attempt.initialized = true;
    }
    if !movement.accepts_block_interactions()
        || movement.has_held_release()
        || movement.between_ticks_sample().is_none()
    {
        return Step::new(attempt, Progress::Pending);
    }
    let Some(selection) = verified_use_selection(&player, &context.ui) else {
        return Step::new(attempt, Progress::Pending);
    };
    let revision = context
        .ui
        .inventory_ledger(&player)
        .authoritative_slot_revision(selection.slot);
    if let Some(expected) = &attempt.selection {
        if expected != &(selection.clone(), revision) {
            attempt.cancelled = true;
            runtime.clear_press();
            return Step::new(attempt, Progress::Complete);
        }
    } else {
        runtime.selection_changed(&selection);
        attempt.selection = Some((selection.clone(), revision));
    }
    if attempt.proposal.is_none() {
        let (Some(state), Some(pick), Some(input)) = (
            movement.build_action_state(),
            pick,
            context.input.snapshot(),
        ) else {
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
        let mode = protocol_input_mode(input.input_mode);
        if let Some(actor) = actor_use::prepare(
            &context, &selection, game_mode, pick, &state, mode, clock.now, clock,
        ) {
            attempt.proposal = Some(Proposal::Actor(actor));
        } else if let Some(observed) = observe_use_target(
            &player,
            &context,
            mode,
            clock.survival,
            (input.authority_generation, input.frame_sequence),
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
