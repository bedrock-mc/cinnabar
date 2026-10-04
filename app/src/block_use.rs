//! Block use as standalone click-block transactions on the press and while held.
//!
//! The local use outcome (interaction, placement or nothing) decides the
//! transaction's prediction and swing. A placement or switch toggle whose state
//! is certain is also applied locally; the server's block updates stay authoritative. Air
//! use lives in `item_use`; item-use-on start/stop actions are not implemented.

use bevy::{
    ecs::system::SystemParam,
    prelude::{Query, Real, Res, ResMut, Resource, Time, Window, With},
    window::PrimaryWindow,
};
use protocol::{ItemUseTrigger, PlayerGameMode, PlayerInputMode};
use semantic_input::Action;
use sim::PaletteWorld;

use crate::{
    interaction_authority::{FrozenBlockObservation, observe_block, within_pick_range},
    local_player::InteractionOriginSnapshot,
    melee::{MeleeRuntime, SwingTracker, obstructs_placement, swing_duration},
    menu::MenuRuntime,
    mining::{
        FrozenMiningSelection, creative_reach, protocol_input_mode, survival_reach,
        verified_selection,
    },
    movement::{LocalMovementEffectTimeline, MovementTicker, PhysicsCollisionRegistries},
    runtime::{network::NetworkHandle, world::ClientWorld},
    semantic_controls::SemanticInputSnapshot,
    ui_runtime::UiRuntime,
};

pub(crate) use gameplay::block_use::{
    LocalUse, RepeatClock, UseSurroundings, placement_cell, use_packets,
};

/// Bevy resource adapter for the gameplay block_use owner.
#[derive(Resource, Debug, Default)]
pub(crate) struct BlockUseRuntime(gameplay::block_use::BlockUseRuntime);
impl std::ops::Deref for BlockUseRuntime {
    type Target = gameplay::block_use::BlockUseRuntime;
    /// Borrows the gameplay owner at the existing ordered system boundary.
    fn deref(&self) -> &Self::Target {
        &self.0
    }
}
impl std::ops::DerefMut for BlockUseRuntime {
    /// Mutates the gameplay owner without duplicating its state.
    fn deref_mut(&mut self) -> &mut Self::Target {
        &mut self.0
    }
}
#[derive(SystemParam)]
pub(crate) struct BlockUseContext<'w, 's> {
    input: Res<'w, SemanticInputSnapshot>,
    origin: Res<'w, InteractionOriginSnapshot>,
    ui: Res<'w, UiRuntime>,
    menu: Res<'w, MenuRuntime>,
    windows: Query<'w, 's, &'static Window, With<PrimaryWindow>>,
    client_world: ResMut<'w, ClientWorld>,
    collisions: Res<'w, PhysicsCollisionRegistries>,
    effects: Res<'w, LocalMovementEffectTimeline>,
    melee: Res<'w, MeleeRuntime>,
    network: Res<'w, NetworkHandle>,
    time: Res<'w, Time<Real>>,
    audio_cues: bevy::prelude::MessageWriter<'w, crate::audio::LocalBlockCue>,
}

pub(crate) fn produce_block_use(
    player_runtime: bevy::prelude::Res<crate::player_runtime::PlayerRuntime>,
    mut context: BlockUseContext,
    mut runtime: ResMut<BlockUseRuntime>,
    mut swings: ResMut<SwingTracker>,
    movement: Res<MovementTicker>,
) {
    runtime.synchronize(movement.interaction_authority_identity());
    let focused =
        !context.menu.is_visible() && context.windows.single().is_ok_and(|window| window.focused);
    let game_mode = context.ui.player_game_mode(&player_runtime);
    let caps = context.ui.game_mode_capabilities(&player_runtime);
    let Some((input, caps)) = context.input.snapshot().zip(caps).filter(|(input, caps)| {
        focused
            && !context.ui.ui_focused(&player_runtime)
            && caps.can_use_blocks()
            && input.input_mode != semantic_input::InputMode::Touch
            && movement.accepts_block_interactions()
    }) else {
        runtime.clear();
        return;
    };
    let use_phase = context.input.phase(Action::Use);
    if !runtime.observe_use(
        use_phase.held,
        use_phase.pressed,
        context.input.phase(Action::Attack).held,
    ) {
        return;
    }
    // Frames between physics ticks have no unsent tick; a press waits for one.
    let Some(sample) = movement.newest_unsent_sample() else {
        return;
    };
    let clock = RepeatClock {
        now_millis: u64::try_from(context.time.elapsed().as_millis()).unwrap_or(u64::MAX),
        sneaking: sample.sneaking,
        speed: sample
            .delta
            .map(|axis| axis * sim::TICKS_PER_SECOND as f32)
            .into_iter()
            .map(|axis| axis * axis)
            .sum::<f32>()
            .sqrt(),
        survival: game_mode == Some(PlayerGameMode::Survival),
    };
    let Some((trigger, due)) = runtime.due(use_phase.held, sample.tick, clock) else {
        return;
    };
    if context.melee.blocks_use_at(clock.now_millis) {
        runtime.clear_press();
        return;
    }
    let input_mode = protocol_input_mode(input.input_mode);
    let (Some(observed), Some(stream)) = (
        observe_use_target(
            &player_runtime,
            &context,
            input_mode,
            clock.survival,
            (input.authority_generation, input.frame_sequence),
            movement.interaction_authority_identity().1,
        ),
        context.client_world.stream.as_ref(),
    ) else {
        runtime.record(trigger, due, sample.tick, LocalUse::Nothing, clock);
        return;
    };
    let surroundings = use_surroundings(&context, &observed, sample.position, sample.sneaking);
    let local_use = LocalUse::resolve(
        &observed.selection.item,
        observed.target.position,
        observed.target.face,
        &surroundings,
        &caps,
    );
    let (destination, _) = surroundings.destination(observed.target.position, observed.target.face);
    let predicted = (local_use == LocalUse::Place)
        .then(|| {
            predicted_placement(
                &context.collisions,
                stream,
                observed.selection.item.block_runtime_id(),
            )
        })
        .flatten()
        .map(|block| (destination, block));
    let predicted = predicted.or_else(|| {
        (local_use == LocalUse::Interact)
            .then(|| predicted_toggle(&context.collisions, stream, observed.target.runtime_id))
            .flatten()
            .map(|block| (observed.target.position, block))
    });
    let local_runtime_id = stream.local_player_runtime_id();
    // Only block items keep using while held.
    if trigger == ItemUseTrigger::SimulationTick && observed.selection.item.block_runtime_id() == 0
    {
        runtime.record(trigger, due, sample.tick, local_use, clock);
        return;
    }
    let duration = swing_duration(context.effects.mining_effects());
    let before_swing = swings.clone();
    let packets = use_packets(
        &observed,
        sample.position,
        trigger,
        local_use,
        local_runtime_id,
        |tick| swings.try_swing(tick, duration),
        sample.tick,
    );
    let sent = !packets.is_empty() && context.network.send_inventory_packets(packets).is_ok();
    if !runtime.admit(trigger, due, sample.tick, local_use, clock, sent) {
        *swings = before_swing;
        return;
    }
    if local_use == LocalUse::Place {
        let position = destination;
        context
            .audio_cues
            .write(crate::audio::LocalBlockCue::Place {
                position,
                block_runtime_id: observed.selection.item.block_runtime_id(),
            });
    }
    // Vanilla places locally as it sends; a correction replaces the prediction.
    if let (true, Some((position, block)), Some(stream)) =
        (sent, predicted, context.client_world.stream.as_mut())
    {
        let applied = stream.predict_block(position, 0, block);
        bevy::log::debug!(
            ?position,
            block,
            applied,
            "local block prediction committed"
        );
    }
}

fn use_surroundings(
    context: &BlockUseContext,
    observed: &FrozenBlockObservation,
    network_position: [f32; 3],
    sneaking: bool,
) -> UseSurroundings {
    let stream = context.client_world.stream.as_ref();
    let identifier = |position: [i32; 3]| -> Option<String> {
        let stream = stream?;
        let mode = stream.network_id_mode();
        let world = PaletteWorld::new(
            stream.collision_store(),
            context.collisions.registry(mode),
            stream.current_dimension(),
        );
        if world.is_air(position).ok()? {
            return Some("minecraft:air".to_owned());
        }
        let runtime_id = world.primary_runtime_id(position).ok()?;
        context
            .collisions
            .block_identifier(mode, runtime_id)
            .map(str::to_owned)
    };
    let feet = [
        f64::from(network_position[0]),
        f64::from(network_position[1] - protocol::PLAYER_NETWORK_OFFSET),
        f64::from(network_position[2]),
    ];
    let half_width = sim::PLAYER_WIDTH * 0.5;
    let height = sim::MovementMode::Walking.hitbox_height(sneaking);
    let placed_boxes = stream.and_then(|stream| {
        let shapes = context
            .collisions
            .registry(stream.network_id_mode())
            .collision_shapes(held_block_store_id(
                stream,
                observed.selection.item.block_runtime_id(),
            )?)?;
        Some(
            shapes
                .iter()
                .map(|shape| {
                    (
                        [shape.min.x, shape.min.y, shape.min.z],
                        [shape.max.x, shape.max.y, shape.max.z],
                    )
                })
                .collect(),
        )
    });
    UseSurroundings {
        clicked_identifier: context
            .collisions
            .block_identifier(
                stream.map_or(assets::NetworkIdMode::Sequential, |stream| {
                    stream.network_id_mode()
                }),
                observed.target.runtime_id,
            )
            .map(str::to_owned),
        clicked_canonical_state: stream.and_then(|stream| {
            context
                .collisions
                .block_canonical_state(stream.network_id_mode(), observed.target.runtime_id)
                .map(str::to_owned)
        }),
        held_block_identifier: stream.and_then(|stream| {
            let block = held_block_store_id(stream, observed.selection.item.block_runtime_id())?;
            context
                .collisions
                .block_identifier(stream.network_id_mode(), block)
                .map(str::to_owned)
        }),
        neighbor_identifier: identifier(placement_cell(
            observed.target.position,
            observed.target.face,
        )),
        player_box: (
            [feet[0] - half_width, feet[1], feet[2] - half_width],
            [feet[0] + half_width, feet[1] + height, feet[2] + half_width],
        ),
        actor_boxes: stream
            .into_iter()
            .flat_map(|stream| stream.remote_actors())
            .filter(|actor| obstructs_placement(actor))
            .filter_map(|actor| actor.bounding_box())
            .map(|(min, max)| (min.map(f64::from), max.map(f64::from)))
            .collect(),
        sneaking,
        placed_boxes,
    }
}

fn observe_use_target(
    player_runtime: &crate::player_runtime::PlayerRuntime,
    context: &BlockUseContext,
    input_mode: PlayerInputMode,
    survival: bool,
    input_authority: (std::num::NonZeroU64, u64),
    position_authority_generation: u64,
) -> Option<FrozenBlockObservation> {
    let reach = if survival {
        survival_reach(input_mode)
    } else {
        creative_reach(input_mode)
    };
    let observed = observe_block(
        &context.origin,
        &context.ui,
        &context.client_world,
        &context.collisions,
        verified_use_selection(player_runtime, &context.ui)?,
        (
            input_mode,
            reach,
            input_authority,
            position_authority_generation,
        ),
    )?;
    within_pick_range(&observed).then_some(observed)
}

/// The selected stack, only while no inventory request or hotbar change is in flight.
pub(crate) fn verified_use_selection(
    player_runtime: &crate::player_runtime::PlayerRuntime,
    ui: &UiRuntime,
) -> Option<FrozenMiningSelection> {
    let ledger = ui.inventory_ledger(player_runtime);
    if ledger.pending_request_id().is_some()
        || ledger.resync_required()
        || ui.pending_hotbar_selection(player_runtime).is_some()
    {
        return None;
    }
    verified_selection(player_runtime, ui)
}

#[cfg(test)]
mod tests;

/// Adapts the published world to gameplay's switch prediction.
fn predicted_toggle(
    collisions: &PhysicsCollisionRegistries,
    stream: &client_world::WorldStream,
    clicked: u32,
) -> Option<u32> {
    gameplay::block_use::predicted_toggle(
        collisions,
        &crate::movement::GameplayWorldView(stream),
        clicked,
    )
}
/// Adapts the published world to gameplay's placement prediction.
fn predicted_placement(
    collisions: &PhysicsCollisionRegistries,
    stream: &client_world::WorldStream,
    item_block: i32,
) -> Option<u32> {
    gameplay::block_use::predicted_placement(
        collisions,
        &crate::movement::GameplayWorldView(stream),
        item_block,
    )
}
/// Resolves an item block through the production gameplay boundary.
fn held_block_store_id(stream: &client_world::WorldStream, item_block: i32) -> Option<u32> {
    gameplay::block_use::held_block_store_id(
        &crate::movement::GameplayWorldView(stream),
        item_block,
    )
}
