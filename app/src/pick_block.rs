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
    crate::{interaction_authority::observe_block_ray, runtime::world::ClientWorld},
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
#[derive(Clone)]
pub(crate) struct PickAttempt {
    packet: protocol::Packet,
    lifetime: Option<actor_animation::ActorLifetimeId>,
}

/// Resolves a pick with the same binding, screen, range and actor competition as ordinary input.
fn observe(
    player_runtime: &client_state::PlayerRuntime,
    context: &PickBlockContext,
    ui: &UiRuntimeResource,
) -> Option<PickAttempt> {
    if !(context.controls.pick_block_pressed
        || context
            .input
            .phase(semantic_input::Action::PickItem)
            .pressed)
        || context.focus.absorbs_input
        || !context.windows.single().is_ok_and(|window| window.focused)
        || player_runtime
            .facts
            .player_game_mode()
            .is_some_and(|mode| !mode.shows_hotbar())
    {
        return None;
    }
    let (Some(snapshot), Some(selection)) = (
        context.input.snapshot(),
        hand_interaction_selection(&player_runtime),
    ) else {
        return None;
    };
    let input_mode = protocol_input_mode(snapshot.input_mode);
    let reach = if player_runtime.facts.player_game_mode() == Some(PlayerGameMode::Creative) {
        creative_reach(input_mode)
    } else {
        survival_reach(input_mode)
    };
    let Ok(observed) = observe_block_ray(
        &context.origin,
        &ui,
        &context.client_world,
        &context.collisions,
        selection,
        player_runtime.facts.player_game_mode(),
        (
            input_mode,
            reach,
            (snapshot.authority_generation, snapshot.frame_sequence),
            simulation_clock::identity::PositionAuthorityGeneration::new(0),
        ),
    ) else {
        return None;
    };
    if let (Some(stream), Some(ray)) = (
        context.client_world.stream.as_ref(),
        context.origin.outbound_ray(),
    ) && let Some(unique_id) = gameplay::melee::actor_pick_unique_id(
        stream.authority(),
        ray.origin().to_array(),
        ray.direction().to_array(),
        reach,
        observed.as_ref().map(|observed| observed.hit_distance()),
        ui.gameplay_hud().mount_unique_id(),
    ) {
        return Some(PickAttempt {
            packet: protocol::actor_pick_request_packet(unique_id, false),
            lifetime: stream
                .authority()
                .remote_actors()
                .find(|actor| actor.unique_id == unique_id)
                .and_then(|actor| stream.authority().actor_lifetime(actor.runtime_id)),
        });
    }
    let Some(observed) = observed else {
        return None;
    };
    Some(PickAttempt {
        packet: protocol::block_pick_request_packet(observed.target.position, false),
        lifetime: None,
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
    if let Some(attempt) = observe(&player_runtime, &context, &ui) {
        let _ = context.network.send_inventory_packet(attempt.packet);
    }
}

/// Attempts the same ordered pick until admission, without redirecting a reused actor ID.
pub(crate) fn step_ordered_pick(
    bevy::prelude::In(mut attempt): bevy::prelude::In<Option<PickAttempt>>,
    player: bevy::prelude::Res<client_state::PlayerRuntime>,
    context: PickBlockContext,
    ui: ResMut<UiRuntimeResource>,
) -> crate::ordered_interactions::Step<Option<PickAttempt>> {
    use crate::ordered_interactions::{Progress, Step};
    if attempt.is_none() {
        attempt = observe(&player, &context, &ui);
    }
    let Some(pick) = attempt.as_ref() else {
        return Step::new(attempt, Progress::Complete);
    };
    if let Some(lifetime) = pick.lifetime {
        let valid = context.client_world.stream.as_ref().is_some_and(|stream| {
            stream.authority().actor_lifetime(lifetime.runtime_id) == Some(lifetime)
        });
        if !valid {
            return Step::new(attempt, Progress::Complete);
        }
    }
    let progress = match context.network.send_inventory_packet(pick.packet.clone()) {
        Err(client_session::PacketSendError::Full(_)) => Progress::Pending,
        _ => Progress::Complete,
    };
    Step::new(attempt, progress)
}
