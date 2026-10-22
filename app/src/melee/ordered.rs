//! Immediate ordered attack admission from the current published geometry.

use super::*;
use bevy::prelude::In;
use gameplay::melee::{Crosshair, ItemAttackPress};

/// Captures a batch's actor geometry at its early or post-camera dispatch boundary.
pub(crate) fn capture(
    In(phase): In<crate::ordered_interactions::DispatchPhase>,
    context: MeleeContext,
    mut target: ResMut<MeleeTarget>,
    movement: Res<MovementTicker>,
) -> crosshair::CapturedInputs {
    let position = movement.position_authority_identity().1;
    if phase == crate::ordered_interactions::DispatchPhase::AfterMovement {
        target.frames.capture_current(&context, position)
    } else {
        crosshair::capture_ordered_inputs(
            &target.frames,
            position,
            context.input.snapshot().is_some_and(|input| {
                target.prepared
                    == Some((input.authority_generation, input.frame_sequence, position))
            }),
        )
    }
}

/// Attempts this frame's press once, leaving accepted mining with its gameplay owner.
#[allow(
    clippy::too_many_arguments,
    reason = "Gameplay owners are borrowed separately by ECS."
)]
pub(crate) fn press(
    In(input): In<crate::ordered_interactions::InteractionInput>,
    mut player: ResMut<client_state::PlayerRuntime>,
    mut context: MeleeContext,
    mut runtime: ResMut<MeleeRuntime>,
    mut mining: ResMut<client_interaction::survival_mining::SurvivalMiningRuntime>,
    mut swings: ResMut<SwingTracker>,
    mut movement: ResMut<MovementTicker>,
    mut view: ResMut<client_presentation::local_player::LocalViewPose>,
) {
    let Some(selection) = hand_interaction_selection(&player) else {
        return;
    };
    let position = movement.position_authority_identity().1;
    let Some((mut picked, _)) =
        crosshair::resolve_captured(&player, &context, &input.context.attack, &input)
    else {
        return;
    };
    runtime.synchronize(movement.interaction_authority_identity());
    runtime.observe_input(true, false);
    let now = u64::try_from(context.time.elapsed().as_millis()).unwrap_or(u64::MAX);
    let timing = selected_attack_timing(&player, &context.client_world);
    picked.crosshair = runtime.observe_attack_target(
        picked.crosshair,
        timing.is_some_and(|timing| timing.piercing_weapon),
    );
    let sample = runtime.between_ticks_press(&movement).or_else(|| {
        runtime.press_sample(
            picked.crosshair,
            &movement,
            context.effects.recent_tick_count(),
            now,
        )
    });
    let Some((sample, stream)) = sample.zip(context.client_world.stream.as_ref()) else {
        runtime.cancel_press();
        return;
    };
    let mut press = PressContext {
        refused_destroy: None,
        tick: sample.tick,
        player_position: sample.position,
        input_mode: protocol_input_mode(input.context.mode),
        local_runtime_id: stream.local_player_runtime_id(),
        selection: Some(selection),
        swing_duration: timing
            .and_then(|timing| timing.swing_duration_ticks)
            .map(|ticks| i32::try_from(ticks).unwrap_or(i32::MAX).max(1))
            .unwrap_or_else(|| {
                swing_duration(
                    context
                        .effects
                        .mining_tick(sample.tick, movement.completed_tick())
                        .0,
                )
            }),
        now_millis: now,
        item_attack: timing
            .filter(|timing| timing.piercing_weapon)
            .map(|timing| ItemAttackPress {
                direction: picked.direction,
                cooldown: timing.attack_cooldown.clone(),
            }),
    };
    let caps = player.facts.game_mode_capabilities();
    let focused =
        !context.focus.menu_visible && context.windows.single().is_ok_and(|window| window.focused);
    let mining_active = gameplay::survival_mining::mining_active(caps, focused, true);
    // A block press is refused at its start even where the game mode cannot mine.
    let observation = (picked.crosshair == Crosshair::Block && focused)
        .then(|| {
            crate::survival_mining::observe_destroy_target(
                &player,
                &context,
                &input.context.origin,
                caps,
                input.context.mode,
                input.context.identity,
                position,
            )
        })
        .flatten();
    let destroy = match observation {
        Some(crate::survival_mining::DestroyObservation::Refused(refused)) => {
            press.refused_destroy = Some(refused);
            None
        }
        Some(crate::survival_mining::DestroyObservation::Target(target)) if mining_active => {
            Some(target)
        }
        _ => None,
    };
    let authority = player
        .facts
        .server_authoritative_block_breaking()
        .map(gameplay::survival_mining::BlockBreakingAuthority::from_negotiation)
        .unwrap_or(gameplay::survival_mining::BlockBreakingAuthority::Server);
    let completed = movement.completed_tick();
    let effects = &context.effects;
    let network = &context.network;
    let world = &mut context.client_world;
    let admitted = gameplay::melee::admit_attack_press(
        &mut runtime,
        &mut mining,
        &mut swings,
        &mut movement,
        gameplay::melee::AttackFrame {
            attack: Some((picked.crosshair, press.clone())),
            mining_target: destroy.as_ref(),
            allow_mining_start: true,
            attack_held: input.edge.buttons.held(Action::Attack),
            authority,
            now_millis: now,
            local_runtime_id: press.local_runtime_id,
        },
        gameplay::melee::AttackEffects {
            send: |packets| network.send_inventory_packets(packets),
            swing_duration: |tick| swing_duration(effects.mining_tick(tick, completed).0),
            request_id: |slot, damage| {
                player
                    .inventory
                    .ledger_mut()
                    .begin_mining_request(slot, damage)
            },
            predict_break: |position| {
                if let Some(stream) = world.stream.as_mut() {
                    stream.predict_block(position, 0, stream.air_block_id());
                }
            },
        },
    );
    for request in admitted.cancelled_mining_requests {
        player.inventory.ledger_mut().cancel_mining_request(request);
    }
    if let Some(tick) = admitted.rotation_tick
        && let Some(rotation) = picked.action_rotation
    {
        client_interaction::camera::aim_assist::apply_action_rotation(
            rotation,
            &mut view,
            &mut movement,
            tick,
        );
    }
}
