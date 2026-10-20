//! Near-camera first-person pass that draws the local player's own animated rig (arms + hands)
//! over the scene, reusing the actor rig's packed buffers with a hand-local view and lighting.
//! The rendered content is the player's own skin on the standard samples player geometry.

use bevy::prelude::IntoScheduleConfigs;
#[cfg(all(test, target_os = "macos"))]
use bevy::prelude::{Entity, GlobalTransform, UVec4};
use bevy::{
    asset::{load_internal_asset, uuid_handle},
    core_pipeline::{Core3d, core_3d::CORE_3D_DEPTH_FORMAT},
    prelude::{
        App, BevyError, Commands, Handle, Mat4, Msaa, Plugin, Query, Res, ResMut, Resource, Result,
        Shader, SystemSet, World, default,
    },
    render::{
        Render, RenderApp, RenderStartup, RenderSystems,
        extract_resource::ExtractResourcePlugin,
        render_resource::{
            AddressMode, BindGroup, BindGroupEntry, BindGroupLayoutDescriptor,
            BindGroupLayoutEntry, BindingType, BlendState, Buffer, BufferBindingType,
            BufferInitDescriptor, BufferSize, BufferUsages, CachedRenderPipelineId,
            ColorTargetState, ColorWrites, CompareFunction, DepthStencilState, Extent3d,
            FilterMode, FragmentState, PipelineCache, RenderPipelineDescriptor, Sampler,
            SamplerBindingType, SamplerDescriptor, ShaderStages, Texture, TextureDataOrder,
            TextureDescriptor, TextureDimension, TextureFormat, TextureSampleType, TextureUsages,
            TextureView, TextureViewDescriptor, TextureViewDimension, VertexState,
        },
        renderer::{RenderDevice, RenderQueue},
        view::ExtractedView,
    },
};
use render_api::{CAMERA_NEAR_PLANE_BLOCKS, SkinRgba8};
use render_model::ActorRigVertex;
use render_model::hand::{HAND_BLEND_LAYER_FLAG, HAND_CUTOUT_LAYER_FLAG, HAND_TEXTURE_LAYER_MASK};
use render_scene::hand_rig::HandRigFrame;
use std::{mem::size_of, sync::Arc};
use {
    render_model::{HAND_ITEM_LAYER_FLAG, HAND_OFFHAND_LAYER_FLAG},
    render_scene::HandRigScene,
};

pub(crate) mod bindings;
mod gpu;
mod node;
use gpu::*;
#[cfg(test)]
mod tests;

const HAND_RIG_SHADER: Handle<Shader> = uuid_handle!("6f2b1c74-4a2e-49d8-9c1a-2f7b0d5e3a61");

#[repr(C)]
#[derive(Clone, Copy, bytemuck::Pod, bytemuck::Zeroable)]
struct HandMaterialUniform {
    texture_flags: [u32; 4],
    layer_mask: [u32; 4],
}

const HAND_MATERIAL: HandMaterialUniform = HandMaterialUniform {
    texture_flags: [
        HAND_ITEM_LAYER_FLAG,
        HAND_OFFHAND_LAYER_FLAG,
        HAND_BLEND_LAYER_FLAG,
        HAND_CUTOUT_LAYER_FLAG,
    ],
    layer_mask: [HAND_TEXTURE_LAYER_MASK, 0, 0, 0],
};

#[derive(Debug, Hash, PartialEq, Eq, Clone, SystemSet)]
pub(crate) struct HandRigLabel;

#[derive(Debug, Clone, Copy, Default)]
pub struct HandRigRenderPlugin;

impl Plugin for HandRigRenderPlugin {
    fn build(&self, app: &mut App) {
        install(app);
    }
    fn finish(&self, app: &mut App) {
        install(app);
    }
}

fn install(app: &mut App) {
    app.init_resource::<HandRigScene>();
    crate::upload_staging::install(app);
    crate::pipeline_warmup::register::<HandRigGpu>(app);
    crate::lighting::install(app);
    let Some(render_app) = app.get_sub_app(RenderApp) else {
        return;
    };
    if render_app.world().contains_resource::<Installed>() {
        install_graph(app.sub_app_mut(RenderApp).world_mut());
        return;
    }
    app.add_plugins(ExtractResourcePlugin::<HandRigScene>::default());
    crate::actor_render::load_rig_shader(app);
    load_internal_asset!(
        app,
        HAND_RIG_SHADER,
        "hand_rig.wesl",
        crate::shader_safety::from_actor_wesl,
        render_model::ACTOR_GPU_INSTANCE_WORDS,
        render_model::ACTOR_RIG_VERTEX_WORDS
    );
    let render_app = app.sub_app_mut(RenderApp);
    render_app
        .insert_resource(Installed)
        .add_systems(RenderStartup, init_gpu)
        .add_systems(Render, prepare.in_set(RenderSystems::PrepareResources));
    install_graph(render_app.world_mut());
}

/// The rig pass Enhanced views run after Bloom and grading.
#[cfg(feature = "enhanced")]
pub(crate) fn enhanced_post_pass(
    world: &World,
) -> Option<bevy::ecs::schedule::ScheduleConfigs<bevy::ecs::system::ScheduleSystem>> {
    world.contains_resource::<Installed>().then(|| {
        crate::gpu_timing::profiled(
            node::hand_rig,
            Some(render_scene::RuntimeStage::GpuHand),
            "EnhancedHandRigLabel",
        )
        .run_if(crate::ui_render::overlay::grade_stage::<true>)
    })
}

pub(crate) fn install_graph(world: &mut World) {
    if !world.contains_resource::<Installed>() || world.contains_resource::<RigPassInstalled>() {
        return;
    }
    let installed = world
        .try_schedule_scope(Core3d, |_, schedule| {
            schedule.add_systems(
                crate::gpu_timing::profiled(
                    node::hand_rig,
                    Some(render_scene::RuntimeStage::GpuHand),
                    "HandRigLabel",
                )
                .in_set(HandRigLabel)
                .after(crate::ui_render::UiWorldLabel)
                .after(crate::viewmodel_render::HandLabel)
                .before(crate::scene_target::ScenePass::Finish)
                .in_set(bevy::core_pipeline::Core3dSystems::MainPass)
                .run_if(crate::ui_render::overlay::grade_stage::<false>),
            );
        })
        .is_ok();
    if installed {
        world.insert_resource(RigPassInstalled);
    }
}

#[derive(Resource)]
struct RigPassInstalled;

#[derive(Resource)]
struct Installed;
