//! Narrow access to the chunk arena for Enhanced shadow drawing.

use render_scene::ChunkBiomeTints;

use crate::chunk::*;
use crate::enhanced::CascadeBounds;
use bevy::{prelude::Handle, shader::Shader};

/// Returns the existing vertex-pulling layout, shader handles, and the vertex buffer 0 layout
/// their terrain vertex entries read; [`draw_shadow_geometry`] binds the matching buffer.
pub(crate) fn shadow_sources(
    world: &World,
) -> (
    BindGroupLayoutDescriptor,
    Handle<Shader>,
    Handle<Shader>,
    bevy::mesh::VertexBufferLayout,
    bool,
) {
    let pipeline = world.resource::<ChunkPipeline>();
    (
        pipeline.bind_group_layout.clone(),
        CHUNK_SHADER_HANDLE,
        MODEL_SHADER_HANDLE,
        pipeline::layouts::draw_offsets_layout(),
        pipeline.compact,
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
    let Some(lightmap) = world.get_resource::<crate::lighting::LightmapGpu>() else {
        return;
    };
    let envelope = CascadeEnvelope::new(bounds);
    // One pass decides eligibility for both pipelines, keeping the map's iteration order.
    let frame_probe = world.resource::<ActiveFrameProbe>().scope();
    let eligible: Vec<&GpuChunkAllocation> = arena
        .allocations
        .iter()
        .map(|(&entity, resident)| (entity, &resident.gpu))
        .filter(|&(entity, allocation)| {
            chunk_tint_identity_is_active(allocation.tint_identity, tint_identity)
                && envelope.intersects(allocation)
                && !arena.pending_removals.contains(&entity)
                && frame_probe.accepts(
                    entity,
                    FrameAllocationIdentity {
                        entity,
                        key: allocation.key,
                        generation: allocation.generation,
                    },
                )
        })
        .map(|(_, allocation)| allocation)
        .collect();
    drop(frame_probe);
    pass.set_bind_group(0, bind_group, &[view_offset]);
    pass.set_bind_group(1, &lightmap.bind_group, &[]);
    pass.set_render_pipeline(cube_pipeline);
    pass.set_index_buffer(arena.index_buffer.slice(..), IndexFormat::Uint32);
    pass.set_vertex_buffer(0, arena.vertex_offset_buffer.slice(..));
    for &allocation in &eligible {
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
    for &allocation in &eligible {
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

/// The terrain visibility envelope for displaced and overhanging shadow casters, as one cascade
/// sees it; the envelope's light-space radius depends only on the cascade, so it is built once.
struct CascadeEnvelope<'a> {
    bounds: &'a CascadeBounds,
    center: Vec3,
    radius: Vec3,
}

impl<'a> CascadeEnvelope<'a> {
    /// Rotates the shared envelope into `bounds`' light space.
    fn new(bounds: &'a CascadeBounds) -> Self {
        let aabb = crate::chunk::bounds::aabb(true);
        Self {
            bounds,
            center: Vec3::from(aabb.center),
            radius: bounds.rotated_radius(Vec3::from(aabb.half_extents)),
        }
    }

    /// Whether the envelope around `allocation`'s sub-chunk overlaps the cascade.
    fn intersects(&self, allocation: &GpuChunkAllocation) -> bool {
        let origin = render_scene::chunk::chunk_origin(allocation.key);
        let center = Vec3::from_array(origin.map(|value| value as f32)) + self.center;
        self.bounds.intersects_rotated(center, self.radius)
    }
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
            CascadeEnvelope::new(&bounds).intersects(&allocation),
            "shadow cascade retains the displaced overhang"
        );
    }
}
