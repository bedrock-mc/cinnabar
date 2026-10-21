//! Middle-click picking asks the server for the selected block or actor.

use crate::runtime::network::resource_packs::PackApplication;
use bevy::{
    ecs::system::SystemParam,
    prelude::{Query, Res, ResMut, Window, With},
    window::PrimaryWindow,
};
use client_runtime::network::NetworkHandle;
use protocol::PlayerGameMode;

use client_shell::ui_resources::UiRuntimeResource;
use {
    crate::{interaction_authority::observe_block_ray_using, runtime::world::ClientWorld},
    client_interaction::{
        mining::hand_interaction_selection, movement::PhysicsCollisionRegistries,
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
    network: Res<'w, NetworkHandle<PackApplication>>,
}

/// A pick request retains its original target while waiting for transport.
#[derive(Clone, PartialEq)]
pub(crate) struct PickAttempt {
    packet: protocol::Packet,
    lifetime: Option<actor_animation::ActorLifetimeId>,
    world: sim::WorldCollisionIdentity,
}

/// Resolves a pick with the same binding, screen, range and actor competition as ordinary input.
fn observe(
    player_runtime: &client_state::PlayerRuntime,
    context: &PickBlockContext,
    ui: &UiRuntimeResource,
    pressed: bool,
    mode: semantic_input::InputMode,
    identity: (
        semantic_input::identity::InputAuthorityGeneration,
        semantic_input::identity::InputSequence,
    ),
    origin: &InteractionOriginSnapshot,
) -> Option<PickAttempt> {
    if !pressed
        || context.focus.absorbs_input
        || !context.windows.single().is_ok_and(|window| window.focused)
        || player_runtime
            .facts
            .player_game_mode()
            .is_some_and(|mode| !mode.shows_hotbar())
    {
        return None;
    }
    let selection = hand_interaction_selection(player_runtime)?;
    let input_mode = protocol_input_mode(mode);
    let reach = if player_runtime.facts.player_game_mode() == Some(PlayerGameMode::Creative) {
        creative_reach(input_mode)
    } else {
        survival_reach(input_mode)
    };
    let Ok(observed) = observe_block_ray_using(
        origin,
        ui,
        &context.client_world,
        &context.collisions,
        selection,
        player_runtime.facts.player_game_mode(),
        false,
        (
            input_mode,
            reach,
            identity,
            simulation_clock::identity::PositionAuthorityGeneration::new(0),
        ),
        |world, origin, direction, reach| {
            world.block_interaction_ray_current(origin, direction, reach)
        },
    ) else {
        return None;
    };
    if let (Some(stream), Some(ray)) = (context.client_world.stream.as_ref(), origin.outbound_ray())
        && let Some(unique_id) = gameplay::melee::actor_pick_unique_id(
            stream.authority(),
            ray.origin().to_array(),
            ray.direction().to_array(),
            reach,
            observed
                .value
                .as_ref()
                .map(|observed| observed.hit_distance()),
            ui.gameplay_hud().mount_unique_id(),
        )
    {
        return Some(PickAttempt {
            packet: protocol::actor_pick_request_packet(unique_id, false),
            lifetime: stream
                .authority()
                .remote_actors()
                .find(|actor| actor.unique_id == unique_id)
                .and_then(|actor| stream.authority().actor_lifetime(actor.runtime_id)),
            world: observed.identity,
        });
    }
    let hit = observed.value?;
    Some(PickAttempt {
        packet: protocol::block_pick_request_packet(hit.target.position, false),
        lifetime: None,
        world: observed.identity,
    })
}

/// Sends an ordinary pick once; ordered input is owned by the interaction cursor.
pub(crate) fn produce_pick_block(
    player_runtime: bevy::prelude::Res<client_state::PlayerRuntime>,
    context: PickBlockContext,
    ui: ResMut<UiRuntimeResource>,
) {
    if context
        .input
        .snapshot()
        .is_some_and(|input| input.ordered.is_some())
    {
        return;
    }
    let Some(snapshot) = context.input.snapshot() else {
        return;
    };
    if let Some(attempt) = observe(
        &player_runtime,
        &context,
        &ui,
        context.controls.pick_block_pressed
            || context
                .input
                .phase(semantic_input::Action::PickItem)
                .pressed,
        snapshot.input_mode,
        (snapshot.authority_generation, snapshot.frame_sequence),
        &context.origin,
    ) {
        let _ = context.network.send_inventory_packet(attempt.packet);
    }
}

/// Attempts one ordered pick with the origin published after this frame's camera update.
pub(crate) fn ordered_pick(
    bevy::prelude::In(input): bevy::prelude::In<crate::ordered_interactions::InteractionInput>,
    player: bevy::prelude::Res<client_state::PlayerRuntime>,
    context: PickBlockContext,
    ui: ResMut<UiRuntimeResource>,
) {
    if let Some(pick) = observe(
        &player,
        &context,
        &ui,
        input.edge.pressed,
        input.context.mode,
        input.context.identity,
        &input.context.origin,
    ) {
        let _ = context.network.send_inventory_packet(pick.packet);
    }
}
