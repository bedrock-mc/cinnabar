//! Attack-press actor picking, melee transactions and local arm swings.
//!
//! One attack per press; a held button never re-attacks. Swings obey the
//! half-swing guard shared with survival mining.

use crate::runtime::network::resource_packs::PackApplication;
use bevy::{
    ecs::system::SystemParam,
    prelude::{Local, Query, Real, Res, ResMut, Resource, Time, Window, With},
    window::PrimaryWindow,
};
use client_interaction::melee::{MeleeRuntime, SwingTracker};
use client_runtime::network::NetworkHandle;
use semantic_input::Action;

mod crosshair;
use crosshair::resolve_crosshair;

use crate::modding::interaction::ModInteraction;
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

/// This frame's resolved press awaiting admission before its movement input.
#[derive(Resource, Default)]
pub(crate) struct MeleeTarget(Option<crosshair::ResolvedTarget>);

#[derive(SystemParam)]
pub(crate) struct MeleeContext<'w, 's> {
    pub(crate) input: Res<'w, SemanticInputSnapshot>,
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
    mut frames: Local<crosshair::FrameInputs>,
) {
    target.0 = None;
    frames.publish(&context, movement.interaction_authority_identity().1);
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
    let Some(mut picked) = resolve_crosshair(
        &player_runtime,
        &context,
        &frames,
        movement.interaction_authority_identity().1,
    ) else {
        drop("no_interaction_ray");
        runtime.defer(now_millis);
        return;
    };
    picked.crosshair = runtime.observe_attack_target(
        picked.crosshair,
        item_timing.is_some_and(|timing| timing.piercing_weapon),
    );
    target.0 = Some(picked);
}

/// Admits attack presses and their mining carriers through one ordered queue.
pub(crate) fn produce_attacks(
    mut player: ResMut<client_state::PlayerRuntime>,
    mut context: MeleeContext,
    target: Res<MeleeTarget>,
    state: (
        ResMut<MeleeRuntime>,
        ResMut<client_interaction::survival_mining::SurvivalMiningRuntime>,
        ResMut<SwingTracker>,
    ),
    mut movement: ResMut<MovementTicker>,
    mut view: ResMut<client_presentation::local_player::LocalViewPose>,
    mut cues: bevy::prelude::MessageWriter<client_audio::audio::LocalBlockCue>,
) {
    let (mut runtime, mut mining, mut swings) = state;
    let now_millis = u64::try_from(context.time.elapsed().as_millis()).unwrap_or(u64::MAX);
    let attack = (|| {
        let picked = target.0?;
        let input = context.input.snapshot()?;
        let stream = context.client_world.stream.as_ref()?;
        if movement.has_held_release() {
            return None;
        }
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
                tick: sample.tick,
                player_position: sample.position,
                input_mode: protocol_input_mode(input.input_mode),
                local_runtime_id: stream.local_player_runtime_id(),
                selection: hand_interaction_selection(&player),
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
    let attack_held = active && context.input.phase(Action::Attack).held;
    let wants_target =
        active && !runtime.actor_in_front() && (attack_held || runtime.press_pending());
    let destroy = if wants_target {
        input.and_then(|input| {
            crate::survival_mining::observe_destroy_target(
                &player,
                &context,
                caps,
                input.input_mode,
                (input.authority_generation, input.frame_sequence),
                movement.interaction_authority_identity().1,
            )
        })
    } else {
        None
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
    let admitted = gameplay::melee::admit_attack_frame(
        &mut runtime,
        &mut mining,
        &mut swings,
        &mut movement,
        gameplay::melee::AttackFrame {
            attack,
            mining_target: destroy.as_ref(),
            attack_held,
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
        && let Some(rotation) = target.0.and_then(|picked| picked.action_rotation)
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
        .canonical_item_stack(player.selected_stack()?)?;
    stream
        .authority()
        .item_attack_timing(item.identifier.as_deref()?)
}

#[cfg(test)]
mod session_tests;
