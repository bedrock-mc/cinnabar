use bevy::{prelude::Resource, render::extract_resource::ExtractResource};
use render_model::LightmapInputs;
/// Shared classic lightmap inputs, published with the current environment frame.
#[derive(Resource, ExtractResource, Clone, Copy, Debug, Default, PartialEq)]
#[extract_app(bevy::render::RenderApp)]
pub struct WorldLighting(pub LightmapInputs);

/// A transient personal-mod override; the ordinary environment inputs remain intact.
#[derive(Resource, ExtractResource, Clone, Copy, Debug, Default, PartialEq)]
#[extract_app(bevy::render::RenderApp)]
pub struct WorldFullbright(pub bool);
