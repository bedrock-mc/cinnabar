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

/// Uses the terrain visibility envelope for displaced and overhanging shadow casters.
fn intersects(bounds: &CascadeBounds, allocation: &GpuChunkAllocation) -> bool {
    let origin = queue::chunk_origin(allocation.key);
    let aabb = crate::chunk::bounds::aabb(true);
    let center = Vec3::from_array(origin.map(|value| value as f32)) + Vec3::from(aabb.center);
    bounds.intersects_aabb(center, Vec3::from(aabb.half_extents))
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
        assert!(
            intersects(&bounds, &allocation),
            "shadow cascade retains the displaced overhang"
        );
    }
}
