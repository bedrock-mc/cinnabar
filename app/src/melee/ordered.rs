//! Per-press attack proposals retain published picks and retry ownership.

use super::*;
use crate::ordered_interactions::{Progress, Step};
use bevy::prelude::In;
use gameplay::{
    melee::{Crosshair, ItemAttackPress},
    survival_mining::DestroyTarget,
};

/// Each press shares published geometry while resolving its own target and selected item.
#[derive(Clone)]
pub(crate) struct CapturedAttack {
    inputs: crosshair::CapturedInputs,
}

/// Retains immutable inputs once per batch without resolving a whole burst to one target.
pub(crate) fn capture(
    input: Res<SemanticInputSnapshot>,
    target: Res<MeleeTarget>,
    movement: Res<MovementTicker>,
) -> CapturedAttack {
    let identity = input.snapshot().map(|input| {
        (
            input.authority_generation,
            input.frame_sequence,
            movement.interaction_authority_identity().1,
        )
    });
    CapturedAttack {
        inputs: crosshair::capture_ordered_inputs(
            &target.frames,
            movement.interaction_authority_identity().1,
            target.prepared == identity,
        ),
    }
}

/// One press has one selected descriptor and one first-attempt pose even if the queue is full.
pub(crate) struct AttackAttempt {
    source: CapturedAttack,
    picked: Option<crosshair::ResolvedTarget>,
    lifetime: Option<actor_animation::ActorLifetimeId>,
    initialized: bool,
    selection: Option<client_interaction::mining::SelectionObservation>,
    press: Option<PressContext>,
    destroy: Option<DestroyTarget>,
    fence: Option<gameplay::movement::InteractionFence>,
}

impl AttackAttempt {
    /// Starts an unattempted press from its original published pick.
    pub(crate) fn new(source: CapturedAttack) -> Self {
        Self {
            source,
            picked: None,
            lifetime: None,
            initialized: false,
            selection: None,
            press: None,
            destroy: None,
            fence: None,
        }
    }
}

/// Admits this press or its accepted mining carrier without running held mining again.
#[allow(
    clippy::too_many_arguments,
    reason = "Gameplay owners are borrowed separately by ECS."
)]
pub(crate) fn step(
    In((mut attempt, input)): In<(AttackAttempt, crate::ordered_interactions::InteractionInput)>,
    mut player: ResMut<client_state::PlayerRuntime>,
    mut context: MeleeContext,
    mut runtime: ResMut<MeleeRuntime>,
    mut mining: ResMut<client_interaction::survival_mining::SurvivalMiningRuntime>,
    mut swings: ResMut<SwingTracker>,
    mut movement: ResMut<MovementTicker>,
    mut view: ResMut<client_presentation::local_player::LocalViewPose>,
) -> Step<AttackAttempt> {
    if attempt.picked.is_none() {
        if hand_interaction_selection(&player).is_none() {
            return Step::new(attempt, Progress::Pending);
        }
        attempt.picked =
            crosshair::resolve_captured(&player, &context, &attempt.source.inputs, &input);
        attempt.lifetime = attempt.picked.and_then(|picked| match picked.crosshair {
            Crosshair::Actor(hit) => context
                .client_world
                .stream
                .as_ref()?
                .authority()
                .actor_lifetime(hit.runtime_id),
            _ => None,
        });
    }
    let Some(mut picked) = attempt.picked else {
        return Step::new(attempt, Progress::Pending);
    };
    if let Some(lifetime) = attempt.lifetime {
        let valid = context.client_world.stream.as_ref().is_some_and(|stream| {
            stream.authority().actor_lifetime(lifetime.runtime_id) == Some(lifetime)
                && stream
                    .authority()
                    .actor(lifetime.runtime_id)
                    .is_some_and(|actor| !actor.status.dead)
        });
        if !valid {
            runtime.cancel();
            return Step::new(attempt, Progress::Complete);
        }
    }
    if !attempt.initialized {
        runtime.synchronize(movement.interaction_authority_identity());
        runtime.observe_input(true, false);
        attempt.initialized = true;
    }
    let now = u64::try_from(context.time.elapsed().as_millis()).unwrap_or(u64::MAX);
    if attempt.press.is_none() {
        if let Some(selection) = &attempt.selection {
            if !selection.current(&player, &context.ui) {
                runtime.cancel();
                return Step::new(attempt, Progress::Complete);
            }
        } else {
            attempt.selection = client_interaction::mining::SelectionObservation::hand(&player);
        }
        let Some(observation) = attempt.selection.as_ref() else {
            return Step::new(attempt, Progress::Pending);
        };
        let selection = observation.selection.clone();
        let timing = selected_attack_timing(&player, &context.client_world);
        picked.crosshair = runtime.observe_attack_target(
            picked.crosshair,
            timing.is_some_and(|timing| timing.piercing_weapon),
        );
        attempt.picked = Some(picked);
        let sample = runtime.between_ticks_press(&movement).or_else(|| {
            runtime.press_sample(
                picked.crosshair,
                &movement,
                context.effects.recent_tick_count(),
                now,
            )
        });
        let Some(sample) = sample else {
            return Step::new(attempt, Progress::Pending);
        };
        let Some(stream) = context.client_world.stream.as_ref() else {
            return Step::new(attempt, Progress::Pending);
        };
        attempt.fence = Some(movement.reserve_interaction_before(sample.tick));
        attempt.press = Some(PressContext {
            tick: sample.tick,
            player_position: sample.position,
            input_mode: protocol_input_mode(input.mode),
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
        });
        if picked.crosshair == Crosshair::Block {
            attempt.destroy = crate::survival_mining::observe_destroy_target(
                &player,
                &context,
                &input.origin,
                player.facts.game_mode_capabilities(),
                input.mode,
                input.identity,
                movement.interaction_authority_identity().1,
            );
        }
    } else if runtime.press_pending()
        && !attempt
            .selection
            .as_ref()
            .expect("captured press selection")
            .current(&player, &context.ui)
    {
        runtime.cancel();
        return Step::new(attempt, Progress::Complete);
    }
    let authority = player
        .facts
        .server_authoritative_block_breaking()
        .map(gameplay::survival_mining::BlockBreakingAuthority::from_negotiation)
        .unwrap_or(gameplay::survival_mining::BlockBreakingAuthority::Server);
    let press = attempt.press.as_ref().expect("captured attack pose");
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
            mining_target: attempt.destroy.as_ref(),
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
    if !runtime.press_pending() {
        attempt.fence = None;
    }
    let progress = if runtime.press_pending() {
        Progress::Pending
    } else if runtime.has_queued_work() {
        Progress::Carrier
    } else {
        Progress::Complete
    };
    Step::new(attempt, progress)
}
