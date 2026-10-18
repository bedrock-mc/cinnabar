//! One item-use edge refreshes its admission facts; held-use clocks remain frame-owned.

use super::*;
use crate::ordered_interactions::{Progress, Step};
use bevy::prelude::In;
use client_presentation::local_player::LocalViewPose;

/// Retains one press/release attempt and its inventory revision across backpressure.
#[derive(Default)]
pub(crate) struct AirAttempt {
    pending: Option<gameplay::item_use::PendingUseInput>,
    finished: bool,
    selection: Option<client_interaction::mining::SelectionObservation>,
    sample: Option<gameplay::movement::InteractionSample>,
    fence: Option<gameplay::movement::InteractionFence>,
}

/// Attempts a single input edge without advancing another held-use frame.
pub(crate) fn step(
    In((mut attempt, input)): In<(AirAttempt, crate::ordered_interactions::InteractionInput)>,
    mut player: ResMut<client_state::PlayerRuntime>,
    mut context: ItemUseContext,
    mut runtime: ResMut<ItemUseRuntime>,
    mut movement: ResMut<MovementTicker>,
    mut swings: ResMut<SwingTracker>,
    mut view: ResMut<LocalViewPose>,
) -> Step<AirAttempt> {
    if attempt.finished {
        return Step::new(
            attempt,
            if movement.has_held_release() {
                Progress::Carrier
            } else {
                Progress::Complete
            },
        );
    }
    runtime.synchronize(context.ui.session_id());
    if context.client_world.respawn.input_held() {
        runtime.cancel_for_respawn();
        return Step::new(attempt, Progress::Complete);
    }
    swings.sync_ticks_for_item(
        movement.interaction_authority_identity(),
        movement.completed_tick(),
        &context.effects,
        crate::melee::selected_attack_timing(&player, &context.client_world)
            .and_then(|timing| timing.swing_duration_ticks),
    );
    let pressed = input.edge.pressed;
    let allowed = player
        .facts
        .game_mode_capabilities()
        .is_none_or(|caps| caps.can_use_items);
    let held = allowed && input.edge.buttons.held(Action::Use);
    let scope = context.client_world.stream.as_ref().map(|stream| {
        (
            stream.authority().actor_session_id(),
            stream.current_dimension(),
        )
    });
    runtime.set_delay_fix(
        scope.is_some()
            && context
                .delay_fix
                .as_ref()
                .is_some_and(|policy| policy.scope == scope),
    );
    runtime.observe_selected_slot(player.selected_hotbar_slot());
    let mut pending = attempt
        .pending
        .take()
        .unwrap_or_else(|| gameplay::item_use::PendingUseInput::new(allowed && pressed));
    let mut result = runtime.with_pending_input(&mut pending, |runtime| {
        if let Some(selection) = &attempt.selection {
            if !selection.current(&player, &context.ui) {
                return Step::new(attempt, Progress::Complete);
            }
        } else {
            attempt.selection =
                client_interaction::mining::SelectionObservation::for_use(&player, &context.ui);
            if attempt.selection.is_none() && pressed && allowed {
                return Step::new(attempt, Progress::Pending);
            }
        }
        if !runtime.has_work(held) {
            return Step::new(attempt, Progress::Complete);
        }
        let frozen = {
            let scope = context.client_world.stream.as_ref().map(|stream| {
                (
                    stream.authority().actor_session_id(),
                    stream.current_dimension(),
                )
            });
            let release_facing = runtime
                .riptide_release_facing()
                .unwrap_or_else(|| riptide::release_facing(input.context.aim, &input.context.view));
            let policy = frame::FramePolicy {
                sample: attempt.sample,
                held,
                before_flush: release_facing.assisted && runtime.is_releasing(held),
                delay_fix_enabled: scope.is_some()
                    && context
                        .delay_fix
                        .as_ref()
                        .is_some_and(|policy| policy.scope == scope),
                aim_rotation: input.context.aim,
                release_facing,
            };
            frame::observe(&player, &context, runtime, &movement, &mut view, policy)
        };
        let Some(frozen) = frozen else {
            return Step::new(attempt, Progress::Pending);
        };
        attempt.sample = Some(gameplay::movement::InteractionSample {
            tick: frozen.frame.tick,
            position: frozen.frame.position,
        });
        if attempt.fence.is_none() {
            attempt.fence = Some(movement.reserve_interaction_before(frozen.frame.tick));
        }
        frame::admit(
            &mut player,
            &mut context,
            runtime,
            &mut movement,
            &mut swings,
            &mut view,
            &frozen,
        );
        if !runtime.pending_edge() {
            attempt.fence = None;
        }
        let progress = if runtime.pending_edge() {
            Progress::Pending
        } else if movement.has_held_release() {
            attempt.finished = true;
            Progress::Carrier
        } else {
            Progress::Complete
        };
        Step::new(attempt, progress)
    });
    if result.progress == Progress::Pending {
        result.attempt.pending = Some(pending);
    }
    result
}

/// Delivers an already-admitted release carrier before the next mutating event.
pub(crate) fn flush_release(
    mut movement: ResMut<MovementTicker>,
    network: Res<NetworkHandle<PackApplication>>,
) {
    movement.send_held_release(|packets| network.send_inventory_packets(packets));
}
