//! Actor publication and runtime frame-system registration.

use crate::{
    environment::{
        update_atmosphere_frame, update_lightning, update_precipitation_scene,
        update_seasonal_foliage,
    },
    present_mode::apply_present_mode,
    runtime::{
        network::{
            advance_actor_frame, prepare_actor_render_frame, publish_entity_shadows,
            receive_network_events, resource_packs::PackApplication,
        },
        shutdown::{exit_on_fatal_runtime_error, exit_on_window_close_requested},
        telemetry::{publish_runtime_stage_profile, record_metrics, update_visibility_diagnostics},
        visibility::{
            apply_added_chunk_visibility, refresh_cave_visibility, remove_chunk_visibility,
        },
        world::{drive_world_stream, update_camera_medium},
    },
    ui_runtime::{flush_chat_network, flush_server_form_network},
};
use bevy::prelude::{App, IntoScheduleConfigs, Update};
use client_presentation::{
    actor_publication::publish_actor_render_frame, camera::FlyCameraUpdateSet,
    local_player::LocalPlayerFrameSet,
};
use client_state::ClientFrameSet;
use render_scene::ChunkRenderApplySet;
#[cfg(feature = "acceptance")]
use {
    crate::acceptance::{model_witness::drive_model_witness, world_ready::emit_world_ready},
    acceptance::{
        model_witness::poll_model_witness_request,
        transparent_witness::poll_transparent_witness_request,
    },
};

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

/// Registers runtime publication after camera and render-mode authority updates.
pub(crate) fn configure_client_runtime_frame_systems(app: &mut App) {
    app.add_plugins(client_runtime::ClientRuntimePlugin::<PackApplication>::default());
    crate::runtime::world::configure_world_service(app);
    app.add_observer(apply_added_chunk_visibility)
        .add_observer(remove_chunk_visibility)
        .configure_sets(
            Update,
            (
                LocalPlayerFrameSet::Physics,
                LocalPlayerFrameSet::Camera,
                LocalPlayerFrameSet::Interaction,
            )
                .chain()
                .after(FlyCameraUpdateSet)
                .after(crate::render_mode::RenderModeUpdateSet),
        )
        .configure_sets(
            Update,
            client_state::RuntimeFrameSet::PublicationBudget
                .before(receive_network_events)
                .before(drive_world_stream),
        )
        .add_systems(
            Update,
            (
                exit_on_window_close_requested,
                flush_chat_network.before(ClientFrameSet::UiPreparation),
                flush_server_form_network.in_set(ClientFrameSet::NetworkSend),
                exit_on_fatal_runtime_error,
                #[cfg(feature = "acceptance")]
                poll_transparent_witness_request,
                #[cfg(feature = "acceptance")]
                poll_model_witness_request,
                update_camera_medium,
                update_atmosphere_frame,
                update_seasonal_foliage,
                crate::environment::log_world_lighting,
                update_precipitation_scene,
                update_lightning,
                refresh_cave_visibility,
                update_visibility_diagnostics.after(ChunkRenderApplySet),
                #[cfg(feature = "acceptance")]
                emit_world_ready,
                #[cfg(feature = "acceptance")]
                drive_model_witness,
                apply_present_mode,
                record_metrics,
                publish_runtime_stage_profile,
            )
                .chain()
                .after(FlyCameraUpdateSet),
        );
}
