//! Presentation resources shared by actor preparation and publication.
use crate::actor_publication::{ActorFramePartialTick, ActorFrameState, PreparedActorPublication};
use bevy::prelude::*;

/// Installs retained actor presentation state; the host sets frame ordering explicitly.
pub struct ClientPresentationPlugin;

impl Plugin for ClientPresentationPlugin {
    /// Creates each retained presentation resource once without advancing any simulation.
    fn build(&self, app: &mut App) {
        app.init_resource::<ActorFrameState>()
            .init_resource::<ActorFramePartialTick>()
            .init_resource::<PreparedActorPublication>();
    }
}
