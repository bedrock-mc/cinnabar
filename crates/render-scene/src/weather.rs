use bevy::{
    prelude::{Deref, DerefMut, Resource},
    render::extract_resource::ExtractResource,
};
use render_model::{OcclusionGrid, PrecipitationLayerRecord};
use std::sync::Arc;

/// Current rain and snow intensities shared by weather presentation systems.
#[derive(Resource, Clone, Copy, Debug, Default, PartialEq, Deref, DerefMut)]
pub struct PrecipitationMixResource(pub render_model::PrecipitationMix);

/// Decoded weather sheet and End sky, when the optional carrier is present.
#[derive(Resource, ExtractResource, Clone, Default)]
#[extract_app(bevy::render::RenderApp)]
pub struct WeatherTextureAssets {
    textures: Option<Arc<assets::WeatherTextures>>,
    identity: [u8; 32],
}

impl WeatherTextureAssets {
    #[must_use]
    pub fn new(textures: Arc<assets::WeatherTextures>, identity: [u8; 32]) -> Self {
        Self {
            textures: Some(textures),
            identity,
        }
    }

    #[must_use]
    pub fn textures(&self) -> Option<&Arc<assets::WeatherTextures>> {
        self.textures.as_ref()
    }

    #[must_use]
    pub const fn identity(&self) -> [u8; 32] {
        self.identity
    }
}

/// Precipitation state consumed by the render world.
#[derive(Resource, ExtractResource, Clone, Debug, Default, PartialEq)]
#[extract_app(bevy::render::RenderApp)]
pub struct PrecipitationScene {
    pub layers: Vec<PrecipitationLayerRecord>,
    pub forward_offset: [f32; 3],
    pub occlusion: Arc<OcclusionGrid>,
    /// Bumped whenever `occlusion` changes, so the GPU copy uploads only then.
    pub occlusion_generation: u64,
}

/// Main-world queue of splash positions for the particle system to drain each frame.
#[derive(Resource, Debug, Default)]
pub struct RainSplashQueue {
    pub positions: Vec<[f32; 3]>,
}
