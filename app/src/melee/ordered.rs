//! Per-press attack proposals retain published picks and retry ownership.

use super::*;
use crate::ordered_interactions::{Progress, Step};
use bevy::prelude::In;
use gameplay::melee::{Crosshair, ItemAttackPress};

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

/// Rebuilt target, reach, actor lifetime and terrain evidence for one admission.
#[derive(PartialEq)]
struct AttackInputs {
    target: crosshair::ResolvedTarget,
    lifetime: Option<actor_animation::ActorLifetimeId>,
    world: Option<sim::WorldCollisionIdentity>,
}

impl AttackInputs {
    /// Resolves the original published geometry against current policy and world authority.
    fn observe(
        player: &client_state::PlayerRuntime,
        context: &MeleeContext,
        source: &CapturedAttack,
        input: &crate::ordered_interactions::InteractionInput,
    ) -> Option<Self> {
        let (target, world) = crosshair::resolve_captured(player, context, &source.inputs, input)?;
        let lifetime = match target.crosshair {
            Crosshair::Actor(hit) => Some(
                context
                    .client_world
                    .stream
                    .as_ref()?
                    .authority()
                    .actor_lifetime(hit.runtime_id)?,
            ),
            _ => None,
        };
        Some(Self {
            target,
            lifetime,
            world,
        })
    }
}

/// One press has one selected descriptor and one first-attempt pose even if the queue is full.
pub(crate) struct AttackAttempt {
    source: CapturedAttack,
    picked: Option<AttackInputs>,
    initialized: bool,
    selection: Option<client_interaction::mining::SelectionObservation>,
    press: Option<PressContext>,
    destroy: Option<crate::survival_mining::DestroyObservation>,
    fence: Option<gameplay::movement::InteractionFence>,
}

impl AttackAttempt {
    /// Starts an unattempted press from its original published pick.
    pub(crate) fn new(source: CapturedAttack) -> Self {
        Self {
            source,
            picked: None,
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
    if !attempt.initialized || runtime.press_pending() {
        if hand_interaction_selection(&player).is_none() {
            return Step::new(attempt, Progress::Pending);
        }
        let current = AttackInputs::observe(&player, &context, &attempt.source, &input);
        if let Some(original) = &attempt.picked
            && (current.as_ref() != Some(original)
                || (attempt.press.is_some() && original.world.is_none()))
        {
            runtime.cancel();
            return Step::new(attempt, Progress::Complete);
        }
        attempt.picked = current;
    }
    let Some(mut picked) = attempt.picked.as_ref().map(|inputs| inputs.target) else {
        return Step::new(attempt, Progress::Pending);
    };
    if !attempt.initialized {
        runtime.synchronize(movement.interaction_authority_identity());
        runtime.observe_input(true, false);
        attempt.initialized = true;
    }
    let now = u64::try_from(context.time.elapsed().as_millis()).unwrap_or(u64::MAX);
    let caps = player.facts.game_mode_capabilities();
    let focused =
        !context.focus.menu_visible && context.windows.single().is_ok_and(|window| window.focused);
    let mining_active = gameplay::survival_mining::mining_active(caps, focused, true);
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
        });
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
    if runtime.press_pending() {
        if attempt.destroy.as_ref().is_some_and(|observation| {
            !crate::interaction_authority::world_identity_current(
                &context.client_world,
                &context.collisions,
                &observation.world,
            )
        }) {
            runtime.cancel();
            return Step::new(attempt, Progress::Complete);
        }
        let timing = selected_attack_timing(&player, &context.client_world);
        picked.crosshair = runtime.observe_attack_target(
            picked.crosshair,
            timing.is_some_and(|timing| timing.piercing_weapon),
        );
        attempt.destroy = (picked.crosshair == Crosshair::Block && mining_active)
            .then(|| {
                crate::survival_mining::observe_destroy_target(
                    &player,
                    &context,
                    &input.context.origin,
                    caps,
                    input.context.mode,
                    input.context.identity,
                    movement.interaction_authority_identity().1,
                )
            })
            .flatten();
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
            mining_target: attempt
                .destroy
                .as_ref()
                .map(|observation| &observation.target),
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
