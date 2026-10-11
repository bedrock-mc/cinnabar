//! Resource adapters for engine-free model data.
use bevy::prelude::{Deref, DerefMut, Resource};

/// Startup artwork shared by actor presentation systems.
#[derive(Clone, Debug, Default, Resource, Deref, DerefMut)]
pub struct ActorArtworkResource(pub render_model::ActorArtworkPages);

/// Validated first-person mesh available to presentation systems.
#[derive(Clone, Debug, Resource, Deref, DerefMut)]
pub struct ViewmodelGeometryResource(pub render_model::ViewmodelGeometry);
