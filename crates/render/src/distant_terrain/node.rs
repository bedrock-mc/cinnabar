//! The distant terrain passes of the main view, after opaque terrain and before
//! transparency: the scatter, pass A (last frame's visible clusters), the depth pyramid
//! over the near terrain and pass A, culling and compaction, then pass B (newly visible
//! clusters) and water in one render pass, with an encoder-level timestamp between each
//! while timing is on. Every render pass loads and stores the whole target, which costs
//! bandwidth on tiled GPUs, so there are only two, and none, nor a pyramid, while nothing is
//! drawn.

use std::sync::atomic::Ordering;

use bevy::{
    camera::{MainPassResolutionOverride, Viewport},
    core_pipeline::{Core3d, Core3dSystems},
    ecs::{
        resource::Resource,
        schedule::{IntoScheduleConfigs, SystemSet},
        world::World,
    },
    render::{
        camera::ExtractedCamera,
        render_resource::StoreOp,
        renderer::RenderContext,
        view::{ViewDepthStencilTexture, ViewTarget},
    },
};

use super::{
    gpu::{LodGpu, TIMESTAMP_MARKS},
    layout::{
        ARGS_PASS_A, ARGS_PASS_B, ARGS_SECTION_BINS, ARGS_TRANSLUCENT, ARGS_WORDS, DRAW_PASS_STRIDE,
    },
    renderer::{LodFrame, LodPyramid, LodRenderer},
};
use crate::scene_target::{ScenePass, SceneTarget};

/// Orders the distant terrain passes in the main pass.
#[derive(Debug, Hash, PartialEq, Eq, Clone, SystemSet)]
pub struct DistantTerrainPassLabel;

#[derive(Resource)]
struct PassesInstalled;

/// Adds the pass system to the camera schedule once.
pub(crate) fn install_graph(world: &mut World) {
    if world.contains_resource::<PassesInstalled>() {
        return;
    }
    let installed = world
        .try_schedule_scope(Core3d, |_, schedule| {
            schedule.add_systems(
                crate::gpu_timing::profiled(
                    distant_terrain_passes,
                    Some(render_scene::RuntimeStage::GpuOpaque),
                    "DistantTerrainPassLabel",
                )
                .in_set(DistantTerrainPassLabel)
                .after(ScenePass::Opaque)
                .after(crate::chunk::GpuCullLateLabel)
                .before(ScenePass::Transparent)
                .in_set(Core3dSystems::MainPass),
            );
        })
        .is_ok();
    if installed {
        world.insert_resource(PassesInstalled);
    }
}

type PassQuery = (
    &'static ExtractedCamera,
    &'static ViewTarget,
    &'static SceneTarget,
    &'static ViewDepthStencilTexture,
    Option<&'static MainPassResolutionOverride>,
);

/// Which draw: its args and its entry in the pass uniform.
#[derive(Clone, Copy)]
enum Draw {
    A,
    B,
    Translucent,
}

impl Draw {
    /// The word offset of the pass's indirect draw in the args buffer.
    fn args(self) -> u32 {
        match self {
            Self::A => ARGS_PASS_A,
            Self::B => ARGS_PASS_B,
            Self::Translucent => ARGS_TRANSLUCENT,
        }
    }

    /// The dynamic offset of the pass's entry in the draw pass uniform.
    fn pass_offset(self) -> u32 {
        let index = match self {
            Self::A => 0,
            Self::B => 1,
            Self::Translucent => 2,
        };
        index * DRAW_PASS_STRIDE as u32
    }
}

/// Encodes the frame's distant terrain work; see the module documentation.
pub(crate) fn distant_terrain_passes(
    world: &World,
    query: bevy::render::renderer::ViewQuery<PassQuery>,
    mut render_context: RenderContext,
) -> bevy::ecs::error::Result {
    let view_entity = query.entity();
    let (camera, target, scene_target, depth, resolution_override) = query.into_inner();
    let renderer = world.resource::<LodRenderer>();
    let (Some(frame), Some(state)) = (renderer.frame.as_ref(), renderer.state.as_ref()) else {
        return Ok(());
    };
    if frame.view != view_entity {
        return Ok(());
    }
    // `None` keeps the full target; `Some(None)` is a viewport outside the attachment.
    let viewport =
        Viewport::from_viewport_and_override(camera.viewport.as_ref(), resolution_override).map(
            |viewport| {
                crate::render_bounds::viewport(
                    &viewport,
                    crate::render_bounds::extent(scene_target.color_view(false)),
                )
            },
        );
    let encoder = render_context.command_encoder();
    encode(
        encoder,
        &state.gpu,
        frame,
        state.pyramid.as_ref(),
        &|| scene_target.color_attachment(target, false),
        &|| depth.get_attachment(StoreOp::Store),
        viewport.as_ref(),
    );
    finish(encoder, &state.gpu, frame);
    Ok(())
}

/// Encodes one frame's passes into `encoder`, drawing into the attachments `colour` and
/// `depth` make, within `viewport` (`None` for the whole target, `Some(None)` to draw
/// nothing), with an encoder-level timestamp before and after each pass.
pub(super) fn encode<'a>(
    encoder: &mut wgpu::CommandEncoder,
    gpu: &LodGpu,
    frame: &LodFrame,
    pyramid: Option<&LodPyramid>,
    colour: &dyn Fn() -> wgpu::RenderPassColorAttachment<'a>,
    depth: &dyn Fn() -> wgpu::RenderPassDepthStencilAttachment<'a>,
    viewport: Option<&Option<Viewport>>,
) {
    #[cfg(feature = "tracy")]
    let _span = bevy::log::info_span!(
        "distant_terrain.encode",
        commands = frame.scatter_groups,
        cluster_groups = frame.cluster_groups
    )
    .entered();
    let marks = gpu.timestamps.as_ref().map(|(queries, _)| queries);
    let mut mark = 0;
    let mut stamp = |encoder: &mut wgpu::CommandEncoder| {
        if let Some(queries) = marks {
            crate::gpu_timing::mark(encoder, queries, mark);
        }
        mark += 1;
    };
    if frame.clear {
        gpu.clear(encoder);
    }
    stamp(encoder);
    scatter(encoder, gpu, frame);
    stamp(encoder);
    // With no section slot in use every list is empty: skip the passes' attachment traffic.
    let drawing = frame.section_groups != 0;
    let draw = |encoder: &mut wgpu::CommandEncoder, draws: &[Draw]| {
        if drawing {
            draw_pass(encoder, gpu, frame, draws, colour(), depth(), viewport);
        }
    };
    draw(encoder, &[Draw::A]);
    stamp(encoder);
    // Nothing is culled against it without a section slot in use.
    if drawing && let Some(prepared) = pyramid {
        gpu.pyramid
            .encode(encoder, &prepared.pyramid, &prepared.bindings);
    }
    stamp(encoder);
    cull(encoder, gpu, frame);
    stamp(encoder);
    draw(encoder, &[Draw::B, Draw::Translucent]);
    stamp(encoder);
}

/// Applies the staged writes.
fn scatter(encoder: &mut wgpu::CommandEncoder, gpu: &LodGpu, frame: &LodFrame) {
    if frame.scatter_groups == 0 {
        return;
    }
    let mut pass = encoder.begin_compute_pass(&wgpu::ComputePassDescriptor {
        label: Some("distant terrain scatter"),
        timestamp_writes: None,
    });
    pass.set_pipeline(&gpu.scatter);
    pass.set_bind_group(0, &gpu.scatter_group, &[]);
    pass.dispatch_workgroups(frame.scatter_groups, 1, 1);
}

/// Culls sections, then compacts opaque and translucent clusters.
fn cull(encoder: &mut wgpu::CommandEncoder, gpu: &LodGpu, frame: &LodFrame) {
    // Culling adds to the statistics words; the counters before them are stored whole.
    let first = u64::from(ARGS_SECTION_BINS) * 4;
    encoder.clear_buffer(&gpu.args, first, Some(u64::from(ARGS_WORDS) * 4 - first));
    let mut pass = encoder.begin_compute_pass(&wgpu::ComputePassDescriptor {
        label: Some("distant terrain cull"),
        timestamp_writes: None,
    });
    pass.set_bind_group(0, &frame.cull_group, &[]);
    let kernels = &gpu.cull;
    // The scans always run, so empty frames still write zero draws.
    for (pipeline, groups) in [
        (&kernels.sections, frame.section_groups),
        (&kernels.count_opaque, frame.cluster_groups),
        (&kernels.scan_opaque, 1),
        (&kernels.emit_opaque, frame.cluster_groups),
        (&kernels.count_translucent, frame.order_groups),
        (&kernels.scan_translucent, 1),
        (&kernels.emit_translucent, frame.order_groups),
    ] {
        if groups != 0 {
            pass.set_pipeline(pipeline);
            pass.dispatch_workgroups(groups, 1, 1);
        }
    }
}

/// One render pass with one indirect draw of each of `draws`' lists, in order.
fn draw_pass(
    encoder: &mut wgpu::CommandEncoder,
    gpu: &LodGpu,
    frame: &LodFrame,
    draws: &[Draw],
    colour: wgpu::RenderPassColorAttachment<'_>,
    depth: wgpu::RenderPassDepthStencilAttachment<'_>,
    viewport: Option<&Option<Viewport>>,
) {
    let mut pass = encoder.begin_render_pass(&wgpu::RenderPassDescriptor {
        label: Some("distant terrain draw"),
        color_attachments: &[Some(colour)],
        depth_stencil_attachment: Some(depth),
        timestamp_writes: None,
        occlusion_query_set: None,
        multiview_mask: None,
    });
    match viewport {
        Some(Some(viewport)) => {
            pass.set_viewport(
                viewport.physical_position.x as f32,
                viewport.physical_position.y as f32,
                viewport.physical_size.x as f32,
                viewport.physical_size.y as f32,
                viewport.depth.start,
                viewport.depth.end,
            );
        }
        Some(None) => return,
        None => {}
    }
    pass.set_index_buffer(gpu.indices.slice(..), wgpu::IndexFormat::Uint16);
    for &which in draws {
        pass.set_pipeline(match which {
            Draw::Translucent => &gpu.translucent,
            Draw::A | Draw::B => &gpu.opaque,
        });
        pass.set_bind_group(0, &frame.draw_group, &[which.pass_offset()]);
        pass.draw_indexed_indirect(&gpu.args, u64::from(which.args()) * 4);
    }
}

/// Marks the frame encoded and copies the counters and timestamps out for readback.
pub(super) fn finish(encoder: &mut wgpu::CommandEncoder, gpu: &LodGpu, frame: &LodFrame) {
    frame.encoded.store(true, Ordering::Release);
    let Some(slot) = frame.readback else {
        return;
    };
    let buffers = &gpu.readbacks[slot];
    encoder.copy_buffer_to_buffer(&gpu.args, 0, &buffers.stats, 0, u64::from(ARGS_WORDS) * 4);
    if let (Some((queries, resolve)), Some(readback)) = (&gpu.timestamps, &buffers.timestamps) {
        encoder.resolve_query_set(queries, 0..TIMESTAMP_MARKS, resolve, 0);
        encoder.copy_buffer_to_buffer(resolve, 0, readback, 0, resolve.size());
    }
}
