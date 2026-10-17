//! One item-use edge uses a frozen proposal; held-use clocks remain frame-owned.

use super::*;
use crate::ordered_interactions::{Progress, Step};
use bevy::prelude::In;
use client_presentation::local_player::LocalViewPose;
use gameplay::mining::FrozenMiningSelection;

/// Retains one press/release attempt and its inventory revision across backpressure.
#[derive(Default)]
pub(crate) struct AirAttempt {
    initialized: bool,
    finished: bool,
    selection: Option<(FrozenMiningSelection, Option<u64>)>,
    frozen: Option<frame::FrozenAirUse>,
}

/// Attempts a single input edge without advancing another held-use frame.
pub(crate) fn step(
    In((mut attempt, facing)): In<(AirAttempt, (Option<bevy::prelude::Quat>, LocalViewPose))>,
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
    let phase = context.input.phase(Action::Use);
    let allowed = player
        .facts
        .game_mode_capabilities()
        .is_none_or(|caps| caps.can_use_items);
    let held = allowed && phase.held;
    if !attempt.initialized {
        let scope = context.client_world.stream.as_ref().map(|stream| {
            (
                stream.authority().actor_session_id(),
                stream.current_dimension(),
            )
        });
        let delay_fix = scope.is_some()
            && context
                .delay_fix
                .as_ref()
                .is_some_and(|policy| policy.scope == scope);
        runtime.set_delay_fix(delay_fix);
        runtime.observe_selected_slot(player.selected_hotbar_slot());
        runtime.observe_press(allowed && phase.pressed);
        attempt.initialized = true;
    }
    let selection = verified_use_selection(&player, &context.ui);
    let revision = selection.as_ref().and_then(|selection| {
        context
            .ui
            .inventory_ledger(&player)
            .authoritative_slot_revision(selection.slot)
    });
    if let Some(expected) = &attempt.selection {
        if selection.as_ref() != Some(&expected.0) || revision != expected.1 {
            runtime.cancel_pending_input();
            return Step::new(attempt, Progress::Complete);
        }
    } else if let Some(selection) = selection {
        attempt.selection = Some((selection, revision));
    } else if phase.pressed && allowed {
        return Step::new(attempt, Progress::Pending);
    }
    if !runtime.has_work(held) {
        return Step::new(attempt, Progress::Complete);
    }
    if attempt.frozen.is_none() {
        let scope = context.client_world.stream.as_ref().map(|stream| {
            (
                stream.authority().actor_session_id(),
                stream.current_dimension(),
            )
        });
        let release_facing = runtime
            .riptide_release_facing()
            .unwrap_or_else(|| riptide::release_facing(facing.0, &facing.1));
        let policy = frame::FramePolicy {
            held,
            before_flush: release_facing.assisted && runtime.is_releasing(held),
            delay_fix_enabled: scope.is_some()
                && context
                    .delay_fix
                    .as_ref()
                    .is_some_and(|policy| policy.scope == scope),
            aim_rotation: facing.0,
            release_facing,
        };
        attempt.frozen = frame::observe(
            &player,
            &context,
            &mut runtime,
            &movement,
            &mut view,
            policy,
        );
    }
    let Some(frozen) = attempt.frozen.as_ref() else {
        return Step::new(attempt, Progress::Pending);
    };
    frame::admit(
        &mut player,
        &mut context,
        &mut runtime,
        &mut movement,
        &mut swings,
        &mut view,
        frozen,
    );
    let progress = if runtime.pending_edge() {
        Progress::Pending
    } else if movement.has_held_release() {
        attempt.finished = true;
        Progress::Carrier
    } else {
        Progress::Complete
    };
    Step::new(attempt, progress)
}

/// Delivers an already-admitted release carrier before the next mutating event.
pub(crate) fn flush_release(
    mut movement: ResMut<MovementTicker>,
    network: Res<NetworkHandle<PackApplication>>,
) {
    movement.send_held_release(|packets| network.send_inventory_packets(packets));
}
