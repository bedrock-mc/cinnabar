//! Draws shadow volumes over opaque geometry, multiplying each covered sample once.
//! Rules: `docs/reference/entity-shadows.md`.
use std::num::NonZeroU64;

use bevy::image::BevyDefault;
#[cfg(test)]
use bevy::prelude::{IntoSystem, System};
use bevy::{
    asset::{load_internal_asset, uuid_handle},
    core_pipeline::core_3d::graph::{Core3d, Node3d},
    ecs::query::QueryItem,
    mesh::VertexBufferLayout,
    prelude::{
        App, Camera3d, Commands, Component, Entity, Handle, IntoScheduleConfigs, Last, Msaa,
        Plugin, Query, Res, ResMut, Resource, Result, Shader, With, World, default,
    },
    render::{
        Render, RenderApp, RenderSystems,
        camera::ExtractedCamera,
        extract_resource::{ExtractResource, ExtractResourcePlugin},
        render_graph::{
            NodeRunError, RenderGraph, RenderGraphContext, RenderLabel, ViewNode, ViewNodeRunner,
        },
        render_resource::{
            BindGroup, BindGroupEntry, BindGroupLayoutDescriptor, BindGroupLayoutEntry,
            BindingResource, BindingType, BlendComponent, BlendFactor, BlendOperation, BlendState,
            Buffer, BufferBindingType, BufferDescriptor, BufferId, BufferUsages,
            CachedRenderPipelineId, ColorTargetState, ColorWrites, CompareFunction,
            DepthStencilState, Face, FragmentState, LoadOp, MultisampleState, Operations,
            PipelineCache, PrimitiveState, RenderPassDepthStencilAttachment, RenderPassDescriptor,
            RenderPipelineDescriptor, ShaderStages, ShaderType, StencilFaceState, StencilOperation,
            StencilState, StoreOp, Texture, TextureDescriptor, TextureDimension, TextureFormat,
            TextureSampleType, TextureUsages, TextureView, TextureViewDimension, TextureViewId,
            VertexAttribute, VertexFormat, VertexState, VertexStepMode,
        },
        renderer::{RenderContext, RenderDevice, RenderQueue},
        view::{
            ExtractedView, ViewDepthTexture, ViewTarget, ViewUniform, ViewUniformOffset,
            ViewUniforms,
        },
    },
};
use render_model::{
    EntityShadow, EntityShadowFrame, EntityShadowParams, SHADOW_VOLUME_VERTICES,
    entity_shadow_colour, shadow_screen_rect, shadow_volume_mesh,
};

use crate::{AtmosphereFrame, SkyKind, scene_target::SceneTarget};

const SHADER: Handle<Shader> = uuid_handle!("6b0e5c1d-3f8a-4e27-9b41-2d7c0a5e8f13");
const INSTANCE_BYTES: u64 = size_of::<EntityShadow>() as u64;

/// Main-world holder of this frame's casters, cloned into the render world.
#[derive(Resource, ExtractResource, Clone, Default, Debug)]
pub struct EntityShadowScene(pub EntityShadowFrame);

#[derive(Debug, Hash, PartialEq, Eq, Clone, RenderLabel)]
pub(crate) struct EntityShadowLabel;

#[derive(Debug, Clone, Copy, Default)]
pub struct EntityShadowRenderPlugin;

impl Plugin for EntityShadowRenderPlugin {
    fn build(&self, app: &mut App) {
        crate::pipeline_warmup::register::<EntityShadowGpu>(app);
        load_internal_asset!(
            app,
            SHADER,
            "entity_shadow.wgsl",
            crate::shader_safety::from_wgsl
        );
        app.init_resource::<EntityShadowScene>()
            .add_plugins(ExtractResourcePlugin::<EntityShadowScene>::default())
            .add_systems(Last, crate::chunk::admit_depth_sampling);
    }

    fn finish(&self, app: &mut App) {
        let Some(render_app) = app.get_sub_app_mut(RenderApp) else {
            return;
        };
        let device = render_app.world().resource::<RenderDevice>().clone();
        render_app
            .insert_resource(EntityShadowGpu::new(&device))
            .add_systems(
                Render,
                (
                    prepare_shadow_buffers.in_set(RenderSystems::PrepareResources),
                    prepare_shadow_views
                        .in_set(RenderSystems::PrepareResources)
                        .after(bevy::render::view::prepare_view_targets)
                        .after(bevy::core_pipeline::core_3d::prepare_core_3d_depth_textures),
                    prepare_shadow_bind_groups.in_set(RenderSystems::PrepareBindGroups),
                ),
            );
        install_graph(render_app.world_mut());
    }
}

/// Orders shadows after all opaque terrain and before transparent geometry.
fn install_graph(world: &mut World) {
    let node = ViewNodeRunner::new(EntityShadowNode, world);
    let Some(mut graphs) = world.get_resource_mut::<RenderGraph>() else {
        return;
    };
    let Some(graph) = graphs.get_sub_graph_mut(Core3d) else {
        return;
    };
    graph.add_node(EntityShadowLabel, node);
    graph.add_node_edges((
        Node3d::MainOpaquePass,
        EntityShadowLabel,
        Node3d::MainTransmissivePass,
    ));
    // Whichever plugin installs second orders shadows after late terrain draws.
    if graph.get_node_state(crate::chunk::GpuCullLateLabel).is_ok() {
        let _ = graph.try_add_node_edge(crate::chunk::GpuCullLateLabel, EntityShadowLabel);
    }
}

mod gpu;
mod node;
mod view;
use gpu::*;
use node::EntityShadowNode;
use view::*;

#[cfg(test)]
mod tests;
