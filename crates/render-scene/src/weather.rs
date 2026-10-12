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
    textures: Option<Arc<assets::weather_textures::WeatherTextures>>,
    end_sky: Option<Arc<assets::weather_textures::WeatherImage>>,
    identity: [u8; 32],
}

impl WeatherTextureAssets {
    #[must_use]
    pub fn new(
        textures: Arc<assets::weather_textures::WeatherTextures>,
        identity: [u8; 32],
    ) -> Self {
        Self {
            textures: Some(textures),
            end_sky: None,
            identity,
        }
    }

    #[must_use]
    pub fn textures(&self) -> Option<&Arc<assets::weather_textures::WeatherTextures>> {
        self.textures.as_ref()
    }

    /// Applies a bounded decoded End sky without changing startup carrier validation.
    pub fn with_end_sky(
        mut self,
        image: assets::weather_textures::WeatherImage,
        identity: [u8; 32],
    ) -> Self {
        self.end_sky = Some(Arc::new(image));
        self.identity = identity;
        self
    }

    /// Resolves the runtime override first, then the immutable startup End sky.
    pub fn end_sky(&self) -> Option<&assets::weather_textures::WeatherImage> {
        self.end_sky
            .as_deref()
            .or_else(|| self.textures.as_ref().map(|textures| &textures.end_sky))
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
