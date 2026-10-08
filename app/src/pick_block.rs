//! Middle-click pick block: asks the server to put the targeted block in hand.

use bevy::{
    ecs::system::SystemParam,
    prelude::{ButtonInput, KeyCode, MouseButton, Query, Res, ResMut, Window, With},
    window::PrimaryWindow,
};
use protocol::PlayerGameMode;

use crate::{
    interaction_authority::observe_block,
    local_player::InteractionOriginSnapshot,
    menu::MenuRuntime,
    mining::{creative_reach, hand_interaction_selection, protocol_input_mode, survival_reach},
    movement::PhysicsCollisionRegistries,
    runtime::{network::NetworkHandle, world::ClientWorld},
    semantic_controls::SemanticInputSnapshot,
};
use client_ui::ui_runtime::UiRuntime;

#[derive(SystemParam)]
pub(crate) struct PickBlockContext<'w, 's> {
    gamepads: Query<'w, 's, &'static bevy::input::gamepad::Gamepad>,
    input: Res<'w, SemanticInputSnapshot>,
    origin: Res<'w, InteractionOriginSnapshot>,
    menu: Res<'w, MenuRuntime>,
    presentation: Option<Res<'w, client_ui::ui_runtime::presentation::UiPresentationRuntime>>,
    windows: Query<'w, 's, &'static Window, With<PrimaryWindow>>,
    client_world: Res<'w, ClientWorld>,
    collisions: Res<'w, PhysicsCollisionRegistries>,
    keys: Res<'w, ButtonInput<KeyCode>>,
    mouse: Res<'w, ButtonInput<MouseButton>>,
    network: Res<'w, NetworkHandle>,
}

/// Sends one block-pick request per middle-click on a block in reach.
pub(crate) fn produce_pick_block(
    player_runtime: bevy::prelude::Res<crate::player_runtime::PlayerRuntime>,
    context: PickBlockContext,
    ui: ResMut<UiRuntime>,
) {
    if !(crate::menu::settings_options::binding_gamepad(
        Some(&context.menu),
        "key.pickItem",
        &context.gamepads,
    ) || crate::menu::settings_options::binding_pressed(
        Some(&context.menu),
        "key.pickItem",
        &context.keys,
        &context.mouse,
    )) || crate::screen_policy::absorbs_input(
        &player_runtime,
        Some(&ui),
        Some(&context.menu),
        context.presentation.as_deref(),
    ) || !context.windows.single().is_ok_and(|window| window.focused)
        || player_runtime
            .facts
            .player_game_mode()
            .is_some_and(|mode| !mode.shows_hotbar())
    {
        return;
    }
    let (Some(snapshot), Some(selection)) = (
        context.input.snapshot(),
        hand_interaction_selection(&player_runtime),
    ) else {
        return;
    };
    let input_mode = protocol_input_mode(snapshot.input_mode);
    let reach = if player_runtime.facts.player_game_mode() == Some(PlayerGameMode::Creative) {
        creative_reach(input_mode)
    } else {
        survival_reach(input_mode)
    };
    let Some(observed) = observe_block(
        &context.origin,
        &ui,
        &context.client_world,
        &context.collisions,
        selection,
        (
            input_mode,
            reach,
            (snapshot.authority_generation, snapshot.frame_sequence),
            0,
        ),
    ) else {
        return;
    };
    // A full queue drops the pick; the player simply clicks again.
    let _ = context
        .network
        .send_inventory_packet(protocol::block_pick_request_packet(
            observed.target.position,
            false,
        ));
}
