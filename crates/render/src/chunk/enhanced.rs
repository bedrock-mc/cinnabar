//! Narrow access to the chunk arena for Enhanced shadow drawing.

use crate::chunk::*;
use crate::enhanced::CascadeBounds;
use bevy::camera::primitives::{Aabb, Frustum};
use bevy::shader::Shader;

mod batches;
pub(crate) use batches::EnhancedGeometryCache;
use batches::{Region, SubmissionKind};

pub(crate) fn install_geometry_cache(render_app: &mut bevy::app::SubApp) {
    render_app
        .init_resource::<EnhancedGeometryCache>()
        .add_systems(
            Render,
            batches::prepare_enhanced_geometry.in_set(RenderSystems::PrepareBindGroups),
        );
}

/// Returns the existing vertex-pulling layout and shader handles.
pub(crate) fn shadow_sources(
    world: &World,
) -> (BindGroupLayoutDescriptor, Handle<Shader>, Handle<Shader>) {
    let pipeline = world.resource::<ChunkPipeline>();
    (
        pipeline.bind_group_layout.clone(),
        CHUNK_SHADER_HANDLE,
        MODEL_SHADER_HANDLE,
    )
}

/// Draws resident terrain intersecting a light-space cascade, including offscreen casters.
pub(crate) fn draw_shadow_geometry<'w>(
    world: &'w World,
    view: Entity,
    cascade: usize,
    bounds: &CascadeBounds,
    view_offset: u32,
    pass: &mut TrackedRenderPass<'w>,
    cube_pipeline: &'w RenderPipeline,
    model_pipeline: &'w RenderPipeline,
) {
    draw_geometry(
        world,
        view,
        SubmissionKind::Shadow(cascade),
        Region::Shadow(*bounds),
        view_offset,
        pass,
        cube_pipeline,
        model_pipeline,
    );
}

/// Resident terrain inside one point-light shadow face, including offscreen casters.
pub(crate) fn draw_local_light_geometry<'w>(
    world: &'w World,
    view: Entity,
    index: usize,
    clip: &Mat4,
    view_offset: u32,
    pass: &mut TrackedRenderPass<'w>,
    cube: &'w RenderPipeline,
    model: &'w RenderPipeline,
) {
    draw_geometry(
        world,
        view,
        SubmissionKind::LocalLight(index),
        Region::Camera(*clip),
        view_offset,
        pass,
        cube,
        model,
    );
}

/// Camera-visible alpha coverage for the Enhanced screen-space visibility pass.
pub(crate) fn draw_depth_geometry<'w>(
    world: &'w World,
    view: Entity,
    clip: &Mat4,
    view_offset: u32,
    pass: &mut TrackedRenderPass<'w>,
    cube_pipeline: &'w RenderPipeline,
    model_pipeline: &'w RenderPipeline,
) {
    draw_geometry(
        world,
        view,
        SubmissionKind::Camera,
        Region::Camera(*clip),
        view_offset,
        pass,
        cube_pipeline,
        model_pipeline,
    );
}

fn draw_geometry<'w>(
    world: &'w World,
    view: Entity,
    kind: SubmissionKind,
    region: Region,
    view_offset: u32,
    pass: &mut TrackedRenderPass<'w>,
    cube_pipeline: &'w RenderPipeline,
    model_pipeline: &'w RenderPipeline,
) {
    let Some(arena) = world.get_resource::<ChunkGpuArena>() else {
        return;
    };
    let Some(bind_group) = &arena.bind_group else {
        return;
    };
    let Some(cache) = world.get_resource::<EnhancedGeometryCache>() else {
        return;
    };
    let Some(batch) = cache.batch(view, kind, region) else {
        return;
    };
    let Some(lightmap) = world.get_resource::<crate::lighting::LightmapGpu>() else {
        return;
    };
    pass.set_bind_group(0, bind_group, &[view_offset]);
    pass.set_bind_group(1, &lightmap.bind_group, &[]);
    pass.set_render_pipeline(cube_pipeline);
    pass.set_index_buffer(arena.index_buffer.slice(..), IndexFormat::Uint32);
    draw_batch_commands(batch, cache.indirect_enabled, 0..batch.cube_count, pass);
    pass.set_render_pipeline(model_pipeline);
    pass.set_index_buffer(arena.model_index_buffer.slice(..), IndexFormat::Uint32);
    draw_batch_commands(
        batch,
        cache.indirect_enabled,
        batch.cube_count..batch.commands.len(),
        pass,
    );
}

fn draw_batch_commands<'w>(
    batch: &'w batches::GeometryBatch,
    indirect: bool,
    range: Range<usize>,
    pass: &mut TrackedRenderPass<'w>,
) {
    if range.is_empty() {
        return;
    }
    if indirect && let Some(buffer) = &batch.indirect {
        pass.multi_draw_indexed_indirect(
            buffer,
            range.start as u64 * INDEXED_INDIRECT_BYTES,
            range.len() as u32,
        );
    } else {
        for draw in &batch.commands[range] {
            pass.draw_indexed(
                draw.first_index..draw.first_index + draw.index_count,
                draw.base_vertex,
                draw.first_instance..draw.first_instance + draw.instance_count,
            );
        }
    }
}

/// Keeps one extra block around a subchunk for waving and overhanging models.
fn chunk_bounds(allocation: &GpuChunkAllocation) -> (Vec3, Vec3) {
    let origin = queue::chunk_origin(allocation.key);
    let size = queue::chunk_origin(SubChunkKey::new(0, 1, 0, 0))[0] as f32;
    let half = size * 0.5;
    let center = Vec3::from_array(origin.map(|value| value as f32)) + Vec3::splat(half);
    (center, Vec3::splat(half + 1.0))
}
