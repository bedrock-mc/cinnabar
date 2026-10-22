//! Air item use: the click-air transaction every held item sends, and holding, releasing and
//! throwing.
//!
//! Follows vanilla's build action, air use, release and use completion; projectiles,
//! food effects and ammunition stay server-owned.

use client_interaction::item_use::{
    ItemUseRuntime,
    selection::{
        loading_projectile, needs_met, selected_air_use_with_projectile, selected_riptide_use,
    },
};

use crate::runtime::network::resource_packs::PackApplication;
use bevy::{
    ecs::system::SystemParam,
    prelude::{Query, Real, Res, ResMut, Resource, Time, Window, With},
    window::PrimaryWindow,
};
use client_runtime::network::NetworkHandle;
use protocol::PlayerGameMode;
use semantic_input::Action;

use client_shell::ui_resources::UiRuntimeResource;
use {
    crate::runtime::world::ClientWorld,
    client_interaction::{
        block_use::{BlockUseRuntime, verified_use_selection},
        melee::{MeleeRuntime, SwingTracker},
        movement::{LocalMovementEffectTimeline, MovementTicker},
    },
    client_state::SemanticInputSnapshot,
    gameplay::melee::swing_duration,
};

use gameplay::item_use::UseFrame;
use gameplay::item_use::admit_on_tick;

use gameplay::item_use::AirUse;
mod frame;
pub(crate) mod ordered;
mod riptide;

/// A successful extension request is valid only in its originating world scope.
#[derive(Resource, Debug, Default)]
pub(crate) struct ModItemUsePolicy {
    pub scope: Option<(u64, i32)>,
}

#[derive(SystemParam)]
pub(crate) struct ItemUseContext<'w, 's> {
    input: Res<'w, SemanticInputSnapshot>,
    ordered: Option<Res<'w, crate::ordered_interactions::InteractionRuntime>>,
    delay_fix: Option<Res<'w, ModItemUsePolicy>>,
    ui: Res<'w, UiRuntimeResource>,
    focus: Res<'w, client_state::ScreenFocus>,
    windows: Query<'w, 's, &'static Window, With<PrimaryWindow>>,
    client_world: Res<'w, ClientWorld>,
    melee: Res<'w, MeleeRuntime>,
    block_use: Res<'w, BlockUseRuntime>,
    effects: Res<'w, LocalMovementEffectTimeline>,
    physics: Option<ResMut<'w, client_interaction::movement::LocalPhysicsController>>,
    weather: Option<Res<'w, render_scene::AtmosphereViewInputs>>,
    network: Res<'w, NetworkHandle<PackApplication>>,
    time: Res<'w, Time<Real>>,
    aim: Res<'w, client_presentation::aim_assist::AimAssistFrame>,
    camera: Res<'w, client_presentation::camera::ServerCameraView>,
}

/// Consumes new frame edges after movement drains, then advances accepted held item use once.
pub(crate) fn produce_item_use(world: &mut bevy::prelude::World) {
    crate::ordered_interactions::drain(
        world,
        crate::ordered_interactions::DispatchPhase::AfterMovement,
    );
    world
        .run_system_cached(maintain_item_use)
        .expect("item maintenance resources");
}

/// Applies a server `UsingItem` write to the next tick before physics simulates
/// it, as vanilla's movement-tick start-or-stop system does. A write that
/// cannot resolve yet waits for the pass after movement.
pub(crate) fn adopt_server_item_use(
    player_runtime: Res<client_state::PlayerRuntime>,
    mut context: ItemUseContext,
    mut runtime: ResMut<ItemUseRuntime>,
    mut movement: ResMut<MovementTicker>,
    mut view: ResMut<client_presentation::local_player::LocalViewPose>,
) {
    if let Some(using) = context
        .physics
        .as_deref_mut()
        .and_then(|physics| physics.take_server_using_item())
    {
        runtime.adopt_server_using_item(using);
    }
    let Some(position) = context
        .physics
        .as_deref()
        .and_then(|physics| physics.network_position())
    else {
        return;
    };
    if !runtime.has_server_using_item() {
        return;
    }
    // The next tick physics simulates, whether or not earlier inputs await sending.
    let sample = gameplay::movement::InteractionSample {
        tick: movement.next_tick(),
        position,
    };
    let policy = frame::FramePolicy {
        held: true,
        before_flush: false,
        delay_fix_enabled: false,
        aim_rotation: None,
        release_facing: Default::default(),
        sample: Some(sample),
    };
    // Presses wait for their own pass; this one resolves only the server write.
    let mut pending = gameplay::item_use::PendingUseInput::new(false);
    runtime.with_pending_input(&mut pending, |runtime| {
        let Some(proposal) = frame::observe(
            &player_runtime,
            &context,
            runtime,
            &movement,
            &mut view,
            policy,
        ) else {
            return;
        };
        if runtime.resolve_server_using_item(&proposal.frame) {
            movement.mark_started_using_item(proposal.frame.tick);
        }
        if let Some(tick) = runtime.take_withdrawn_start() {
            movement.withdraw_started_using_item(tick);
        }
    });
}

/// Runs after block use so a press that interacted with a block starts no item use.
fn maintain_item_use(
    player_runtime: ResMut<client_state::PlayerRuntime>,
    context: ItemUseContext,
    runtime: ResMut<ItemUseRuntime>,
    movement: ResMut<MovementTicker>,
    swings: ResMut<SwingTracker>,
    view: ResMut<client_presentation::local_player::LocalViewPose>,
) {
    produce_use(
        player_runtime,
        context,
        runtime,
        movement,
        swings,
        view,
        false,
    );
}

/// Stages an assisted release on a completed using tick before its facing input drains.
pub(crate) fn prepare_item_release(
    player_runtime: ResMut<client_state::PlayerRuntime>,
    context: ItemUseContext,
    runtime: ResMut<ItemUseRuntime>,
    movement: ResMut<MovementTicker>,
    swings: ResMut<SwingTracker>,
    view: ResMut<client_presentation::local_player::LocalViewPose>,
) {
    produce_use(
        player_runtime,
        context,
        runtime,
        movement,
        swings,
        view,
        true,
    );
}

/// Resolves assisted releases before the drain and other item actions after it.
fn produce_use(
    mut player_runtime: ResMut<client_state::PlayerRuntime>,
    mut context: ItemUseContext,
    mut runtime: ResMut<ItemUseRuntime>,
    mut movement: ResMut<MovementTicker>,
    mut swings: ResMut<SwingTracker>,
    mut view: ResMut<client_presentation::local_player::LocalViewPose>,
    before_flush: bool,
) {
    runtime.synchronize(context.ui.session_id());
    if context.client_world.respawn.input_held() {
        runtime.cancel_for_respawn();
        return;
    }
    // Writes that arrived after physics ran resolve against the next tick here.
    if let Some(using) = context
        .physics
        .as_deref_mut()
        .and_then(|physics| physics.take_server_using_item())
    {
        runtime.adopt_server_using_item(using);
    }
    movement.send_held_release(|packets| context.network.send_inventory_packets(packets));
    swings.sync_ticks_for_item(
        movement.interaction_authority_identity(),
        movement.completed_tick(),
        &context.effects,
        crate::melee::selected_attack_timing(&player_runtime, &context.client_world)
            .and_then(|timing| timing.swing_duration_ticks),
    );

    let focused =
        !context.focus.menu_visible && context.windows.single().is_ok_and(|window| window.focused);
    let use_phase = context.input.phase(Action::Use);
    let admitted = if context.input.snapshot().is_none() {
        false
    } else if !focused || context.focus.ui_focused {
        use_phase
            .pressed
            .then(|| gameplay::movement::note_click_drop("use", "screen_open"));
        false
    } else if player_runtime
        .facts
        .game_mode_capabilities()
        .is_some_and(|caps| !caps.can_use_items)
    {
        use_phase
            .pressed
            .then(|| gameplay::movement::note_click_drop("use", "spectator"));
        false
    } else {
        true
    };
    runtime.observe_gameplay_focus(focused && !context.focus.ui_focused);
    let held = admitted
        && crate::ordered_interactions::held(
            context.ordered.as_deref(),
            &context.input,
            Action::Use,
        );
    runtime.observe_use_button(held);
    // Accepted uses may end while new starts wait for the ordered cursor.
    if admitted
        && context
            .ordered
            .as_deref()
            .is_some_and(|ordered| ordered.maintenance_pending(&context.input))
        && !runtime.is_releasing(held)
    {
        return;
    }
    let aim_rotation =
        client_interaction::camera::aim_assist::action_rotation(&context.aim, &context.camera);
    let release_facing = runtime
        .riptide_release_facing()
        .unwrap_or_else(|| riptide::release_facing(aim_rotation, &view));
    let assisted_release = release_facing.assisted && runtime.is_releasing(held);
    if before_flush != assisted_release {
        return;
    }
    if !admitted {
        runtime.cancel_pending_input();
    }
    let scope = context.client_world.stream.as_ref().map(|stream| {
        let authority = stream.authority();
        (authority.actor_session_id(), authority.current_dimension())
    });
    let delay_fix_enabled = admitted
        && scope.is_some()
        && context
            .delay_fix
            .as_ref()
            .is_some_and(|policy| policy.scope == scope);
    runtime.set_delay_fix(delay_fix_enabled);
    runtime.observe_selected_slot(player_runtime.selected_hotbar_slot());
    runtime.observe_press(
        admitted
            && use_phase.pressed
            && context
                .input
                .snapshot()
                .is_none_or(|input| input.ordered.is_none()),
    );
    if !runtime.has_work(held) {
        return;
    }
    let policy = frame::FramePolicy {
        held,
        before_flush,
        delay_fix_enabled,
        aim_rotation,
        release_facing,
        sample: None,
    };
    let Some(frozen) = frame::observe(
        &player_runtime,
        &context,
        &mut runtime,
        &movement,
        &mut view,
        policy,
    ) else {
        return;
    };
    frame::admit(
        &mut player_runtime,
        &mut context,
        &mut runtime,
        &mut movement,
        &mut swings,
        &mut view,
        &frozen,
    );
}

/// Admits use on its logical clock and fences releases behind their captured action facing.
#[allow(clippy::too_many_arguments)]
fn admit_with_action_aim(
    runtime: &mut gameplay::item_use::ItemUseRuntime,
    swings: &mut gameplay::melee::SwingTracker,
    movement: &mut MovementTicker,
    view: &mut client_presentation::local_player::LocalViewPose,
    frame: &UseFrame,
    local_runtime_id: u64,
    swing_duration: i32,
    aim_rotation: Option<bevy::prelude::Quat>,
    network: &NetworkHandle<PackApplication>,
) {
    let release_facing = runtime
        .riptide_release_facing()
        .unwrap_or_else(|| riptide::release_facing(aim_rotation, view));
    let assisted_release = release_facing.assisted && runtime.is_releasing(frame.held);
    // Facing uses a completed input; elapsed use time includes that tick.
    let action_frame = UseFrame {
        tick: if assisted_release {
            movement.completed_tick().saturating_add(1)
        } else {
            frame.tick
        },
        riptide_facing: release_facing,
        ..frame.clone()
    };
    let first = runtime.observe_riptide_release(
        action_frame.tick,
        action_frame.held,
        action_frame.riptide,
        action_frame.selection.as_ref(),
        release_facing,
    );
    if first && let Some(rotation) = aim_rotation {
        view.set_rotation(rotation);
    }
    // Freeze the edge before waiting, retaining slowdown until facing can be written.
    if assisted_release && movement.unsent_sample_at(frame.tick).is_none() {
        return;
    }
    let riptide_fence = runtime.riptide_release_facing().is_some();
    let use_modifier = runtime.movement_modifier();
    let mut deferred = None;
    admit_on_tick(
        runtime,
        swings,
        movement,
        &action_frame,
        local_runtime_id,
        swing_duration,
        |packets| {
            if (aim_rotation.is_some() || riptide_fence)
                && packets.iter().any(protocol::is_aim_assist_rotation_action)
            {
                deferred = Some(packets);
                return Ok(());
            }
            network.send_inventory_packets(packets)
        },
    );
    if let Some(packets) = deferred {
        if riptide_fence {
            movement.override_action_rotation(
                frame.tick,
                release_facing.rotation[0],
                release_facing.rotation[1],
            );
        } else if let Some(rotation) = aim_rotation {
            client_interaction::camera::aim_assist::apply_action_rotation(
                rotation, view, movement, frame.tick,
            );
        }
        movement.hold_release_after_tick(frame.tick, packets);
        movement.retain_release_use_modifier(use_modifier);
    }
}

#[cfg(test)]
mod session_tests;
