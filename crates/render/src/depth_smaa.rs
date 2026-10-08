//! Spatial SMAA detects world depth edges before text and first-person overlays draw.

mod nametags;
mod node;
mod pipelines;
#[cfg(test)]
mod tests;

use bevy::{
    anti_alias::smaa::{Smaa, SmaaPlugin, SmaaTextures},
    asset::load_internal_asset,
    core_pipeline::core_3d::graph::{Core3d, Node3d},
    ecs::system::RunSystemOnce,
    prelude::*,
    render::{
        Render, RenderApp, RenderStartup, RenderSystems,
        render_graph::{RenderGraph, RenderLabel, ViewNodeRunner},
        render_resource::*,
        renderer::RenderDevice,
        view::{ViewDepthTexture, ViewTarget},
    },
    shader::Shader,
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
            Shader::from_wgsl
        );
        load_internal_asset!(
            app,
            pipelines::RESTORE_SHADER,
            "depth_smaa/restore.wgsl",
            Shader::from_wgsl
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

/// Replaces the stock luma node and restores world samples before any text or hand draw.
fn install_graph(world: &mut World) {
    let runner = ViewNodeRunner::new(node::DepthSmaaNode, world);
    let nametags = ViewNodeRunner::new(nametags::NametagsAfterSmaa, world);
    let Some(mut graphs) = world.get_resource_mut::<RenderGraph>() else {
        return;
    };
    let Some(graph) = graphs.get_sub_graph_mut(Core3d) else {
        return;
    };
    let _ = graph.remove_node(Node3d::Smaa);
    let _ = graph.remove_node(nametags::NametagsAfterSmaaLabel);
    graph.add_node(Node3d::Smaa, runner);
    graph.add_node(nametags::NametagsAfterSmaaLabel, nametags);
    graph.add_node_edges((
        Node3d::MainTransparentPass,
        Node3d::Smaa,
        nametags::NametagsAfterSmaaLabel,
        crate::ui_render::UiWorldLabel,
    ));
    for before in [
        crate::chunk::GpuCullLateLabel.intern(),
        crate::entity_shadow_render::EntityShadowLabel.intern(),
    ] {
        if graph.get_node_state(before).is_ok() {
            graph.add_node_edge(before, Node3d::Smaa);
        }
    }
}

/// Disabled views have no SMAA graph node or pass, including on the launcher camera.
fn sync_graph(
    world: &mut World,
    mut views: Local<Option<QueryState<Entity, (With<Smaa>, With<Camera3d>)>>>,
) {
    let enabled = views
        .get_or_insert_with(|| world.query_filtered())
        .iter(world)
        .next()
        .is_some();
    let installed = world
        .get_resource::<RenderGraph>()
        .and_then(|graphs| graphs.get_sub_graph(Core3d))
        .is_some_and(|graph| {
            graph
                .get_node_state(Node3d::Smaa)
                .is_ok_and(|state| state.type_name.contains("DepthSmaaNode"))
        });
    if enabled && !installed {
        install_graph(world);
    }
    if !enabled
        && let Some(mut graphs) = world.get_resource_mut::<RenderGraph>()
        && let Some(graph) = graphs.get_sub_graph_mut(Core3d)
    {
        let _ = graph.remove_node(Node3d::Smaa);
        let _ = graph.remove_node(nametags::NametagsAfterSmaaLabel);
    }
    if enabled
        && let Some(mut graphs) = world.get_resource_mut::<RenderGraph>()
        && let Some(graph) = graphs.get_sub_graph_mut(Core3d)
        && graph
            .get_node_state(crate::motion_blur::graph::MotionBlurLabel)
            .is_ok()
    {
        for (output_node, input_node) in [
            (
                Node3d::Smaa.intern(),
                crate::motion_blur::graph::MotionBlurLabel.intern(),
            ),
            (
                crate::motion_blur::graph::MotionBlurLabel.intern(),
                nametags::NametagsAfterSmaaLabel.intern(),
            ),
        ] {
            if !graph.has_edge(&bevy::render::render_graph::Edge::NodeEdge {
                output_node,
                input_node,
            }) {
                graph.add_node_edge(output_node, input_node);
            }
        }
    }
}
