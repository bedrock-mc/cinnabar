//! The shared terrain occlusion preference for main-world and render-world consumers.

use bevy::{prelude::Resource, render::extract_resource::ExtractResource};

/// Disabling occlusion keeps frustum culling and all resident terrain intact.
#[derive(Resource, ExtractResource, Clone, Copy)]
#[extract_app(bevy::render::RenderApp)]
pub struct ChunkOcclusionCulling(pub bool);

impl Default for ChunkOcclusionCulling {
    fn default() -> Self {
        Self(render_api::DEFAULT_CHUNK_OCCLUSION_CULLING)
    }
}
