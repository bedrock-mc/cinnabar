//! Optional camera exposure, using scene depth without object motion vectors.

#[cfg(test)]
mod extraction_tests;
pub(crate) mod graph;
mod history;
mod pipeline;
mod prepare;
#[cfg(test)]
mod raster_tests;
#[cfg(test)]
mod resource_tests;
#[cfg(test)]
mod tests;

use bevy::{
    asset::{load_internal_asset, uuid_handle},
    prelude::*,
    render::{
        Extract, ExtractSchedule, Render, RenderApp, RenderStartup, RenderSystems,
        sync_world::RenderEntity,
    },
};

/// Presentation-only exposure; removing this component releases the per-view blur resources.
#[derive(Component, Clone, Copy, Debug, PartialEq)]
pub struct CameraMotionBlur {
    pub exposure_seconds: f32,
    pub samples: u32,
    pub reset_epoch: u64,
    pub delta_seconds: f32,
}

const SHADER: Handle<Shader> = uuid_handle!("084d4ee2-222c-4990-9c05-e303b8e8081a");
const MSAA_SHADER: Handle<Shader> = uuid_handle!("935fcd71-f7d0-43b9-9b70-374366e0b3c3");

/// Installs the effect's pipelines; graph nodes and per-view allocations exist only while enabled.
pub struct CameraMotionBlurPlugin;

impl Plugin for CameraMotionBlurPlugin {
    fn build(&self, app: &mut App) {
        if app.get_sub_app(RenderApp).is_none() {
            return;
        }
        load_internal_asset!(app, SHADER, "motion_blur.wgsl", |source, path| {
            crate::shader_safety::from_wgsl(shader_source(source, false), path)
        });
        load_internal_asset!(app, MSAA_SHADER, "motion_blur.wgsl", |source, path| {
            crate::shader_safety::from_wgsl(shader_source(source, true), path)
        });
        crate::pipeline_warmup::register::<pipeline::BlurPipeline>(app);
        app.sub_app_mut(RenderApp)
            .add_systems(ExtractSchedule, extract_settings)
            .add_systems(RenderStartup, pipeline::init)
            .add_systems(
                Render,
                (
                    graph::sync_graph.in_set(RenderSystems::Prepare),
                    prepare::prepare_views.in_set(RenderSystems::PrepareBindGroups),
                ),
            );
    }
}

fn shader_source(source: &str, multisampled: bool) -> String {
    if multisampled {
        source.replace("texture_depth_2d", "texture_depth_multisampled_2d")
    } else {
        source.to_owned()
    }
}

/// All transparent passes use the same readiness decision, including stationary and reset frames.
pub(crate) fn applies(world: &World, entity: Entity) -> bool {
    use bevy::render::{
        render_resource::PipelineCache,
        view::{ViewDepthTexture, ViewTarget},
    };
    let (Some(state), Some(target), Some(depth), Some(cache)) = (
        world.get::<prepare::BlurView>(entity),
        world.get::<ViewTarget>(entity),
        world.get::<ViewDepthTexture>(entity),
        world.get_resource::<PipelineCache>(),
    ) else {
        return false;
    };
    state.active
        && cache.get_render_pipeline(state.pipeline).is_some()
        && state
            .binding(target.main_texture_view().id(), depth.view().id())
            .is_some()
}

type MainCameras<'w, 's> =
    Query<'w, 's, (RenderEntity, Option<&'static CameraMotionBlur>), With<Camera3d>>;

/// Camera entities are already synchronized; setting changes retain their scene and depth targets.
fn extract_settings(
    mut commands: Commands,
    cameras: Extract<MainCameras>,
    mut settings: Query<&mut CameraMotionBlur>,
) {
    for (entity, desired) in &cameras {
        match (settings.get_mut(entity), desired) {
            (Ok(mut current), Some(desired)) => {
                if *current != *desired {
                    *current = *desired;
                }
            }
            (Ok(_), None) => {
                commands.entity(entity).remove::<CameraMotionBlur>();
            }
            (Err(_), Some(desired)) => {
                commands.entity(entity).insert(*desired);
            }
            (Err(_), None) => {}
        }
    }
}
