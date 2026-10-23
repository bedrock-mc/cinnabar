//! Attack-press actor picking, melee transactions and local arm swings.
//!
//! One attack per press; a held button never re-attacks. Swings obey the
//! half-swing guard shared with survival mining.

use crate::runtime::network::resource_packs::PackApplication;
use bevy::{
    ecs::system::SystemParam,
    prelude::{Query, Real, Res, ResMut, Resource, Time, Window, With},
    window::PrimaryWindow,
};
use client_interaction::melee::{MeleeRuntime, SwingTracker};
use client_runtime::network::NetworkHandle;
use semantic_input::Action;

pub(crate) mod crosshair;
mod hover;
pub(crate) mod ordered;
pub(crate) mod server_swings;
use crosshair::resolve_crosshair;

use crate::interaction_override::ModInteraction;
use client_shell::ui_resources::UiRuntimeResource;
use {
    crate::runtime::world::ClientWorld,
    client_interaction::{
        mining::hand_interaction_selection,
        movement::{LocalMovementEffectTimeline, MovementTicker, PhysicsCollisionRegistries},
    },
    client_presentation::local_player::InteractionOriginSnapshot,
    client_state::SemanticInputSnapshot,
    gameplay::mining::protocol_input_mode,
};

use gameplay::melee::{PressContext, press_admission, swing_duration};

/// Retained frame picks shared by immediate attacks and tick-time hover reports.
#[derive(Resource, Default)]
pub(crate) struct MeleeTarget {
    frames: crosshair::FrameInputs,
    /// Input identity of the last successfully resolved frame.
    prepared: Option<(
        semantic_input::identity::InputAuthorityGeneration,
        semantic_input::identity::InputSequence,
        simulation_clock::identity::PositionAuthorityGeneration,
    )>,
    pub(crate) attack: Option<crosshair::ResolvedTarget>,
    pub(crate) hover: Option<(u64, gameplay::entity_hover::HoverSnapshot)>,
}

impl MeleeTarget {
    /// Reports whether the current published inputs belong to this movement authority.
    pub(crate) fn has_published_inputs(
        &self,
        position: simulation_clock::identity::PositionAuthorityGeneration,
    ) -> bool {
        self.frames.has_current(position)
    }
}

#[derive(SystemParam)]
pub(crate) struct MeleeContext<'w, 's> {
    pub(crate) input: Res<'w, SemanticInputSnapshot>,
    ordered: Option<Res<'w, crate::ordered_interactions::InteractionRuntime>>,
    pub(crate) origin: Res<'w, InteractionOriginSnapshot>,
    pub(crate) ui: Res<'w, UiRuntimeResource>,
    pub(crate) focus: Res<'w, client_state::ScreenFocus>,
    pub(crate) windows: Query<'w, 's, &'static Window, With<PrimaryWindow>>,
    pub(crate) client_world: ResMut<'w, ClientWorld>,
    pub(crate) collisions: Res<'w, PhysicsCollisionRegistries>,
    pub(crate) effects: Res<'w, LocalMovementEffectTimeline>,
    pub(crate) network: Res<'w, NetworkHandle<PackApplication>>,
    pub(crate) time: Res<'w, Time<Real>>,
    pub(crate) aim: Res<'w, client_presentation::aim_assist::AimAssistFrame>,
    pub(crate) camera: Res<'w, client_presentation::camera::ServerCameraView>,
    pub(crate) mod_interaction: Option<Res<'w, ModInteraction>>,
    pub(crate) physics: Option<Res<'w, client_interaction::movement::LocalPhysicsController>>,
}

/// Publishes frame inputs and resolves presses through the single published-frame pick.
/// Open screens and spectator mode cancel the press.
pub(crate) fn prepare_melee(
    player_runtime: bevy::prelude::Res<client_state::PlayerRuntime>,
    context: MeleeContext,
    mut runtime: ResMut<MeleeRuntime>,
    mut swings: ResMut<SwingTracker>,
    movement: Res<MovementTicker>,
    mut target: ResMut<MeleeTarget>,
) {
    let ordered = context
        .input
        .snapshot()
        .filter(|input| input.ordered.is_some());
    let identity = context.input.snapshot().map(|input| {
        (
            input.authority_generation,
            input.frame_sequence,
            movement.position_authority_identity().1,
        )
    });
    if ordered.is_some() && target.prepared == identity {
        return;
    }
    target.attack = None;
    target.hover = None;
    target
        .frames
        .publish(&context, movement.position_authority_identity().1);
    let picked = resolve_crosshair(
        &player_runtime,
        &context,
        &target.frames,
        movement.position_authority_identity().1,
    );
    target.prepared = picked.and(identity);
    if let Some(picked) = picked {
        // Earlier queued movement must drain before this frame's first local tick.
        let first_tick = movement
            .completed_tick()
            .saturating_sub((context.effects.recent_tick_count() as u64).saturating_sub(1));
        target.hover = Some((
            first_tick,
            gameplay::entity_hover::HoverSnapshot {
                actor: picked.hover,
                held_item: hover::held_item_key(&player_runtime, &context.client_world),
            },
        ));
    }
    let item_timing = selected_attack_timing(&player_runtime, &context.client_world);
    swings.sync_ticks_for_item(
        movement.interaction_authority_identity(),
        movement.completed_tick(),
        &context.effects,
        item_timing.and_then(|timing| timing.swing_duration_ticks),
    );

    runtime.synchronize(movement.interaction_authority_identity());
    let attack = context.input.phase(Action::Attack);
    let drop = |reason| {
        if attack.pressed {
            gameplay::movement::note_click_drop("attack", reason);
        }
    };
    if context.input.snapshot().is_none() {
        runtime.cancel();
        return;
    }
    let now_millis = u64::try_from(context.time.elapsed().as_millis()).unwrap_or(u64::MAX);
    let focused =
        !context.focus.menu_visible && context.windows.single().is_ok_and(|window| window.focused);
    if let Err(reason) = press_admission(
        focused && !context.focus.ui_focused,
        player_runtime.facts.player_game_mode(),
    ) {
        drop(reason);
        runtime.cancel();
        return;
    }
    if ordered.is_some() {
        target.attack = picked;
        if !crate::ordered_interactions::is_blocked(context.ordered.as_deref())
            && let Some(picked) = picked
        {
            runtime.observe_attack_target(
                picked.crosshair,
                item_timing.is_some_and(|timing| timing.piercing_weapon),
            );
        }
        return;
    }
    let pending = runtime.observe_input(attack.pressed, attack.held);
    // A position-authority change is resolving; the press waits for it.
    if !movement.accepts_block_interactions() {
        drop("position_authority_pending");
        runtime.defer(now_millis);
        return;
    }
    if !pending {
        runtime.cancel();
        return;
    }
    let Some(mut picked) = picked else {
        drop("no_interaction_ray");
        runtime.defer(now_millis);
        return;
    };
    picked.crosshair = runtime.observe_attack_target(
        picked.crosshair,
        item_timing.is_some_and(|timing| timing.piercing_weapon),
    );
    target.attack = Some(picked);
}

/// Drains ordered presses before advancing this frame's held mining work once.
pub(crate) fn produce_attacks(world: &mut bevy::prelude::World) {
    crate::ordered_interactions::drain(
        world,
        crate::ordered_interactions::DispatchPhase::BeforeMovement,
    );
    world
        .run_system_cached(maintain_attacks)
        .expect("attack maintenance resources");
}

/// Admits attack presses and their mining carriers through one ordered queue.
fn maintain_attacks(
    mut player: ResMut<client_state::PlayerRuntime>,
    mut context: MeleeContext,
    target: Res<MeleeTarget>,
    state: (
        ResMut<MeleeRuntime>,
        ResMut<client_interaction::survival_mining::SurvivalMiningRuntime>,
        ResMut<SwingTracker>,
        Res<client_interaction::item_use::ItemUseRuntime>,
    ),
    mut movement: ResMut<MovementTicker>,
    mut view: ResMut<client_presentation::local_player::LocalViewPose>,
    mut cues: bevy::prelude::MessageWriter<client_audio::audio::LocalBlockCue>,
) {
    let (mut runtime, mut mining, mut swings, item_use) = state;
    let now_millis = u64::try_from(context.time.elapsed().as_millis()).unwrap_or(u64::MAX);
    let mut attack = (|| {
        if context
            .input
            .snapshot()
            .is_some_and(|input| input.ordered.is_some())
        {
            return None;
        }
        let picked = target.attack?;
        let input = context.input.snapshot()?;
        let stream = context.client_world.stream.as_ref()?;
        if movement.has_held_release() {
            return None;
        }
        let selection = hand_interaction_selection(&player)?;
        let sample = runtime.between_ticks_press(&movement).or_else(|| {
            runtime.press_sample(
                picked.crosshair,
                &movement,
                context.effects.recent_tick_count(),
                now_millis,
            )
        })?;
        let item = selected_attack_timing(&player, &context.client_world);
        Some((
            picked.crosshair,
            PressContext {
                refused_destroy: None,
                tick: sample.tick,
                player_position: sample.position,
                input_mode: protocol_input_mode(input.input_mode),
                local_runtime_id: stream.local_player_runtime_id(),
                selection: Some(selection),
                swing_duration: item
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
                now_millis,
                item_attack: item.filter(|timing| timing.piercing_weapon).map(|timing| {
                    gameplay::melee::ItemAttackPress {
                        direction: picked.direction,
                        cooldown: timing.attack_cooldown.clone(),
                    }
                }),
            },
        ))
    })();
    let caps = player.facts.game_mode_capabilities();
    let focused =
        !context.focus.menu_visible && context.windows.single().is_ok_and(|window| window.focused);
    let input = context.input.snapshot();
    let active = gameplay::survival_mining::mining_active(caps, focused, input.is_some());
    let attack_held = active
        && crate::ordered_interactions::held(
            context.ordered.as_deref(),
            &context.input,
            Action::Attack,
        );
    let wants_target =
        active && !runtime.actor_in_front() && (attack_held || runtime.press_pending());
    // A block press is refused at its start even where the game mode cannot mine.
    let block_press = focused
        && attack
            .as_ref()
            .is_some_and(|(crosshair, _)| *crosshair == gameplay::melee::Crosshair::Block);
    let observation = if wants_target || block_press {
        input.and_then(|input| {
            crate::survival_mining::observe_destroy_target(
                &player,
                &context,
                &context.origin,
                caps,
                input.input_mode,
                (input.authority_generation, input.frame_sequence),
                movement.position_authority_identity().1,
            )
        })
    } else {
        None
    };
    let destroy = match observation {
        Some(crate::survival_mining::DestroyObservation::Refused(refused)) => {
            if let Some((_, press)) = attack.as_mut() {
                press.refused_destroy = Some(refused);
            }
            None
        }
        Some(crate::survival_mining::DestroyObservation::Target(target)) if wants_target => {
            Some(target)
        }
        _ => None,
    };
    let local_runtime_id = context
        .client_world
        .stream
        .as_ref()
        .map_or(0, |stream| stream.local_player_runtime_id());
    let authority = player
        .facts
        .server_authoritative_block_breaking()
        .map(gameplay::survival_mining::BlockBreakingAuthority::from_negotiation)
        .unwrap_or(gameplay::survival_mining::BlockBreakingAuthority::Server);
    let completed_tick = movement.completed_tick();
    let effects = &context.effects;
    let network = &context.network;
    let world = &mut context.client_world;
    let allow_mining_start = context.ordered.as_deref().is_none_or(|ordered| {
        !ordered.maintenance_pending(&context.input)
            && !crate::ordered_interactions::release_pending(&item_use, &movement)
    });
    let admitted = gameplay::melee::admit_attack_frame(
        &mut runtime,
        &mut mining,
        &mut swings,
        &mut movement,
        gameplay::melee::AttackFrame {
            attack,
            mining_target: destroy.as_ref(),
            attack_held,
            allow_mining_start,
            authority,
            now_millis,
            local_runtime_id,
        },
        gameplay::melee::AttackEffects {
            send: |packets| network.send_inventory_packets(packets),
            swing_duration: |tick| swing_duration(effects.mining_tick(tick, completed_tick).0),
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
    for cue in mining.take_break_cues() {
        let gameplay::survival_mining::BlockBreakCue::Break {
            position,
            block_runtime_id,
        } = cue;
        cues.write(client_audio::audio::LocalBlockCue::Break {
            position,
            block_runtime_id,
        });
    }
    if let Some(tick) = admitted.rotation_tick
        && let Some(rotation) = target.attack.and_then(|picked| picked.action_rotation)
    {
        client_interaction::camera::aim_assist::apply_action_rotation(
            rotation,
            &mut view,
            &mut movement,
            tick,
        );
    }
}

/// Resolves selected-item component facts through the admitted registry and active catalog.
pub(crate) fn selected_attack_timing<'a>(
    player: &client_state::PlayerRuntime,
    world: &'a ClientWorld,
) -> Option<&'a protocol::ItemAttackTiming> {
    let stream = world.stream.as_ref()?;
    let item = stream
        .authority()
        .canonical_item_stack(player.selected_backing_stack()?)?;
    stream
        .authority()
        .item_attack_timing(item.identifier.as_deref()?)
}

#[cfg(test)]
mod session_tests;
