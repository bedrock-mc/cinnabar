//! Middle-click pick block: asks the server to put the targeted block in hand.

use bevy::{
    ecs::system::SystemParam,
    prelude::{Query, Res, ResMut, Window, With},
    window::PrimaryWindow,
};
use protocol::PlayerGameMode;

use client_ui::ui_runtime::UiRuntime;
use {
    crate::{
        interaction_authority::observe_block,
        mining::hand_interaction_selection,
        movement::PhysicsCollisionRegistries,
        runtime::{network::NetworkHandle, world::ClientWorld},
    },
    client_presentation::local_player::InteractionOriginSnapshot,
    client_state::SemanticInputSnapshot,
    gameplay::mining::{creative_reach, protocol_input_mode, survival_reach},
};

#[derive(SystemParam)]
pub(crate) struct PickBlockContext<'w, 's> {
    input: Res<'w, SemanticInputSnapshot>,
    origin: Res<'w, InteractionOriginSnapshot>,
    focus: Res<'w, client_state::ScreenFocus>,
    controls: Res<'w, client_state::GameplayScreenControls>,
    windows: Query<'w, 's, &'static Window, With<PrimaryWindow>>,
    client_world: Res<'w, ClientWorld>,
    collisions: Res<'w, PhysicsCollisionRegistries>,
    network: Res<'w, NetworkHandle>,
}

/// Sends one block-pick request per middle-click on a block in reach.
pub(crate) fn produce_pick_block(
    player_runtime: bevy::prelude::Res<client_state::PlayerRuntime>,
    context: PickBlockContext,
    ui: ResMut<UiRuntime>,
) {
    if !context.controls.pick_block_pressed
        || context.focus.absorbs_input
        || !context.windows.single().is_ok_and(|window| window.focused)
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
