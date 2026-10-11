use bevy::{
    prelude::{Deref, DerefMut, Resource},
    render::extract_resource::ExtractResource,
};
use render_model::{UiRenderScene, UiRenderStats};
/// Main-world holder of the published [`UiRenderScene`], cloned into the render world.
#[derive(Resource, ExtractResource, Clone, Debug, Default, Deref, DerefMut)]
#[extract_app(bevy::render::RenderApp)]
pub struct UiRenderSceneResource(pub UiRenderScene);

/// The [`UiRenderStats`] handle both worlds share.
#[derive(Resource, Clone, Debug, Default, Deref, DerefMut)]
pub struct UiRenderStatsResource(pub UiRenderStats);
