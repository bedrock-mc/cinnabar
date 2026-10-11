//! Presentation resources shared by actor preparation and publication.
use crate::actor_publication::{ActorFramePartialTick, ActorFrameState, PreparedActorPublication};
use bevy::prelude::{App, Plugin};

/// Installs retained actor presentation state; the host sets frame ordering explicitly.
pub struct ClientPresentationPlugin;

impl Plugin for ClientPresentationPlugin {
    /// Creates each retained presentation resource once without advancing any simulation.
    fn build(&self, app: &mut App) {
        // Starts reading seat layouts now, so the first session's frames find them ready.
        let _ = crate::seat_defaults::seat_defaults();
        app.init_resource::<ActorFrameState>()
            .init_resource::<ActorFramePartialTick>()
            .init_resource::<PreparedActorPublication>();
    }
}
