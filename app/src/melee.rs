//! Attack-press actor picking, melee transactions and local arm swings.
//!
//! One attack per press; a held button never re-attacks. Swings obey the
//! half-swing guard shared with survival mining.

use bevy::{
    ecs::system::SystemParam,
    prelude::{Query, Real, Res, ResMut, Resource, Time, Window, With},
    window::PrimaryWindow,
};
use protocol::PlayerInputMode;
use semantic_input::Action;

use crate::{
    game_mode_capabilities::SURVIVAL_ATTACK_REACH,
    interaction_authority::{BlockRayUnavailable, observe_block_ray, ray_is_current},
    local_player::InteractionOriginSnapshot,
    menu::MenuRuntime,
    mining::{creative_reach, hand_interaction_selection, protocol_input_mode, survival_reach},
    movement::{LocalMovementEffectTimeline, MovementTicker, PhysicsCollisionRegistries},
    runtime::{network::NetworkHandle, world::ClientWorld},
    semantic_controls::SemanticInputSnapshot,
    ui_runtime::UiRuntime,
};

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
) {
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
        context.ui.player_game_mode(&player_runtime),
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
    let caps = context.ui.game_mode_capabilities(&player_runtime);
    let input_mode = protocol_input_mode(input.input_mode);
    let (Some(crosshair), Some(stream)) = (
        resolve_crosshair(
            &player_runtime,
            &context,
            input_mode,
            caps.map_or(SURVIVAL_ATTACK_REACH, |caps| caps.attack_reach),
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
    // Frames between physics ticks have no unsent tick; the press waits for one.
    let Some(sample) = movement.newest_unsent_sample() else {
        return;
    };
    let press = PressContext {
        tick: sample.tick,
        player_position: sample.position,
        input_mode,
        local_runtime_id: stream.local_player_runtime_id(),
        selection: hand_interaction_selection(&player_runtime, &context.ui),
        swing_duration: swing_duration(context.effects.mining_effects()),
        now_millis: u64::try_from(context.time.elapsed().as_millis()).unwrap_or(u64::MAX),
    };
    let missed_swing = resolve_and_send(
        &mut runtime,
        &mut swings,
        crosshair,
        &press,
        input.frame_sequence,
        |packets| context.network.send_inventory_packets(packets),
    );
    if missed_swing {
        movement.mark_missed_swing(sample.tick);
    }
}

fn resolve_crosshair(
    player_runtime: &crate::player_runtime::PlayerRuntime,
    context: &MeleeContext,
    input_mode: PlayerInputMode,
    attack_reach: f64,
    creative_pick_reach: bool,
    input_authority: (std::num::NonZeroU64, u64),
    position_authority_generation: u64,
) -> Option<Crosshair> {
    let ray = context.origin.outbound_ray()?;
    let stream = context.client_world.stream.as_ref()?;
    if !ray_is_current(ray, context.ui.session_id(), stream) {
        return None;
    }
    let reach = if creative_pick_reach {
        creative_reach(input_mode)
    } else {
        survival_reach(input_mode)
    };
    let origin = ray.origin().to_array();
    // Vanilla picks against the world it holds, where unreadable space is empty; an
    // unreadable block ray therefore neither blocks the swing nor occludes a target.
    let observed = hand_interaction_selection(player_runtime, &context.ui).and_then(|selection| {
        match observe_block_ray(
            &context.origin,
            &context.ui,
            &context.client_world,
            &context.collisions,
            selection,
            (
                input_mode,
                reach,
                input_authority,
                position_authority_generation,
            ),
        ) {
            Ok(observed) => observed,
            Err(BlockRayUnavailable) => {
                crate::movement::note_click_drop("attack", "block_ray_unreadable_treated_as_clear");
                None
            }
        }
    });
    let block_distance = observed.map(|observed| {
        let hit = observed.target.position;
        let offset = observed.target.relative_hit;
        (0..3)
            .map(|axis| {
                (f64::from(hit[axis]) + f64::from(offset[axis]) - f64::from(origin[axis])).powi(2)
            })
            .sum::<f64>()
            .sqrt()
    });
    let actor = pick_actor(
        stream.remote_actors(),
        context.ui.gameplay_hud().mount_unique_id(),
        origin,
        ray.direction().to_array(),
        reach,
    );
    Some(classify(actor, block_distance, attack_reach))
}

#[cfg(test)]
mod session_tests;

impl SwingTracker {
    /// Supplies the existing actor-publication adapter with the accepted swing duration.
    pub(crate) fn take_started(&mut self) -> Option<i32> {
        self.0.take_started()
    }
}

#[cfg(test)]
pub(crate) use gameplay::melee::ActorHit;
