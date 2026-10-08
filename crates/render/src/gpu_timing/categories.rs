//! Optional pass splitting attributes opaque cost on adapters without in-pass timestamps.
//! Extra attachment stores and loads perturb timing; ordinary rendering never takes this path.

use std::ops::Range;

use bevy::{
    camera::Viewport,
    core_pipeline::core_3d::{AlphaMask3d, MainOpaquePass3dNode, Opaque3d},
    ecs::query::QueryItem,
    prelude::*,
    render::{
        render_graph::{Node, NodeRunError, RenderGraphContext, ViewNode, ViewNodeRunner},
        render_phase::{
            BinnedPhaseItem, BinnedRenderPhase, DrawError, DrawFunctions, DrawFunctionsInternal,
            PhaseItem, PhaseItemExtraIndex, TrackedRenderPass, ViewBinnedRenderPhases,
        },
        render_resource::{CommandEncoderDescriptor, RenderPassDescriptor, StoreOp},
        renderer::{RenderContext, RenderDevice},
    },
};

use super::GpuTimestamps;
use crate::{RuntimeStage, scene_target::SceneTarget};

#[cfg(test)]
mod tests;

/// Explicit profiling consent; splitting a tile render pass changes its GPU workload.
#[derive(Resource)]
pub(super) struct CategoryProfiling;

/// Reads only the opt-in flag, without inspecting or recording other process settings.
pub(super) fn requested() -> bool {
    std::env::var_os("RUST_MCBE_GPU_CATEGORIES").is_some_and(|value| value == "1")
}

/// Builds the replacement only when explicitly requested on a timestamp-capable device.
pub(super) fn replacement(world: &mut World) -> Option<Box<dyn Node>> {
    if !world.contains_resource::<CategoryProfiling>()
        || !world
            .get_resource::<RenderDevice>()?
            .features()
            .contains(wgpu::Features::TIMESTAMP_QUERY)
    {
        return None;
    }
    let scene = crate::scene_target::opaque_pass(world);
    Some(Box::new(ViewNodeRunner::new(
        OpaqueCategoryNode { scene },
        world,
    )))
}

/// Leaves other renderers' opaque nodes unchanged; only category splitting also owns the scene pass.
pub(super) fn replaceable(node: &dyn Node, categories: bool) -> bool {
    node.downcast_ref::<ViewNodeRunner<MainOpaquePass3dNode>>()
        .is_some()
        || (categories && crate::scene_target::is_opaque_pass(node))
}

/// Mesh, alpha-mask and skybox phases retain Bevy's complete original implementation.
fn admitted(
    opaque: &BinnedRenderPhase<Opaque3d>,
    alpha: &BinnedRenderPhase<AlphaMask3d>,
    sky: bool,
) -> bool {
    !sky && alpha.is_empty()
        && opaque.multidrawable_meshes.is_empty()
        && opaque.batchable_meshes.is_empty()
        && opaque.unbatchable_meshes.is_empty()
}

/// Adjacent equal categories share a pass; separated categories never move across other bins.
fn category_ranges(
    stages: impl IntoIterator<Item = RuntimeStage>,
) -> Vec<(RuntimeStage, Range<usize>)> {
    let mut groups: Vec<(RuntimeStage, Range<usize>)> = Vec::new();
    for (index, stage) in stages.into_iter().enumerate() {
        if let Some((previous, range)) = groups.last_mut()
            && *previous == stage
        {
            range.end = index + 1;
        } else {
            groups.push((stage, index..index + 1));
        }
    }
    if groups.is_empty() {
        groups.push((RuntimeStage::GpuOpaqueOther, 0..0));
    }
    groups
}

/// Executes exactly the phase's existing bin and entity order with Bevy's non-mesh item state.
fn draw_bins<'w>(
    phase: &BinnedRenderPhase<Opaque3d>,
    range: Range<usize>,
    functions: &mut DrawFunctionsInternal<Opaque3d>,
    world: &'w World,
    view: Entity,
    pass: &mut TrackedRenderPass<'w>,
) -> Result<(), DrawError> {
    for ((batch, bin), entries) in phase
        .non_mesh_items
        .iter()
        .skip(range.start)
        .take(range.len())
    {
        for (main_entity, entity) in &entries.entities {
            let item = Opaque3d::new(
                batch.clone(),
                bin.clone(),
                (*entity, *main_entity),
                0..1,
                PhaseItemExtraIndex::None,
            );
            if let Some(draw) = functions.get_mut(item.draw_function()) {
                draw.draw(world, pass, view, &item)?;
            }
        }
    }
    Ok(())
}

struct OpaqueCategoryNode {
    /// Draws unsplittable phases when the view renders into shared scene samples.
    scene: Box<dyn Node>,
}

impl ViewNode for OpaqueCategoryNode {
    type ViewQuery = (
        <MainOpaquePass3dNode as ViewNode>::ViewQuery,
        Option<&'static SceneTarget>,
    );

    fn update(&mut self, world: &mut World) {
        self.scene.update(world);
    }

    fn run<'w>(
        &self,
        graph: &mut RenderGraphContext,
        context: &mut RenderContext<'w>,
        (view, scene): QueryItem<'w, '_, Self::ViewQuery>,
        world: &'w World,
    ) -> Result<(), NodeRunError> {
        let (camera, extracted, target, depth, sky_pipeline, sky_group, _, resolution_override) =
            view;
        let phases = world.get_resource::<ViewBinnedRenderPhases<Opaque3d>>();
        let alpha = world.get_resource::<ViewBinnedRenderPhases<AlphaMask3d>>();
        let phases = phases.and_then(|phases| phases.get(&extracted.retained_view_entity));
        let alpha = alpha.and_then(|phases| phases.get(&extracted.retained_view_entity));
        // The shared scene pass never draws a skybox.
        let sky = scene.is_none() && (sky_pipeline.is_some() || sky_group.is_some());
        let (Some(phase), Some(alpha)) = (phases, alpha) else {
            return self.fallback(graph, context, view, scene, world);
        };
        if !admitted(phase, alpha, sky) {
            return self.fallback(graph, context, view, scene, world);
        }
        let functions = world.resource::<DrawFunctions<Opaque3d>>();
        let groups = {
            let functions = functions.read();
            category_ranges(phase.non_mesh_items.keys().map(|(batch, _)| {
                crate::chunk::pipeline::opaque::timing_category(&functions, batch.draw_function)
            }))
        };
        let view_entity = graph.view_entity();
        // Claim clear ownership before later graph nodes request these attachments.
        let mut color = scene.map_or_else(
            || target.get_color_attachment(),
            |scene| scene.color_attachment(target, false),
        );
        let mut depth = depth.get_attachment(StoreOp::Store);
        context.add_command_buffer_generation_task(move |device| {
            let mut encoder = device.create_command_encoder(&CommandEncoderDescriptor {
                label: Some("opaque category profiling"),
            });
            let mut functions = functions.write();
            if !phase.is_empty() {
                functions.prepare(world);
            }
            for (stage, range) in groups {
                let span = world
                    .get_resource::<GpuTimestamps>()
                    .and_then(|timestamps| timestamps.open_pass(stage));
                let colors = [Some(color.clone())];
                let pass = encoder.begin_render_pass(&RenderPassDescriptor {
                    label: Some(stage.name()),
                    color_attachments: &colors,
                    depth_stencil_attachment: Some(depth.clone()),
                    timestamp_writes: span.as_ref().map(|span| wgpu::RenderPassTimestampWrites {
                        query_set: span.queries,
                        beginning_of_pass_write_index: Some(span.begin),
                        end_of_pass_write_index: Some(span.begin + 1),
                    }),
                    occlusion_query_set: None,
                });
                color.ops.load = wgpu::LoadOp::Load;
                if let Some(ops) = &mut depth.depth_ops {
                    ops.load = wgpu::LoadOp::Load;
                }
                let mut pass = TrackedRenderPass::new(&device, pass);
                if let Some(viewport) = Viewport::from_viewport_and_override(
                    camera.viewport.as_ref(),
                    resolution_override,
                ) {
                    pass.set_camera_viewport(&viewport);
                }
                if let Err(error) =
                    draw_bins(phase, range, &mut functions, world, view_entity, &mut pass)
                {
                    bevy::log::error!("Opaque category draw failed: {error:?}");
                    break;
                }
            }
            encoder.finish()
        });
        Ok(())
    }
}

impl OpaqueCategoryNode {
    /// Runs the view's ordinary opaque pass in one timed render pass.
    fn fallback<'w>(
        &self,
        graph: &mut RenderGraphContext,
        context: &mut RenderContext<'w>,
        view: QueryItem<'w, '_, <MainOpaquePass3dNode as ViewNode>::ViewQuery>,
        scene: Option<&SceneTarget>,
        world: &'w World,
    ) -> Result<(), NodeRunError> {
        if scene.is_some() {
            return self.scene.run(graph, context, world);
        }
        super::opaque::OpaqueTimingNode.run(graph, context, view, world)
    }
}
