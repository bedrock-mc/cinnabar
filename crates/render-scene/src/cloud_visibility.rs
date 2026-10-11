use bevy::{prelude::Resource, render::extract_resource::ExtractResource};
/// The user's cloud visibility preference, copied into the render world each frame.
#[derive(Resource, ExtractResource, Clone, Copy)]
#[extract_app(bevy::render::RenderApp)]
pub struct CloudVisibility(pub bool);

impl Default for CloudVisibility {
    /// Clouds remain visible until the app supplies its saved preference.
    fn default() -> Self {
        Self(true)
    }
}
