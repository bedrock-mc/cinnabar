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

pub(crate) fn produce_block_use(
    mut player_runtime: bevy::prelude::ResMut<client_state::PlayerRuntime>,
    mut context: BlockUseContext,
    mut runtime: ResMut<BlockUseRuntime>,
    mut swings: ResMut<SwingTracker>,
    mut item_use: ResMut<client_interaction::item_use::ItemUseRuntime>,
    movement: Res<MovementTicker>,
    physics: Option<Res<client_interaction::movement::LocalPhysicsController>>,
) {
    let pick = runtime.pick(movement.interaction_authority_identity());
    if context.input.phase(Action::Use).pressed {
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
    let use_phase = context.input.phase(Action::Use);
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
    if context.input.phase(Action::Attack).pressed || context.melee.press_pending() {
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
    let clock = RepeatClock::for_state(
        u64::try_from(context.time.elapsed().as_millis()).unwrap_or(u64::MAX),
        &state,
        game_mode,
    );
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
    if context.melee.blocks_use_at(clock.now_millis) {
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
    let (Some(mut observed), Some(stream)) = (
        observe_use_target(
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
        ),
        context.client_world.stream.as_ref(),
    ) else {
        runtime.record(trigger, due, tick, LocalUse::Nothing, clock);
        return;
    };
    let server_selection = observed.selection.clone();
    let inventory_revision = context
        .ui
        .inventory_ledger(&player_runtime)
        .authoritative_slot_revision(server_selection.slot)
        .unwrap_or(0);
    observed.selection = runtime
        .inventory
        .selection(&server_selection, inventory_revision);
    let surroundings = use_surroundings(&context, &observed, state.position, state.sneaking);
    let mut local_use = LocalUse::resolve(
        &observed.selection.item,
        observed.target.position,
        observed.target.face,
        &surroundings,
        &caps,
    );
    let placement = (caps.can_build && local_use != LocalUse::Interact)
        .then(|| {
            predicted_placement(
                &context.collisions,
                stream,
                observed.selection.item.block_runtime_id(),
                &observed,
                &surroundings,
                *context.view,
            )
        })
        .flatten();
    if placement.is_some() && observed.selection.item.count() > 0 {
        local_use = LocalUse::Place;
    }
    if !runtime.may_attempt(tick, local_use, &swings) {
        return;
    }
    let destination = placement.as_ref().map_or_else(
        || {
            surroundings
                .destination(observed.target.position, observed.target.face)
                .0
        },
        |placement| placement.position,
    );
    let predicted = placement
        .filter(|_| local_use == LocalUse::Place)
        .map(|placement| {
            std::iter::once((placement.position, 0, placement.block))
                .chain(
                    placement
                        .additional
                        .into_iter()
                        .map(|(position, block)| (position, 0, block)),
                )
                .collect::<Vec<_>>()
        });
    let predicted = predicted.or_else(|| {
        (local_use == LocalUse::Interact)
            .then(|| predicted_toggle(&context.collisions, stream, observed.target.runtime_id))
            .flatten()
            .map(|block| vec![(observed.target.position, 0, block)])
    });
    let local_runtime_id = stream.local_player_runtime_id();
    // Only block items keep using while held.
    if trigger == ItemUseTrigger::SimulationTick && observed.selection.item.block_runtime_id() == 0
    {
        runtime.record(trigger, due, tick, LocalUse::Nothing, clock);
        return;
    }
    // The tick has not simulated yet, so its swing reads the effects in force before it.
    let swing_effects = context
        .effects
        .mining_tick(tick, movement.completed_tick())
        .0;
    let Some(block_network_id) = stream.block_network_id(observed.target.runtime_id) else {
        return;
    };
    item_use.synchronize(context.ui.session_id());
    let legacy_request_id = item_use.next_legacy_request_id();
    let change = (local_use == LocalUse::Place && game_mode != Some(PlayerGameMode::Creative))
        .then(|| {
            runtime
                .inventory
                .prepare_change(&observed.selection, legacy_request_id)
        });
    let start_destination = runtime
        .last_success_destination()
        .is_none()
        .then_some(destination);
    let mut before_swing = swings.clone();
    let packets = use_packets(
        (&observed, block_network_id),
        state.position,
        trigger,
        local_use,
        start_destination,
        change.clone(),
        local_runtime_id,
        |tick| swings.try_swing_before_tick(tick, swing_effects),
        tick,
    );
    let result = (!packets.is_empty()).then(|| context.network.send_inventory_packets(packets));
    let sent = matches!(result, Some(Ok(())));
    if sent {
        runtime.intention.record(
            trigger == ItemUseTrigger::SimulationTick,
            destination,
            local_use,
            local_use != LocalUse::Interact
                && held_block_store_id(stream, observed.selection.item.block_runtime_id())
                    .is_some_and(|block| {
                        context
                            .collisions
                            .block_has_build_intention(stream.network_id_mode(), block)
                    }),
            state.sneaking,
            std::array::from_fn(|axis| {
                observed.target.position[axis] as f32 + observed.target.relative_hit[axis]
            }),
        );
    }
    if !runtime.admit(trigger, due, tick, local_use, clock, sent) {
        runtime.refuse_transport(
            tick,
            local_use,
            matches!(result, Some(Err(client_session::BatchSendError::Full))),
        );
        before_swing.defer_unadmitted_attempt(&swings);
        *swings = before_swing;
        return;
    }
    if let Some(change) = change {
        if change.to.is_empty() {
            player_runtime
                .inventory
                .ledger_mut()
                .settle_use_emptied_slot(server_selection.slot, inventory_revision);
        }
        runtime
            .inventory
            .commit(server_selection, inventory_revision, change);
    }
    if local_use == LocalUse::Place {
        let position = destination;
        context
            .audio_cues
            .write(client_audio::audio::LocalBlockCue::Place {
                position,
                block_runtime_id: stream.resolve_block_network_id(u32::from_ne_bytes(
                    observed.selection.item.block_runtime_id().to_ne_bytes(),
                )) as i32,
            });
    }
    // Java re-equips the held item after each placement.
    if local_use == LocalUse::Place
        && let Some(stream) = context.client_world.stream.as_mut()
    {
        stream.reset_local_java_equip();
    }
    // Vanilla places locally as it sends; a correction replaces the prediction.
    if let (true, Some(cells), Some(stream)) =
        (sent, predicted, context.client_world.stream.as_mut())
    {
        let applied = stream.predict_blocks(&cells);
        if applied {
            runtime.prediction_frames.committed(
                context.frame.0,
                cells
                    .iter()
                    .filter_map(|cell| stream.prediction_generation(cell.0)),
            );
        }
        bevy::log::debug!(?cells, applied, "local block prediction committed");
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
            Default::default(),
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
    let mut surroundings = UseSurroundings {
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
