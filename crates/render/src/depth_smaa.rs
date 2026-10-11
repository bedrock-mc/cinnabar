//! Spatial SMAA detects world depth edges before text and first-person overlays draw.

mod nametags;
mod node;
mod pipelines;
#[cfg(test)]
mod tests;

#[cfg(test)]
use bevy::prelude::{
    AssetApp, Camera, Image, IntoSystem, MinimalPlugins, Mut, System, Transform, TransformPlugin,
};
#[cfg(test)]
use bevy::render::render_resource::{BufferUsages, Extent3d, TextureUsages};
use bevy::{
    anti_alias::smaa::{Smaa, SmaaPlugin, SmaaTextures},
    asset::load_internal_asset,
    core_pipeline::{Core3d, Core3dSystems},
    ecs::system::RunSystemOnce,
    prelude::{
        App, AssetServer, BevyError, Camera3d, Commands, Component, Entity, Handle,
        IntoScheduleConfigs, Local, Msaa, Plugin, Query, QueryState, Res, ResMut, Resource, Result,
        Shader, SystemSet, With, Without, World, default,
    },
    render::{
        Render, RenderApp, RenderStartup, RenderSystems,
        render_resource::{
            BindGroup, BindGroupEntries, BindGroupLayoutDescriptor, BindGroupLayoutEntries,
            BindGroupLayoutEntry, BindingType, BufferId, CachedRenderPipelineId, ColorTargetState,
            ColorWrites, CompareFunction, DepthStencilState, FragmentState, LoadOp, Operations,
            PipelineCache, RenderPassColorAttachment, RenderPassDepthStencilAttachment,
            RenderPassDescriptor, RenderPipelineDescriptor, SamplerBindingType, ShaderStages,
            StencilFaceState, StencilOperation, StencilState, StoreOp, TextureFormat,
            TextureSampleType, TextureViewDimension, TextureViewId, VertexState,
        },
        renderer::RenderDevice,
        view::{ViewDepthTexture, ViewTarget},
    },
};
use pipelines::DepthSmaaPipelines;

/// Uses Bevy's spatial SMAA blending with an exclusively depth-based edge pass.
#[derive(Default)]
pub struct DepthSmaaPlugin;

impl Plugin for DepthSmaaPlugin {
    fn build(&self, app: &mut App) {
        app.add_plugins(SmaaPlugin);
        load_internal_asset!(
            app,
            pipelines::EDGE_SHADER,
            "depth_smaa/edge.wgsl",
            crate::shader_safety::from_wgsl
        );
        load_internal_asset!(
            app,
            pipelines::RESTORE_SHADER,
            "depth_smaa/restore.wgsl",
            crate::shader_safety::from_wgsl
        );
        crate::pipeline_warmup::register::<DepthSmaaPipelines>(app);
        if let Some(render) = app.get_sub_app_mut(RenderApp) {
            render
                .add_systems(RenderStartup, pipelines::init)
                .add_systems(Render, prepare.in_set(RenderSystems::PrepareBindGroups))
                .add_systems(
                    Render,
                    sync_graph
                        .after(crate::motion_blur::graph::sync_graph)
                        .in_set(RenderSystems::Prepare),
                );
        }
    }

    fn finish(&self, app: &mut App) {
        if let Some(render) = app.get_sub_app_mut(RenderApp) {
            render.world_mut().run_system_once(sync_graph).unwrap();
        }
    }
}

#[derive(Component)]
struct DepthSmaaView {
    source: TextureViewId,
    uniform: BufferId,
    depth: BindGroup,
    post: [(TextureViewId, BindGroup, BindGroup); 2],
    ids: pipelines::PipelineIds,
    samples: u32,
    format: TextureFormat,
}

type PreparedView = (
    Entity,
    &'static ViewTarget,
    &'static ViewDepthTexture,
    &'static Msaa,
    Option<&'static DepthSmaaView>,
);

/// Caches bindings for both ping-pong colours and removes them immediately when disabled.
fn prepare(
    mut commands: Commands,
    device: Res<RenderDevice>,
    cache: Res<PipelineCache>,
    mut pipelines: ResMut<DepthSmaaPipelines>,
    uniforms: Res<bevy::anti_alias::smaa::SmaaInfoUniformBuffer>,
    views: Query<PreparedView, (With<Smaa>, With<SmaaTextures>)>,
    removed: Query<Entity, (With<DepthSmaaView>, Without<Smaa>)>,
) {
    for entity in &removed {
        commands.entity(entity).remove::<(
            DepthSmaaView,
            SmaaTextures,
            bevy::anti_alias::smaa::SmaaBindGroups,
            bevy::anti_alias::smaa::ViewSmaaPipelines,
            bevy::anti_alias::smaa::SmaaInfoUniformOffset,
        )>();
    }
    let Some(uniform) = uniforms.binding() else {
        return;
    };
    let uniform_id = uniforms.buffer().expect("bound SMAA uniforms").id();
    for (entity, target, depth, msaa, previous) in &views {
        let samples = msaa.samples();
        let format = target.main_texture_format();
        if previous.is_some_and(|old| {
            old.source == depth.view().id()
                && old.samples == samples
                && old.format == format
                && old.uniform == uniform_id
                && old
                    .post
                    .iter()
                    .any(|(id, _, _)| *id == target.main_texture_view().id())
        }) {
            continue;
        }
        let ids = pipelines.ids(&cache, samples, format);
        let depth_binding = device.create_bind_group(
            "SMAA depth input",
            &cache.get_bind_group_layout(&pipelines.depth_layout(samples)),
            &BindGroupEntries::single(depth.view()),
        );
        let post = [target.main_texture_view(), target.main_texture_other_view()].map(|view| {
            let binding = device.create_bind_group(
                "SMAA world colour",
                &cache.get_bind_group_layout(&pipelines.post_layout),
                &BindGroupEntries::sequential((view, uniform.clone())),
            );
            let restore = device.create_bind_group(
                "SMAA scene restore",
                &cache.get_bind_group_layout(&pipelines.restore_layout),
                &BindGroupEntries::single(view),
            );
            (view.id(), binding, restore)
        });
        commands.entity(entity).insert(DepthSmaaView {
            source: depth.view().id(),
            uniform: uniform_id,
            depth: depth_binding,
            post,
            ids,
            samples,
            format,
        });
    }
}

#[derive(SystemSet, Clone, Debug, PartialEq, Eq, Hash)]
pub(crate) struct DepthSmaaLabel;

#[derive(Resource)]
struct SmaaPassesInstalled(bool);

/// Replaces luma filtering with depth filtering and omits both passes when disabled.
fn configure_passes(world: &mut World, enabled: bool) {
    if world
        .get_resource::<SmaaPassesInstalled>()
        .is_some_and(|state| state.0 == enabled)
    {
        return;
    }
    let configured = world
        .try_schedule_scope(Core3d, |world, schedule| {
            use bevy::ecs::schedule::ScheduleCleanupPolicy;
            let _ = schedule.remove_systems_in_set(
                bevy::anti_alias::smaa::smaa,
                world,
                ScheduleCleanupPolicy::RemoveSystemsOnly,
            );
            let _ = schedule.remove_systems_in_set(
                DepthSmaaLabel,
                world,
                ScheduleCleanupPolicy::RemoveSystemsOnly,
            );
            let _ = schedule.remove_systems_in_set(
                nametags::NametagsAfterSmaaLabel,
                world,
                ScheduleCleanupPolicy::RemoveSystemsOnly,
            );
            if enabled {
                schedule.add_systems(
                    (
                        crate::gpu_timing::profiled(node::depth_smaa, None, "Smaa")
                            .in_set(DepthSmaaLabel)
                            .before(crate::motion_blur::graph::MotionBlurLabel),
                        crate::gpu_timing::profiled(
                            nametags::nametags_after_smaa,
                            None,
                            "NametagsAfterSmaaLabel",
                        )
                        .in_set(nametags::NametagsAfterSmaaLabel)
                        .after(DepthSmaaLabel)
                        .after(crate::motion_blur::graph::MotionBlurLabel),
                    )
                        .after(crate::scene_target::ScenePass::Transparent)
                        .before(crate::ui_render::UiWorldLabel)
                        .in_set(Core3dSystems::MainPass),
                );
            }
        })
        .is_ok();
    if configured {
        world.insert_resource(SmaaPassesInstalled(enabled));
    }
}

/// Reuses the world-camera query across frames without allocating for unchanged views.
type SmaaCameraQuery = QueryState<Entity, (With<Smaa>, With<Camera3d>)>;

/// Disabled views have no SMAA pass, including on the launcher camera.
fn sync_graph(world: &mut World, mut views: Local<Option<SmaaCameraQuery>>) {
    let enabled = views
        .get_or_insert_with(|| world.query_filtered())
        .iter(world)
        .next()
        .is_some();
    configure_passes(world, enabled);
}
