use bevy::{prelude::Resource, render::extract_resource::ExtractResource};
use render_model::EntityShadowFrame;
/// Main-world holder of this frame's casters, cloned into the render world.
#[derive(Resource, ExtractResource, Clone, Default, Debug)]
#[extract_app(bevy::render::RenderApp)]
pub struct EntityShadowScene(pub EntityShadowFrame);
