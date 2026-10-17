//! Block use as standalone click-block transactions on the press and while held.
//!
//! The local use outcome (interaction, placement or nothing) decides the
//! transaction's prediction and swing. A validated placement or switch toggle
//! is also applied locally; the server's block updates stay authoritative. Air
//! use lives in `item_use`.

use client_interaction::block_use::{
    BlockUseRuntime, FramePick, held_block_store_id, predicted_placement, predicted_toggle,
    verified_use_selection,
};

use crate::runtime::network::resource_packs::PackApplication;
use bevy::{
    ecs::system::SystemParam,
    prelude::{Query, Real, Res, ResMut, Time, Window, With},
    window::PrimaryWindow,
};
use client_runtime::network::NetworkHandle;
use protocol::{ItemUseTrigger, PlayerGameMode, PlayerInputMode};
use semantic_input::Action;
use sim::PaletteWorld;

use client_shell::ui_resources::UiRuntimeResource;
use {
    crate::runtime::world::ClientWorld,
    client_interaction::{
        melee::{MeleeRuntime, SwingTracker},
        movement::{LocalMovementEffectTimeline, MovementTicker, PhysicsCollisionRegistries},
    },
    client_presentation::local_player::{InteractionOriginSnapshot, LocalViewPose},
    client_state::SemanticInputSnapshot,
    gameplay::{
        interaction_authority::FrozenBlockObservation, melee::obstructs_placement,
        mining::protocol_input_mode,
    },
};

mod actor_use;
mod build;
pub(crate) mod ordered;

use gameplay::block_use::{LocalUse, RepeatClock, UseSurroundings, placement_cell, use_packets};

#[derive(SystemParam)]
pub(crate) struct BlockUseContext<'w, 's> {
    input: Res<'w, SemanticInputSnapshot>,
    frame: Res<'w, bevy::diagnostic::FrameCount>,
    origin: Res<'w, InteractionOriginSnapshot>,
    view: Res<'w, LocalViewPose>,
    ui: Res<'w, UiRuntimeResource>,
    focus: Res<'w, client_state::ScreenFocus>,
    windows: Query<'w, 's, &'static Window, With<PrimaryWindow>>,
    client_world: ResMut<'w, ClientWorld>,
    collisions: Res<'w, PhysicsCollisionRegistries>,
    effects: Res<'w, LocalMovementEffectTimeline>,
    melee: Res<'w, MeleeRuntime>,
    network: Res<'w, NetworkHandle<PackApplication>>,
    time: Res<'w, Time<Real>>,
    audio_cues: bevy::prelude::MessageWriter<'w, client_audio::audio::LocalBlockCue>,
}

/// Drains physical interactions before physics, then advances held placement once.
pub(crate) fn produce_block_use(world: &mut bevy::prelude::World) {
    crate::ordered_interactions::drain(world);
    if !crate::ordered_interactions::pending(world) {
        world
            .run_system_cached(maintain_block_use)
            .expect("block maintenance resources");
    }
}

/// Advances held placement once after all immediately eligible ordered edges have drained.
fn maintain_block_use(
    mut player_runtime: bevy::prelude::ResMut<client_state::PlayerRuntime>,
    mut context: BlockUseContext,
    mut runtime: ResMut<BlockUseRuntime>,
    mut swings: ResMut<SwingTracker>,
    mut item_use: ResMut<client_interaction::item_use::ItemUseRuntime>,
    movement: Res<MovementTicker>,
    physics: Option<Res<client_interaction::movement::LocalPhysicsController>>,
) {
    let pick = runtime.pick(movement.interaction_authority_identity());
    if context.input.phase(Action::Use).pressed
        && !context
            .input
            .snapshot()
            .is_some_and(|input| input.ordered.is_some())
    {
        runtime.forget_press_resolution();
    }
    swings.sync_ticks_for_item(
        movement.interaction_authority_identity(),
        movement.completed_tick(),
        &context.effects,
        crate::melee::selected_attack_timing(&player_runtime, &context.client_world)
            .and_then(|timing| timing.swing_duration_ticks),
    );

    runtime.synchronize(movement.interaction_authority_identity());
    let focused =
        !context.focus.menu_visible && context.windows.single().is_ok_and(|window| window.focused);
    let game_mode = player_runtime.facts.player_game_mode();
    let caps = player_runtime.facts.game_mode_capabilities();
    let Some((input, caps)) = context.input.snapshot().zip(caps).filter(|(input, caps)| {
        focused
            && !context.focus.ui_focused
            && caps.can_use_blocks()
            && input.input_mode != semantic_input::InputMode::Touch
    }) else {
        runtime.clear_press();
        stop_block_use(&mut runtime, &context, false);
        return;
    };
    let mut use_phase = context.input.phase(Action::Use);
    if input.ordered.is_some() {
        use_phase.pressed = false;
        use_phase.released = false;
    }
    let attacking = context.input.phase(Action::Attack).held;
    if attacking {
        runtime.clear_press();
    }
    if (runtime.stopping()
        || (runtime.last_success_destination().is_some()
            && (attacking || use_phase.pressed || !use_phase.held)))
        && !stop_block_use(&mut runtime, &context, use_phase.pressed && !attacking)
    {
        return;
    }
    if !runtime.observe_use(
        use_phase.held,
        use_phase.pressed,
        attacking,
        movement.accepts_block_interactions(),
    ) {
        return;
    }
    // An attack in the same frame is handled first: the use waits until melee resolves it,
    // and an actor hit then holds the use off for the post-attack window.
    if (input.ordered.is_none() && context.input.phase(Action::Attack).pressed)
        || context.melee.press_pending()
    {
        return;
    }
    let Some(selection) = verified_use_selection(&player_runtime, &context.ui) else {
        // Unconfirmed inventory suspends attempts while preserving the held action.
        return;
    };
    runtime.selection_changed(&selection);
    // Vanilla builds before each simulation tick, from the last completed tick's end state;
    // this runs before the frame's physics, so a placed block is in the world the tick sees.
    let Some(state) = movement.build_action_state() else {
        return;
    };
    let tick = state.tick.saturating_add(1);
    let clock = RepeatClock::for_state(context.time.elapsed(), &state, game_mode);
    let Some((trigger, due)) = runtime.due(use_phase.held, tick, clock) else {
        return;
    };
    // Presses resolve at once; held repeats wait for a frame that simulates the next tick.
    if trigger == ItemUseTrigger::SimulationTick
        && !physics.as_deref().is_some_and(|physics| {
            gameplay::movement::frame_simulates_tick(physics, &movement, context.time.delta())
        })
    {
        return;
    }
    if context
        .melee
        .blocks_use_at(u64::try_from(clock.now.as_millis()).unwrap_or(u64::MAX))
    {
        runtime.clear_press();
        return;
    }
    // A press or repeat waits for a pick taken under the current movement authority.
    if pick.is_none() {
        return;
    }
    let input_mode = protocol_input_mode(input.input_mode);
    if actor_use::produce(
        &context,
        &mut runtime,
        &selection,
        player_runtime.facts.player_game_mode(),
        pick.expect("the current pick was checked above"),
        &state,
        input_mode,
        trigger,
        due,
        clock,
    ) {
        return;
    }
    let Some(observed) = observe_use_target(
        &player_runtime,
        &context,
        input_mode,
        clock.survival,
        (input.authority_generation, input.frame_sequence),
        movement.interaction_authority_identity().1,
        &runtime,
        pick,
        trigger,
        &state,
    ) else {
        runtime.record(trigger, due, tick, LocalUse::Nothing, clock);
        return;
    };
    if trigger == ItemUseTrigger::SimulationTick && observed.selection.item.block_runtime_id() == 0
    {
        runtime.record(trigger, due, tick, LocalUse::Nothing, clock);
        return;
    }
    let mut attempt = build::FrozenBuild::new(observed, state, trigger, due, clock);
    build::advance(
        &mut player_runtime,
        &mut context,
        &mut runtime,
        &mut item_use,
        &mut swings,
        &movement,
        &mut attempt,
    );
}

fn use_surroundings(
    context: &BlockUseContext,
    observed: &FrozenBlockObservation,
    network_position: [f32; 3],
    sneaking: bool,
) -> UseSurroundings {
    let stream = context.client_world.stream.as_ref();
    let fact = |position| {
        let stream = stream?;
        let mode = stream.network_id_mode();
        let world = PaletteWorld::new(
            stream.collision_store(),
            context.collisions.registry(mode),
            stream.current_dimension(),
            Default::default(),
        );
        let runtime_id = if world.is_air(position).ok()? {
            stream.air_block_id()
        } else {
            world.primary_runtime_id(position).ok()?
        };
        Some(context.collisions.block_facts(mode, runtime_id))
    };
    let feet = [
        f64::from(network_position[0]),
        f64::from(network_position[1] - protocol::PLAYER_NETWORK_OFFSET),
        f64::from(network_position[2]),
    ];
    let half_width = sim::PLAYER_WIDTH * 0.5;
    let height = sim::MovementMode::Walking.hitbox_height(sneaking);
    let mut surroundings = UseSurroundings {
        clicked: context.collisions.block_facts(
            stream.map_or(assets::NetworkIdMode::Sequential, |stream| {
                stream.network_id_mode()
            }),
            observed.target.runtime_id,
        ),
        clicked_canonical_state: stream.and_then(|stream| {
            context
                .collisions
                .block_canonical_state(stream.network_id_mode(), observed.target.runtime_id)
                .map(str::to_owned)
        }),
        held: stream
            .and_then(|stream| {
                let block =
                    held_block_store_id(stream, observed.selection.item.block_runtime_id())?;
                Some(
                    context
                        .collisions
                        .block_facts(stream.network_id_mode(), block),
                )
            })
            .unwrap_or_default(),
        neighbor: fact(placement_cell(
            observed.target.position,
            observed.target.face,
        ))
        .unwrap_or_default(),
        player_box: (
            [feet[0] - half_width, feet[1], feet[2] - half_width],
            [feet[0] + half_width, feet[1] + height, feet[2] + half_width],
        ),
        actor_boxes: stream
            .into_iter()
            .flat_map(|stream| stream.authority().remote_actors())
            .filter(|actor| obstructs_placement(actor))
            .filter_map(|actor| actor.bounding_box())
            .map(|(min, max)| (min.map(f64::from), max.map(f64::from)))
            .collect(),
        sneaking,
        placed_boxes: None,
    };
    if let Some((stream, held)) = stream.and_then(|stream| {
        held_block_store_id(stream, observed.selection.item.block_runtime_id())
            .map(|held| (stream, held))
    }) {
        surroundings.set_placed_collision_shapes(
            context.collisions.registry(stream.network_id_mode()),
            held,
            observed.target.position,
            observed.target.face,
        );
    }
    surroundings
}

mod target;
use target::observe_use_target;

/// Sends the held-use stop action before dropping its successful-placement history.
fn stop_block_use(runtime: &mut BlockUseRuntime, context: &BlockUseContext, repress: bool) -> bool {
    let Some(stream) = context.client_world.stream.as_ref() else {
        runtime.clear();
        return true;
    };
    let packets = runtime.stop_packets(stream.local_player_runtime_id(), repress);
    let admitted = packets.is_empty() || context.network.send_inventory_packets(packets).is_ok();
    runtime.admit_stop(admitted)
}

#[cfg(test)]
#[path = "block_use/published_tests.rs"]
mod published_tests;
