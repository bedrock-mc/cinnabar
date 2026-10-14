//! Air item use: the click-air transaction every held item sends, and holding, releasing and
//! throwing.
//!
//! Follows vanilla's build action, air use, release and use completion; projectiles,
//! food effects and ammunition stay server-owned.

use client_interaction::item_use::{
    ItemUseRuntime,
    selection::{loading_projectile, needs_met, selected_air_use_with_projectile},
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

/// A successful extension request is valid only in its originating world scope.
#[derive(Resource, Debug, Default)]
pub(crate) struct ModItemUsePolicy {
    pub scope: Option<(u64, i32)>,
}

#[derive(SystemParam)]
pub(crate) struct ItemUseContext<'w, 's> {
    input: Res<'w, SemanticInputSnapshot>,
    delay_fix: Option<Res<'w, ModItemUsePolicy>>,
    ui: Res<'w, UiRuntimeResource>,
    focus: Res<'w, client_state::ScreenFocus>,
    windows: Query<'w, 's, &'static Window, With<PrimaryWindow>>,
    client_world: Res<'w, ClientWorld>,
    melee: Res<'w, MeleeRuntime>,
    block_use: Res<'w, BlockUseRuntime>,
    effects: Res<'w, LocalMovementEffectTimeline>,
    physics: Option<Res<'w, client_interaction::movement::LocalPhysicsController>>,
    network: Res<'w, NetworkHandle<PackApplication>>,
    time: Res<'w, Time<Real>>,
    aim: Res<'w, client_presentation::aim_assist::AimAssistFrame>,
    camera: Res<'w, client_presentation::camera::ServerCameraView>,
}

/// Runs after block use so a press that interacted with a block starts no item use.
pub(crate) fn produce_item_use(
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
    context: ItemUseContext,
    mut runtime: ResMut<ItemUseRuntime>,
    mut movement: ResMut<MovementTicker>,
    mut swings: ResMut<SwingTracker>,
    mut view: ResMut<client_presentation::local_player::LocalViewPose>,
    before_flush: bool,
) {
    swings.sync_ticks_for_item(
        movement.interaction_authority_identity(),
        movement.completed_tick(),
        &context.effects,
        crate::melee::selected_attack_timing(&player_runtime, &context.client_world)
            .and_then(|timing| timing.swing_duration_ticks),
    );

    runtime.synchronize(context.ui.session_id());
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
    let held = admitted && use_phase.held;
    let aim_rotation =
        client_interaction::camera::aim_assist::action_rotation(&context.aim, &context.camera);
    let assisted_release = aim_rotation.is_some() && runtime.is_using() && !held;
    if before_flush != assisted_release
        || (before_flush && movement.newest_unsent_sample().is_none())
    {
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
    runtime.observe_press(admitted && use_phase.pressed);
    movement.send_held_release(|packets| context.network.send_inventory_packets(packets));
    if movement.has_held_release() {
        return;
    }
    let Some(stream) = context.client_world.stream.as_ref() else {
        return;
    };
    if !runtime.has_work(held) {
        return;
    }
    let sample = if before_flush {
        movement.newest_unsent_sample().map(Into::into)
    } else {
        runtime.frame_sample(&movement, held, true)
    };
    let Some(sample) = sample else {
        return;
    };
    // One press resolves once: item use waits while block use still holds it.
    if context.block_use.press_pending() {
        return;
    }
    let now_millis = u64::try_from(context.time.elapsed().as_millis()).unwrap_or(u64::MAX);
    let air_use = selected_air_use_with_projectile(
        &player_runtime,
        stream,
        runtime.selected_projectile(&player_runtime, &context.ui),
    );
    let creative = player_runtime.facts.player_game_mode() == Some(PlayerGameMode::Creative);
    let inventory_revision = player_runtime.selected_hotbar_slot().and_then(|slot| {
        context
            .ui
            .inventory_ledger(&player_runtime)
            .authoritative_slot_revision(slot)
    });
    let selection = verified_use_selection(&player_runtime, &context.ui).map(|server| {
        context
            .block_use
            .inventory
            .predicted_selection(&server, inventory_revision.unwrap_or(0))
            .unwrap_or(server)
    });
    let frame = UseFrame {
        tick: sample.tick,
        now_millis,
        position: sample.position,
        held,
        selection,
        air_use,
        ready: match air_use {
            Some(AirUse::Hold { needs, .. }) => {
                creative || needs_met(&player_runtime, stream, &context.ui, needs)
            }
            Some(AirUse::GlideRocket) => context.physics.as_deref().is_some_and(|physics| {
                physics.is_active() && physics.mode() == sim::MovementMode::Gliding
            }),
            _ => false,
        },
        creative,
        inventory_revision,
        charge_projectile: loading_projectile(&player_runtime, stream, &context.ui, creative),
        press_consumed: before_flush
            || (!delay_fix_enabled && context.melee.blocks_use_at(now_millis))
            || context.block_use.press_interacted(),
    };
    if let Some(reason) = runtime.press_drop_reason(&frame) {
        gameplay::movement::note_click_drop("use", reason);
    }
    let duration = swing_duration(
        context
            .effects
            .mining_tick(sample.tick, movement.completed_tick())
            .0,
    );
    admit_with_action_aim(
        &mut runtime,
        &mut swings,
        &mut movement,
        &mut view,
        &frame,
        stream.local_player_runtime_id(),
        duration,
        aim_rotation,
        &context.network,
    );
    if let Some((slot, revision, change)) = runtime.take_inventory_change() {
        player_runtime
            .inventory
            .ledger_mut()
            .settle_use_slot_change(slot, revision, &change);
    }
}

/// Admits item use, holding an assisted release behind the completed input carrying its facing.
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
    // Keep the use active until a completed using tick can carry the release facing.
    if aim_rotation.is_some()
        && runtime.is_using()
        && !frame.held
        && movement.unsent_sample_at(frame.tick).is_none()
    {
        return;
    }
    let mut assisted = None;
    admit_on_tick(
        runtime,
        swings,
        movement,
        frame,
        local_runtime_id,
        swing_duration,
        |packets| {
            if aim_rotation.is_some() && packets.iter().any(protocol::is_aim_assist_rotation_action)
            {
                assisted = Some(packets);
                return Ok(());
            }
            network.send_inventory_packets(packets)
        },
    );
    if let (Some(rotation), Some(packets)) = (aim_rotation, assisted) {
        client_interaction::camera::aim_assist::apply_action_rotation(
            rotation, view, movement, frame.tick,
        );
        movement.hold_release_after_tick(frame.tick, packets);
    }
}

#[cfg(test)]
mod session_tests;
