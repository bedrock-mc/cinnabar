//! Retained server debug geometry: shared meshes, stable instance slots and GPU visibility.

mod gpu;
mod mesh;
mod pipeline;
#[cfg(test)]
mod tests;

use bevy::{
    asset::{load_internal_asset, uuid_handle},
    prelude::*,
    render::{
        Render, RenderApp, RenderStartup, RenderSystems,
        extract_resource::{ExtractResource, ExtractResourcePlugin},
        render_phase::AddRenderCommand,
    },
};
use render_model::primitive_shapes::PrimitiveShapeStore;
use std::sync::{Arc, Mutex};

/// Shared retained state; extraction copies only the handle and the current clock/dimension.
#[derive(Resource, Clone, ExtractResource)]
pub struct PrimitiveShapesScene {
    pub store: Arc<Mutex<PrimitiveShapeStore>>,
    pub clock: f32,
    pub dimension: i32,
    pub render_distance: f32,
}

impl Default for PrimitiveShapesScene {
    /// Starts with no geometry or GPU publications.
    fn default() -> Self {
        Self {
            store: Arc::new(Mutex::new(PrimitiveShapeStore::default())),
            clock: 0.0,
            dimension: 0,
            render_distance: 0.0,
        }
    }
}

/// Installs the network primitive renderer independently of actor rendering.
pub struct PrimitiveShapesRenderPlugin;

const SHADER: Handle<Shader> = uuid_handle!("361a6dc1-b0e8-41ee-a5ac-94b3e8d6cc48");

impl Plugin for PrimitiveShapesRenderPlugin {
    /// Keeps the domain store in the main world and uploads changes in the render world.
    fn build(&self, app: &mut App) {
        app.init_resource::<PrimitiveShapesScene>()
            .add_plugins(ExtractResourcePlugin::<PrimitiveShapesScene>::default());
        load_internal_asset!(app, SHADER, "primitive_shapes/shapes.wgsl", shader);
        let Some(render) = app.get_sub_app_mut(RenderApp) else {
            return;
        };
        render.init_resource::<pipeline::ShapePipeline>()
            .add_render_command::<bevy::core_pipeline::core_3d::Transparent3d, pipeline::DrawShapes>()
            .add_systems(RenderStartup, gpu::init)
            .add_systems(Render, (
                gpu::prepare.in_set(RenderSystems::PrepareResources),
                pipeline::prepare_bind_groups.in_set(RenderSystems::PrepareBindGroups),
                pipeline::queue.run_if(crate::panorama::world_passes_enabled).in_set(RenderSystems::Queue),
            ));
    }
}

/// Injects the existing nametag constants into the shared debug-text shader.
fn shader(raw: &str, path: impl Into<String>) -> Shader {
    let source = raw
        .replace(
            "ARROW_KIND_VALUE",
            &(render_api::primitive_shapes::PrimitiveShapeKind::Arrow as u32).to_string(),
        )
        .replace(
            "BOX_KIND_VALUE",
            &(render_api::primitive_shapes::PrimitiveShapeKind::Box as u32).to_string(),
        )
        .replace(
            "ALL_DIMENSIONS_VALUE",
            &render_api::primitive_shapes::PRIMITIVE_ALL_DIMENSIONS.to_string(),
        )
        .replace(
            "ARROW_MIN_LENGTH_SQUARED_VALUE",
            &render_api::primitive_shapes::PRIMITIVE_ARROW_MIN_LENGTH_SQUARED.to_string(),
        )
        .replace(
            "TEXT_SCALE_VALUE",
            &render_model::NAMETAG_BLOCKS_PER_FONT_PIXEL.to_string(),
        )
        .replace(
            "ACOS_LINEAR_VALUE",
            &render_model::NAMETAG_ACOS_LINEAR.to_string(),
        )
        .replace(
            "ACOS_CUBIC_VALUE",
            &render_model::NAMETAG_ACOS_CUBIC.to_string(),
        )
        .replace(
            "HORIZONTAL_ZERO_VALUE",
            &render_model::NAMETAG_HORIZONTAL_ZERO.to_string(),
        );
    crate::shader_safety::from_wgsl(source, path)
}

/// Identifies the debug draw family for encoded-color transparency routing.
pub(crate) fn draw_function(world: &World) -> Option<bevy::render::render_phase::DrawFunctionId> {
    world.get_resource::<bevy::render::render_phase::DrawFunctions<bevy::core_pipeline::core_3d::Transparent3d>>()?.read().get_id::<pipeline::DrawShapes>()
}
