//! Narrow access to the chunk arena for Enhanced shadow drawing.

use crate::chunk::*;
use crate::enhanced::CascadeBounds;
use bevy::shader::Shader;

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
    bounds: &CascadeBounds,
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
    let tint_identity = world.resource::<ChunkBiomeTints>().table_identity();
    let frame_probe = world.resource::<ActiveFrameProbe>();
    let eligible = |entity, allocation: &GpuChunkAllocation| {
        !arena.pending_removals.contains(&entity)
            && chunk_tint_identity_is_active(allocation.tint_identity, tint_identity)
            && frame_probe.accepts(
                entity,
                FrameAllocationIdentity {
                    entity,
                    key: allocation.key,
                    generation: allocation.generation,
                },
            )
            && intersects(bounds, allocation)
    };
    let Some(lightmap) = world.get_resource::<crate::lighting::LightmapGpu>() else {
        return;
    };
    pass.set_bind_group(0, bind_group, &[view_offset]);
    pass.set_bind_group(1, &lightmap.bind_group, &[]);
    pass.set_render_pipeline(cube_pipeline);
    pass.set_index_buffer(arena.index_buffer.slice(..), IndexFormat::Uint32);
    for (entity, resident) in &arena.allocations {
        let allocation = &resident.gpu;
        if !eligible(*entity, allocation) {
            continue;
        }
        let addresses = direct_stream_addresses(allocation);
        if !cube_stream_addresses_valid(&addresses) || !shared_stream_ranges_disjoint(&addresses) {
            continue;
        }
        let (Some(range), Some(base_vertex)) = (
            addresses.cube,
            metadata_base_vertex(allocation.metadata_index),
        ) else {
            continue;
        };
        pass.draw_indexed(0..STATIC_QUAD_INDICES.len() as u32, base_vertex, range);
    }
    pass.set_render_pipeline(model_pipeline);
    pass.set_index_buffer(arena.model_index_buffer.slice(..), IndexFormat::Uint32);
    for (entity, resident) in &arena.allocations {
        let allocation = &resident.gpu;
        if !eligible(*entity, allocation) {
            continue;
        }
        let Some(draw) = model_direct_draw_command(allocation) else {
            continue;
        };
        pass.draw_indexed(
            draw.first_index..draw.first_index + draw.index_count,
            draw.base_vertex,
            draw.first_instance..draw.first_instance + draw.instance_count,
        );
    }
}

/// Keeps one extra block around a subchunk for waving and overhanging models.
fn intersects(bounds: &CascadeBounds, allocation: &GpuChunkAllocation) -> bool {
    let origin = queue::chunk_origin(allocation.key);
    let size = queue::chunk_origin(SubChunkKey::new(0, 1, 0, 0))[0] as f32;
    let half = size * 0.5;
    let center = Vec3::from_array(origin.map(|value| value as f32)) + Vec3::splat(half);
    bounds.intersects_aabb(center, Vec3::splat(half + 1.0))
}
