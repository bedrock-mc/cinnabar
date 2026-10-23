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

/// The user's configured cloud render distance in chunks (8..=128), copied into the render world.
#[derive(Resource, ExtractResource, Clone, Copy, Debug, PartialEq, Eq)]
#[extract_app(bevy::render::RenderApp)]
pub struct CloudDistanceChunks(pub u16);

impl Default for CloudDistanceChunks {
    fn default() -> Self {
        Self(64)
    }
}
