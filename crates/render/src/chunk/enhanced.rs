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

/// Returns the terrain shader layout, handles, and matching vertex-offset buffer layout.
pub(crate) fn shadow_sources(
    world: &World,
) -> (
    BindGroupLayoutDescriptor,
    Handle<Shader>,
    Handle<Shader>,
    bevy::mesh::VertexBufferLayout,
) {
    let pipeline = world.resource::<ChunkPipeline>();
    (
        pipeline.bind_group_layout.clone(),
        CHUNK_SHADER_HANDLE,
        MODEL_SHADER_HANDLE,
        pipeline::layouts::draw_offsets_layout(),
    )
}

/// Draws resident terrain intersecting a light-space cascade, including offscreen casters.
#[allow(
    clippy::too_many_arguments,
    reason = "Draw submission keeps the view, region and two geometry pipelines explicit."
)]
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
#[allow(
    clippy::too_many_arguments,
    reason = "Draw submission keeps the view, region and two geometry pipelines explicit."
)]
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

#[allow(
    clippy::too_many_arguments,
    reason = "Draw submission keeps the view, region and two geometry pipelines explicit."
)]
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
    pass.set_vertex_buffer(0, arena.vertex_offset_buffer.slice(..));
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

/// Uses the terrain visibility envelope for waving and overhanging models.
fn chunk_bounds(allocation: &GpuChunkAllocation) -> (Vec3, Vec3) {
    let origin = queue::chunk_origin(allocation.key);
    let aabb = crate::chunk::bounds::aabb(true);
    let center = Vec3::from_array(origin.map(|value| value as f32)) + Vec3::from(aabb.center);
    (center, Vec3::from(aabb.half_extents))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn displaced_model_shadow_caster_survives_outer_cascade() {
        let allocation = GpuChunkAllocation {
            key: SubChunkKey::new(0, 0, 0, 0),
            generation: 1,
            tint_identity: ChunkBiomeTintIdentity::default(),
            quad_range: 0..0,
            cube_layout: CubeQuadLayout::default(),
            cube_lighting_range: None,
            model_range: Some(0..8),
            model_lighting_range: Some(8..10),
            model_draw_range: Some(0..1),
            transparent_model_draw_range: None,
            liquid_range: None,
            liquid_lighting_range: None,
            has_depth_liquid: false,
            has_transparent_liquid: false,
            depth_liquid_range: None,
            order_independent_liquid: false,
            metadata_index: 0,
        };
        let bounds = CascadeBounds {
            light_from_world: Mat4::IDENTITY,
            min: Vec3::new(17.25, 0.0, 0.0),
            max: Vec3::new(17.4, 16.0, 16.0),
        };
        let (center, half_extents) = chunk_bounds(&allocation);
        assert!(
            bounds.intersects_aabb(center, half_extents),
            "shadow cascade retains the displaced overhang"
        );
    }
}
