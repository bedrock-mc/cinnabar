//! Optional pass splitting attributes opaque cost on adapters without in-pass timestamps.
//! Extra attachment stores and loads perturb timing; ordinary rendering never takes this path.

use std::ops::Range;

use bevy::{
    camera::Viewport,
    core_pipeline::core_3d::{AlphaMask3d, Opaque3d},
    prelude::{Entity, Resource, World},
    render::{
        render_phase::{
            BinnedPhaseItem, BinnedRenderPhase, DrawError, DrawFunctions, DrawFunctionsInternal,
            PhaseItem, PhaseItemExtraIndex, TrackedRenderPass,
        },
        render_resource::RenderPassDescriptor,
        renderer::RenderContext,
    },
};

use super::GpuTimestamps;
use crate::RuntimeStage;

#[cfg(test)]
mod tests;

/// Explicit profiling consent; splitting a tile render pass changes its GPU workload.
#[derive(Resource)]
pub(super) struct CategoryProfiling;

/// Reads only the opt-in flag, without inspecting or recording other process settings.
pub(super) fn requested() -> bool {
    std::env::var_os("RUST_MCBE_GPU_CATEGORIES").is_some_and(|value| value == "1")
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

/// Splits supported opaque bins into adjacent categories only when profiling is requested.
#[allow(clippy::too_many_arguments)]
pub(crate) fn draw_categories(
    world: &World,
    context: &mut RenderContext,
    view: Entity,
    opaque: &BinnedRenderPhase<Opaque3d>,
    alpha: &BinnedRenderPhase<AlphaMask3d>,
    sky: bool,
    mut color: bevy::render::render_resource::RenderPassColorAttachment,
    mut depth: bevy::render::render_resource::RenderPassDepthStencilAttachment,
    viewport: Option<Option<Viewport>>,
) -> bool {
    if !world.contains_resource::<CategoryProfiling>()
        || !context
            .render_device()
            .features()
            .contains(wgpu::Features::TIMESTAMP_QUERY)
        || !admitted(opaque, alpha, sky)
    {
        return false;
    }
    let functions = world.resource::<DrawFunctions<Opaque3d>>();
    let groups = {
        let functions = functions.read();
        category_ranges(opaque.non_mesh_items.keys().map(|(batch, _)| {
            crate::chunk::pipeline::opaque::timing_category(&functions, batch.draw_function)
        }))
    };
    let mut functions = functions.write();
    if !opaque.is_empty() {
        functions.prepare(world);
    }
    for (stage, range) in groups {
        let span = world
            .get_resource::<GpuTimestamps>()
            .and_then(|timestamps| timestamps.open_pass(stage));
        let colors = [Some(color.clone())];
        let mut pass = context.begin_tracked_render_pass(RenderPassDescriptor {
            label: Some(stage.name()),
            color_attachments: &colors,
            depth_stencil_attachment: Some(depth.clone()),
            timestamp_writes: span.as_ref().map(|span| wgpu::RenderPassTimestampWrites {
                query_set: span.queries,
                beginning_of_pass_write_index: Some(span.begin),
                end_of_pass_write_index: Some(span.begin + 1),
            }),
            occlusion_query_set: None,
            multiview_mask: None,
        });
        color.ops.load = wgpu::LoadOp::Load;
        if let Some(ops) = &mut depth.depth_ops {
            ops.load = wgpu::LoadOp::Load;
        }
        match &viewport {
            Some(Some(viewport)) => pass.set_camera_viewport(viewport),
            Some(None) => continue,
            None => {}
        }
        if let Err(error) = draw_bins(opaque, range, &mut functions, world, view, &mut pass) {
            bevy::log::error!("Opaque category draw failed: {error:?}");
            break;
        }
    }
    true
}
