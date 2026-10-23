//! Common world, movement, camera and actor frame registrations for platform hosts.
use crate::{
    local_player::{
        publish_local_player_frame, resolve_camera_pose, resolve_local_player_presentation,
    },
    movement::{advance_local_physics, send_movement_prediction_sync},
    runtime::{
        network::{
            PackReload, advance_actor_frame, prepare_actor_render_frame, publish_entity_shadows,
            receive_network_events, reload_resource_packs,
        },
        world::drive_world_stream,
    },
};
use bevy::prelude::*;
use client_presentation::{
    actor_publication::publish_actor_render_frame,
    local_player::{LocalPlayerFrameSet, publish_interaction_origin},
};
use client_state::ClientFrameSet;
use render_scene::ChunkRenderApplySet;
pub(crate) mod interaction;

/// Orders the common frame boundaries before either host adds its systems.
pub(crate) fn configure_client_frame_schedule(app: &mut App) {
    app.init_resource::<client_state::ScreenFocus>()
        .init_resource::<client_state::GameplayScreenControls>();
    app.configure_sets(
        Update,
        (
            ClientFrameSet::RawInput,
            ClientFrameSet::SemanticSample,
            ClientFrameSet::UiAuthority,
            ClientFrameSet::SemanticFinalize,
            ClientFrameSet::Physics,
            ClientFrameSet::Camera,
            ClientFrameSet::Interaction,
            // Packets leave before world, actor and UI preparation, as vanilla flushes before render.
            ClientFrameSet::NetworkSend,
            ClientFrameSet::WorldPublication,
            ClientFrameSet::ActorPreparation,
            ClientFrameSet::UiPreparation,
            ClientFrameSet::ActorFinalization,
            ClientFrameSet::ActorPublication,
            ClientFrameSet::UiPublication,
        )
            .chain(),
    );
}

/// Registers the production actor observation and publication boundaries.
pub(crate) fn configure_actor_render_systems(app: &mut App) {
    app.init_resource::<crate::block_entities::SpawnerDisplayPublisher>()
        .init_resource::<client_presentation::actor_publication::ActorFrameState>()
        .add_systems(
            Update,
            (
                advance_actor_frame,
                crate::runtime::network::publish_local_actor_damage,
            )
                .chain()
                .in_set(ClientFrameSet::ActorPreparation),
        )
        // Picks in Interaction and NetworkSend read this frame's remote actor positions.
        .add_systems(
            Update,
            crate::runtime::network::advance_actor_motion
                .after(ClientFrameSet::Camera)
                .before(ClientFrameSet::Interaction),
        )
        .add_systems(
            Update,
            (
                prepare_actor_render_frame,
                crate::block_entities::prepare_spawner_actors,
            )
                .chain()
                .in_set(ClientFrameSet::ActorFinalization),
        )
        .add_systems(
            Update,
            (publish_actor_render_frame, publish_entity_shadows)
                .chain()
                .in_set(ClientFrameSet::ActorPublication),
        );
}

/// Registers the existing world and player systems once for native and browser hosts.
pub(crate) fn configure_world(app: &mut App) {
    configure_pack_publication(app);
    configure_actor_render_systems(app);
    interaction::configure(app);
    app.add_systems(
        Update,
        advance_local_physics
            .in_set(LocalPlayerFrameSet::Physics)
            .in_set(ClientFrameSet::Physics),
    )
    .add_systems(
        Update,
        send_movement_prediction_sync
            .after(advance_local_physics)
            .in_set(ClientFrameSet::NetworkSend),
    )
    // Look following reads this frame's predicted vehicle heading before the camera resolves.
    .init_resource::<client_presentation::camera::VehicleLookFollow>()
    .add_systems(
        Update,
        crate::runtime::world::vehicle_look::follow_vehicle_look
            .after(advance_local_physics)
            .before(resolve_local_player_presentation),
    )
    .add_systems(
        Update,
        (
            resolve_local_player_presentation,
            client_presentation::local_player_camera_receipt::begin_camera_publication_attempt,
            resolve_camera_pose,
        )
            .chain()
            .in_set(LocalPlayerFrameSet::Camera)
            .in_set(ClientFrameSet::Camera),
    )
    .add_systems(
        Update,
        (
            publish_local_player_frame,
            publish_interaction_origin,
            crate::camera::aim_assist::publish_assisted_interaction,
            crate::camera::aim_highlight::publish,
        )
            .chain()
            .before(client_state::ClientInteractionSet::RetainPick)
            .in_set(LocalPlayerFrameSet::Interaction)
            .in_set(ClientFrameSet::Interaction),
    )
    .add_systems(
        Update,
        drive_world_stream
            .after(receive_network_events)
            .before(ChunkRenderApplySet)
            .in_set(ClientFrameSet::WorldPublication),
    );
}

/// Publishes accepted packs after ingress and before world, UI and render consumers.
pub(crate) fn configure_pack_publication(app: &mut App) {
    app.init_resource::<PackReload>().add_systems(
        Update,
        reload_resource_packs
            .after(receive_network_events)
            .before(ClientFrameSet::WorldPublication)
            .before(ClientFrameSet::UiPublication)
            .before(ChunkRenderApplySet),
    );
}
