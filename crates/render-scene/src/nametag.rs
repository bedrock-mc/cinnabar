use bevy::{prelude::Resource, render::extract_resource::ExtractResource};
use render_model::NametagScene;
/// Main-world holder of this frame's [`NametagScene`], cloned into the render world.
#[derive(Resource, ExtractResource, Clone, Debug, Default, Deref, DerefMut)]
#[extract_app(bevy::render::RenderApp)]
pub struct NametagSceneResource(pub NametagScene);
