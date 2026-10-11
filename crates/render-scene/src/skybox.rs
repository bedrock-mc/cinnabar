use bevy::{prelude::Resource, render::extract_resource::ExtractResource};
/// Whether custom skyboxes retain the ordinary Bedrock vertical translation.
#[derive(Resource, ExtractResource, Clone, Copy, Debug, PartialEq, Eq)]
#[extract_app(bevy::render::RenderApp)]
pub struct SkyboxSettings {
    pub native_vertical_offset: bool,
    /// Admission from Fancy Skies and active camera vision effects.
    pub visible: bool,
}

impl Default for SkyboxSettings {
    fn default() -> Self {
        Self {
            native_vertical_offset: assets::DEFAULT_NATIVE_SKYBOX_OFFSET,
            visible: true,
        }
    }
}
