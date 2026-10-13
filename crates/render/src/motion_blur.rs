//! Optional exposure with depth-derived world motion and rendered actor velocities.

use render_scene::CameraMotionBlur;

pub(crate) mod actor_history;
#[cfg(test)]
mod extraction_tests;
pub(crate) mod graph;
pub(crate) mod history;
mod pipeline;
mod prepare;
#[cfg(test)]
mod raster_tests;
#[cfg(test)]
mod resource_tests;
mod spread;
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

const SHADER: Handle<Shader> = uuid_handle!("084d4ee2-222c-4990-9c05-e303b8e8081a");
const MSAA_SHADER: Handle<Shader> = uuid_handle!("935fcd71-f7d0-43b9-9b70-374366e0b3c3");
const SPREAD_SINGLE: Handle<Shader> = uuid_handle!("d236b2ef-5874-49f2-9158-57f8d58a6759");
const SPREAD_MULTI: Handle<Shader> = uuid_handle!("4816862b-18c7-48c7-90d6-15e7ac274687");

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
        load_internal_asset!(app, SPREAD_SINGLE, "motion_blur/spread.wgsl", |_, path| {
            crate::shader_safety::from_wgsl(spread::source(false, false), path)
        });
        load_internal_asset!(app, SPREAD_MULTI, "motion_blur/spread.wgsl", |_, path| {
            crate::shader_safety::from_wgsl(spread::source(true, false), path)
        });
        crate::pipeline_warmup::register::<pipeline::BlurPipeline>(app);
        app.sub_app_mut(RenderApp)
            .add_systems(ExtractSchedule, extract_settings)
            .add_systems(RenderStartup, pipeline::init)
            .add_systems(
                Render,
                (
                    graph::sync_graph.in_set(RenderSystems::Prepare),
                    prepare::prepare_views
                        .in_set(RenderSystems::PrepareBindGroups)
                        .after(crate::actor_render::prepare_actor_bind_group),
                ),
            );
    }
}

fn shader_source(source: &str, multisampled: bool) -> String {
    if multisampled {
        source
            .replace(
                "return textureLoad(depth, pixel, 0);",
                r#"var nearest = 0.0;
    for (var sample = 0u; sample < textureNumSamples(depth); sample++) {
        nearest = max(nearest, textureLoad(depth, pixel, i32(sample)));
    }
    return nearest;"#,
            )
            .replace("texture_depth_2d", "texture_depth_multisampled_2d")
            .replace(
                "var actor_velocity: texture_2d<f32>",
                "var actor_velocity: texture_multisampled_2d<f32>",
            )
            .replace(
                "return textureLoad(actor_velocity, pixel, 0);",
                r#"var velocity = vec2<f32>(0.0);
    var covered = 0u;
    let samples = textureNumSamples(actor_velocity);
    for (var sample = 0u; sample < samples; sample++) {
        let motion = textureLoad(actor_velocity, pixel, i32(sample));
        if motion.z >= 0.5 {
            velocity += motion.xy;
            covered += 1u;
        }
    }
    if covered == 0u {
        return vec4<f32>(0.0);
    }
    return vec4<f32>(velocity / f32(covered), 1.0, f32(covered) / f32(samples));"#,
            )
    } else {
        source.to_owned()
    }
}

/// All transparent passes use the same readiness decision, including stationary and reset frames.
pub(crate) fn applies(world: &World, entity: Entity) -> bool {
    use bevy::render::{
        render_resource::PipelineCache,
        view::{ViewDepthStencilTexture, ViewTarget},
    };
    let (Some(state), Some(target), Some(depth), Some(cache)) = (
        world.get::<prepare::BlurView>(entity),
        world.get::<ViewTarget>(entity),
        world.get::<ViewDepthStencilTexture>(entity),
        world.get_resource::<PipelineCache>(),
    ) else {
        return false;
    };
    state.active
        && cache.get_render_pipeline(state.pipeline).is_some()
        && state
            .binding(
                target.main_texture_view().id(),
                crate::scene_sampling::view_depth(depth).id(),
            )
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
