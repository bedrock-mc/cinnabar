use crate::ui_render::DeviceObservation;
use crate::viewmodel::{
    HandVertex, ViewmodelCompletionGate, ViewmodelScene, ViewmodelToken, hand_projection,
    viewmodel_depth_bytes,
};
#[cfg(test)]
use bevy::prelude::Assets;
use bevy::{
    asset::{load_internal_asset, uuid_handle},
    core_pipeline::Core3d,
    ecs::system::{SystemChangeTick, SystemParam},
    mesh::VertexBufferLayout,
    prelude::{
        App, BevyError, Commands, DetectChanges, Entity, Handle, IntoScheduleConfigs, Msaa, Plugin,
        Query, Res, ResMut, Resource, Result, Shader, SystemSet, UVec4, World, default,
    },
    render::{
        Render, RenderApp, RenderStartup, RenderSystems,
        extract_resource::ExtractResourcePlugin,
        render_resource::{
            AddressMode, BindGroup, BindGroupEntry, BindGroupLayoutDescriptor,
            BindGroupLayoutEntry, BindingResource, BindingType, Buffer, BufferBindingType,
            BufferDescriptor, BufferInitDescriptor, BufferSize, BufferUsages,
            CachedRenderPipelineId, ColorTargetState, ColorWrites, CompareFunction,
            DepthStencilState, Extent3d, FilterMode, FragmentState, LoadOp, Operations,
            PipelineCache, RenderPassDepthStencilAttachment, RenderPassDescriptor,
            RenderPipelineDescriptor, Sampler, SamplerBindingType, SamplerDescriptor, ShaderStages,
            StoreOp, Texture, TextureDataOrder, TextureDescriptor, TextureDimension, TextureFormat,
            TextureSampleType, TextureUsages, TextureView, TextureViewDescriptor,
            TextureViewDimension, VertexAttribute, VertexFormat, VertexState, VertexStepMode,
        },
        renderer::{RenderAdapter, RenderDevice, RenderQueue},
        sync_world::MainEntity,
        view::ExtractedView,
    },
};
use std::{mem::size_of, sync::Mutex};
mod gpu;
mod node;
use gpu::*;
#[cfg(test)]
mod tests;
const HAND_SHADER: Handle<Shader> = uuid_handle!("05c3d760-7ab6-4f19-b6b3-dea197927fa5");

#[derive(Debug, Clone, Copy, Default)]
pub struct ViewmodelRenderPlugin;
#[derive(Resource)]
struct Installed;
#[derive(Debug, Hash, PartialEq, Eq, Clone, SystemSet)]
pub(crate) struct HandLabel;
impl Plugin for ViewmodelRenderPlugin {
    fn build(&self, app: &mut App) {
        install(app);
    }
    fn finish(&self, app: &mut App) {
        install(app);
    }
}
fn install(app: &mut App) {
    app.init_resource::<ViewmodelScene>()
        .init_resource::<ViewmodelCompletionGate>();
    crate::pipeline_warmup::register::<HandGpu>(app);
    let Some(render_app) = app.get_sub_app(RenderApp) else {
        return;
    };
    if render_app.world().contains_resource::<Installed>() {
        install_hand_graph(app.sub_app_mut(RenderApp).world_mut());
        return;
    }
    let gate = app.world().resource::<ViewmodelCompletionGate>().clone();
    app.add_plugins(ExtractResourcePlugin::<ViewmodelScene>::default());
    load_internal_asset!(
        app,
        HAND_SHADER,
        "viewmodel.wgsl",
        crate::shader_safety::from_wgsl
    );
    let render_app = app.sub_app_mut(RenderApp);
    crate::device_poll::install(render_app);
    render_app
        .insert_resource(Installed)
        .insert_resource(gate)
        .init_resource::<HandDrawn>()
        .add_systems(RenderStartup, init_gpu)
        .add_systems(
            Render,
            (
                prepare.in_set(RenderSystems::PrepareResources),
                submit_completion.in_set(crate::device_poll::FrameSubmissions),
            ),
        );
    install_hand_graph(render_app.world_mut());
}

/// The hand pass Enhanced views run after Bloom and grading.
#[cfg(feature = "enhanced")]
pub(crate) fn enhanced_post_pass(
    world: &World,
) -> Option<bevy::ecs::schedule::ScheduleConfigs<bevy::ecs::system::ScheduleSystem>> {
    world.contains_resource::<Installed>().then(|| {
        crate::gpu_timing::profiled(
            node::hand_view,
            Some(crate::RuntimeStage::GpuHand),
            "EnhancedHandLabel",
        )
        .run_if(crate::ui_render::overlay::grade_stage::<true>)
    })
}

pub(crate) fn install_hand_graph(world: &mut World) {
    if !world.contains_resource::<Installed>() || world.contains_resource::<HandPassInstalled>() {
        return;
    }
    let installed = world
        .try_schedule_scope(Core3d, |_, schedule| {
            schedule.add_systems(
                crate::gpu_timing::profiled(
                    node::hand_view,
                    Some(crate::RuntimeStage::GpuHand),
                    "HandLabel",
                )
                .in_set(HandLabel)
                .after(crate::ui_render::UiWorldLabel)
                .before(crate::hand_rig_render::HandRigLabel)
                .before(crate::scene_target::ScenePass::Finish)
                .in_set(bevy::core_pipeline::Core3dSystems::MainPass)
                .run_if(crate::ui_render::overlay::grade_stage::<false>),
            );
        })
        .is_ok();
    if installed {
        world.insert_resource(HandPassInstalled);
    }
}

#[derive(Resource)]
struct HandPassInstalled;

impl crate::pipeline_warmup::PrewarmPipelines for HandGpu {
    fn prewarm(
        &mut self,
        cache: &PipelineCache,
        view: crate::pipeline_warmup::WarmView,
        ids: &mut crate::pipeline_warmup::WarmupIds,
    ) -> Result<(), BevyError> {
        let layout = &self.layout;
        let samples = view.msaa.samples();
        let id = memoized_hand_pipeline(&mut self.pipeline_variants, samples, view.hdr, || {
            cache.queue_render_pipeline(specialized_hand_pipeline(
                layout.clone(),
                samples,
                view.hdr,
            ))
        })
        .ok_or("unsupported hand pipeline sample count")?;
        ids.push(id);
        Ok(())
    }
}
