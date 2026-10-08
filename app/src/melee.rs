//! Attack-press actor picking, melee transactions and local arm swings.
//!
//! One attack per press; a held button never re-attacks. Swings obey the
//! half-swing guard shared with survival mining.

use bevy::{
    ecs::system::SystemParam,
    prelude::{Query, Real, Res, ResMut, Resource, Time, Window, With},
    window::PrimaryWindow,
};
use semantic_input::Action;

mod crosshair;
use crosshair::resolve_crosshair;

#[cfg(feature = "local-mods")]
use crate::modding::interaction::{ModInteraction, effective_reach};
use crate::{
    local_player::InteractionOriginSnapshot,
    menu::MenuRuntime,
    mining::{hand_interaction_selection, protocol_input_mode},
    movement::{LocalMovementEffectTimeline, MovementTicker, PhysicsCollisionRegistries},
    runtime::{network::NetworkHandle, world::ClientWorld},
    semantic_controls::SemanticInputSnapshot,
};
use client_ui::ui_runtime::UiRuntime;
use client_world::game_mode_capabilities::SURVIVAL_ATTACK_REACH;

pub(crate) use gameplay::melee::{
    Crosshair, PressContext, classify, obstructs_placement, pick_actor, press_admission,
    resolve_and_send, swing_duration,
};

/// Bevy resource adapter for the gameplay melee owner.
#[derive(Resource, Debug, Default, Clone)]
pub(crate) struct SwingTracker(gameplay::melee::SwingTracker);
impl std::ops::Deref for SwingTracker {
    type Target = gameplay::melee::SwingTracker;
    /// Borrows the gameplay owner at the existing ordered system boundary.
    fn deref(&self) -> &Self::Target {
        &self.0
    }
}
impl std::ops::DerefMut for SwingTracker {
    /// Mutates the gameplay owner without duplicating its state.
    fn deref_mut(&mut self) -> &mut Self::Target {
        &mut self.0
    }
}
/// Bevy resource adapter for the gameplay melee owner.
#[derive(Resource, Debug, Default, Clone)]
pub(crate) struct MeleeRuntime(gameplay::melee::MeleeRuntime);
impl std::ops::Deref for MeleeRuntime {
    type Target = gameplay::melee::MeleeRuntime;
    /// Borrows the gameplay owner at the existing ordered system boundary.
    fn deref(&self) -> &Self::Target {
        &self.0
    }
}
impl std::ops::DerefMut for MeleeRuntime {
    /// Mutates the gameplay owner without duplicating its state.
    fn deref_mut(&mut self) -> &mut Self::Target {
        &mut self.0
    }
}
#[derive(SystemParam)]
pub(crate) struct MeleeContext<'w, 's> {
    input: Res<'w, SemanticInputSnapshot>,
    origin: Res<'w, InteractionOriginSnapshot>,
    ui: Res<'w, UiRuntime>,
    menu: Res<'w, MenuRuntime>,
    windows: Query<'w, 's, &'static Window, With<PrimaryWindow>>,
    client_world: Res<'w, ClientWorld>,
    collisions: Res<'w, PhysicsCollisionRegistries>,
    effects: Res<'w, LocalMovementEffectTimeline>,
    network: Res<'w, NetworkHandle>,
    time: Res<'w, Time<Real>>,
    aim: Res<'w, client_presentation::aim_assist::AimAssistFrame>,
    camera: Res<'w, crate::camera::ServerCameraView>,
    #[cfg(feature = "local-mods")]
    mod_interaction: Option<Res<'w, ModInteraction>>,
}

/// Whether the attack button acts at all: only an open screen or spectator mode stops it,
/// whatever the game mode or abilities otherwise allow.
/// Runs before the mining producers so they can defer to a targeted actor.
///
/// Like vanilla's build-action handler, only an open screen or spectator mode ignores the
/// press; everything else swings.
pub(crate) fn produce_melee(
    player_runtime: bevy::prelude::Res<crate::player_runtime::PlayerRuntime>,
    context: MeleeContext,
    mut runtime: ResMut<MeleeRuntime>,
    mut swings: ResMut<SwingTracker>,
    mut movement: ResMut<MovementTicker>,
    mut view: ResMut<crate::local_player::LocalViewPose>,
) {
    swings.sync_ticks(
        movement.interaction_authority_identity(),
        movement.completed_tick(),
        &context.effects,
    );

    runtime.synchronize(movement.interaction_authority_identity());
    let attack = context.input.phase(Action::Attack);
    let drop = |reason| {
        if attack.pressed {
            crate::movement::note_click_drop("attack", reason);
        }
    };
    let Some(input) = context.input.snapshot() else {
        runtime.cancel();
        return;
    };
    let focused =
        !context.menu.is_visible() && context.windows.single().is_ok_and(|window| window.focused);
    if let Err(reason) = press_admission(
        focused && !context.ui.ui_focused(&player_runtime),
        player_runtime.facts.player_game_mode(),
    ) {
        drop(reason);
        runtime.cancel();
        return;
    }
    if !runtime.observe_input(attack.pressed, attack.held) {
        runtime.cancel();
        return;
    }
    // A position-authority change is resolving; the press waits for it.
    if !movement.accepts_block_interactions() {
        drop("position_authority_pending");
        runtime.defer(input.frame_sequence);
        return;
    }
    let caps = player_runtime.facts.game_mode_capabilities();
    let attack_reach = caps.map_or(SURVIVAL_ATTACK_REACH, |caps| caps.attack_reach);
    #[cfg(feature = "local-mods")]
    let attack_reach = effective_reach(attack_reach, context.mod_interaction.as_deref());
    let input_mode = protocol_input_mode(input.input_mode);
    let (Some(crosshair), Some(stream)) = (
        resolve_crosshair(
            &player_runtime,
            &context,
            input_mode,
            attack_reach,
            caps.is_some_and(|caps| caps.creative_reach),
            (input.authority_generation, input.frame_sequence),
            movement.interaction_authority_identity().1,
        ),
        context.client_world.stream.as_ref(),
    ) else {
        drop("no_interaction_ray");
        runtime.defer(input.frame_sequence);
        return;
    };
    runtime.observe_crosshair(crosshair);
    // An actor attack leaves in its own frame; an aim-assist facing needs an unsent tick to carry it.
    let between_ticks = crate::camera::aim_assist::action_rotation(&context.aim, &context.camera)
        .is_none()
        .then(|| runtime.between_ticks_attack(crosshair, &movement))
        .flatten();
    // Fresh block presses wait for a tick committed in this frame.
    let sample = between_ticks.or_else(|| {
        runtime
            .press_sample(
                crosshair,
                &movement,
                context.effects.recent_tick_count(),
                input.frame_sequence,
            )
            .map(Into::into)
    });
    let Some(sample) = sample else {
        return;
    };
    let press = PressContext {
        tick: sample.tick,
        player_position: sample.position,
        input_mode,
        local_runtime_id: stream.local_player_runtime_id(),
        selection: hand_interaction_selection(&player_runtime),
        swing_duration: swing_duration(
            context
                .effects
                .mining_tick(sample.tick, movement.completed_tick())
                .0,
        ),
        now_millis: u64::try_from(context.time.elapsed().as_millis()).unwrap_or(u64::MAX),
    };
    let mut rotate_action = false;
    let missed_swing = resolve_and_send(
        &mut runtime,
        &mut swings,
        crosshair,
        &press,
        input.frame_sequence,
        |packets| {
            let packet_count = packets.len();
            let packet_kinds = [
                packets.first().map(|packet| packet.header.id),
                packets.last().map(|packet| packet.header.id),
            ];
            let rotates = packets.iter().any(protocol::is_aim_assist_rotation_action);
            let result = context.network.send_inventory_packets(packets);
            rotate_action = rotates && result.is_ok();
            let actor = match crosshair {
                Crosshair::Actor(hit) => stream.authority().actor(hit.runtime_id),
                _ => None,
            };
            bevy::log::info!(
                target: "cinnabar::interaction",
                ?crosshair,
                actor_kind = ?actor.map(|actor| &actor.kind),
                actor_bounds = ?actor.and_then(|actor| actor.bounding_box()),
                actor_scale = ?actor.map(|actor| actor.render_scale()),
                hand_available = press.selection.is_some(),
                packet_count,
                ?packet_kinds,
                ?result,
                "attack batch submitted"
            );
            result
        },
    );
    if rotate_action {
        crate::camera::aim_assist::rotate_for_action(
            &context.aim,
            &context.camera,
            &mut view,
            &mut movement,
            sample.tick,
        );
    }
    if missed_swing {
        movement.mark_missed_swing(sample.tick);
    }
}

#[cfg(test)]
mod session_tests;

#[cfg(test)]
pub(crate) use gameplay::melee::ActorHit;
