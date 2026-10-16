//! Actor observation and publication boundaries in the client frame schedule.

use crate::runtime::network::{
    advance_actor_frame, prepare_actor_render_frame, publish_entity_shadows,
};
use bevy::prelude::{App, IntoScheduleConfigs, Update};
use client_presentation::actor_publication::publish_actor_render_frame;
use client_state::ClientFrameSet;

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
