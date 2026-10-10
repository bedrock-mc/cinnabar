//! Single-sided opaque cube runs, drawn with back-face culling and only facing directions.

use bevy::ecs::query::Has;

use crate::chunk::*;

/// Eye for whole-direction culling, or `None` to keep every direction: the view is not an
/// unmirrored perspective with its eye at the view origin, or Enhanced may displace geometry.
pub(in crate::chunk) fn solid_cull_camera(
    view: &ExtractedView,
    enhanced: bool,
) -> Option<[f64; 3]> {
    let projection = view.clip_from_view;
    let perspective = projection.x_axis.w == 0.0
        && projection.y_axis.w == 0.0
        && projection.z_axis.w != 0.0
        && projection.w_axis.w == 0.0;
    // Mirroring flips which side of a face rasterises as front.
    let unmirrored = projection.determinant() > 0.0
        && view.world_from_view.affine().matrix3.determinant() > 0.0
        && !view.invert_culling;
    if enhanced || view.clip_from_world.is_some() || !perspective || !unmirrored {
        return None;
    }
    Some(view.world_from_view.translation().as_dvec3().to_array())
}

pub(in crate::chunk) struct ChunkSolidIndirectBatch {
    pub(in crate::chunk) camera: Option<[f64; 3]>,
    pub(in crate::chunk) cubes: ChunkIndirectBatch,
}

#[derive(Resource, Default)]
pub(in crate::chunk) struct ChunkSolidIndirectBatches(
    pub(in crate::chunk) HashMap<Entity, ChunkSolidIndirectBatch>,
);

/// Every valid cube stream counts as drawn, even when none of its solid runs face the camera.
pub(in crate::chunk) fn prepare_solid_indirect_batch_draws<'a>(
    allocations: impl IntoIterator<Item = (Entity, &'a GpuChunkAllocation)>,
    camera: Option<[f64; 3]>,
    frame_probe: &FrameProbeScope<'_>,
    active_tint_identity: ChunkBiomeTintIdentity,
) -> (
    Vec<DrawIndexedIndirectArgs>,
    Vec<(Entity, FrameAllocationIdentity)>,
) {
    let mut commands = Vec::new();
    let mut drawn = Vec::new();
    for (entity, allocation) in allocations {
        let Some(identity) =
            drawable_allocation_identity(frame_probe, entity, allocation, active_tint_identity)
        else {
            continue;
        };
        let Some(solid) = solid_indirect_commands(allocation, camera) else {
            continue;
        };
        commands.extend(solid);
        drawn.push((entity, identity));
    }
    (commands, drawn)
}

pub(in crate::chunk) type DrawSolidChunkCommands = crate::gpu_timing::GpuDrawSpan<
    { crate::RuntimeStage::GpuTerrainSolid as usize },
    (
        crate::chunk::gpu_cull::SkipOccludedTerrain,
        SetItemPipeline,
        crate::lighting::SetWorldLightmap,
        crate::enhanced::SetEnhancedViewBindGroup<2>,
        DrawPackedSolidChunk,
    ),
>;
pub(in crate::chunk) type DrawSolidChunkIndirectCommands = crate::gpu_timing::GpuDrawSpan<
    { crate::RuntimeStage::GpuTerrainSolid as usize },
    (
        SetItemPipeline,
        crate::lighting::SetWorldLightmap,
        crate::enhanced::SetEnhancedViewBindGroup<2>,
        DrawPackedSolidChunksIndirect,
    ),
>;

pub(in crate::chunk) struct DrawPackedSolidChunk;

impl<P: PhaseItem> RenderCommand<P> for DrawPackedSolidChunk {
    type Param = (
        SRes<ChunkGpuArena>,
        SRes<ActiveFrameProbe>,
        SRes<ActiveVisibilityFrameProbe>,
    );
    type ViewQuery = (
        Entity,
        Read<ViewUniformOffset>,
        Read<ExtractedView>,
        Has<crate::EnhancedRendering>,
    );
    type ItemQuery = Read<GpuChunkAllocation>;

    fn render<'w>(
        item: &P,
        (view_entity, view_offset, view, enhanced): ROQueryItem<'w, '_, Self::ViewQuery>,
        allocation: Option<ROQueryItem<'w, '_, Self::ItemQuery>>,
        (arena, frame_probe, visibility_probe): SystemParamItem<'w, '_, Self::Param>,
        pass: &mut TrackedRenderPass<'w>,
    ) -> RenderCommandResult {
        let arena = arena.into_inner();
        let frame_probe = frame_probe.into_inner();
        let (Some(bind_group), Some(allocation)) = (&arena.bind_group, allocation) else {
            return RenderCommandResult::Skip;
        };
        let identity = FrameAllocationIdentity {
            entity: item.entity(),
            key: allocation.key,
            generation: allocation.generation,
        };
        if !frame_probe.accepts(item.entity(), identity) {
            return RenderCommandResult::Skip;
        }
        let Some(commands) = solid_indirect_commands(allocation, solid_cull_camera(view, enhanced))
        else {
            return RenderCommandResult::Skip;
        };
        pass.set_bind_group(0, bind_group, &[view_offset.offset]);
        pass.set_index_buffer(arena.index_buffer.slice(..), IndexFormat::Uint32);
        pass.set_vertex_buffer(0, arena.vertex_offset_buffer.slice(..));
        for command in commands {
            pass.draw_indexed(
                command.first_index..command.first_index + command.index_count,
                command.base_vertex,
                command.first_instance..command.first_instance + command.instance_count,
            );
        }
        frame_probe.record_direct_draw(item.entity(), identity);
        record_visibility_direct_submission(
            visibility_probe.into_inner(),
            view_entity,
            allocation.key,
        );
        RenderCommandResult::Success
    }
}

pub(in crate::chunk) struct DrawPackedSolidChunksIndirect;

impl<P: PhaseItem> RenderCommand<P> for DrawPackedSolidChunksIndirect {
    type Param = (
        SRes<ChunkGpuArena>,
        SRes<ChunkSolidIndirectBatches>,
        SRes<ActiveFrameProbe>,
        SRes<ActiveVisibilityFrameProbe>,
    );
    type ViewQuery = OpaqueChunkViewQuery;
    type ItemQuery = ();

    fn render<'w>(
        item: &P,
        (view_entity, view_offset): ROQueryItem<'w, '_, Self::ViewQuery>,
        _item_query: Option<ROQueryItem<'w, '_, Self::ItemQuery>>,
        (arena, batches, frame_probe, visibility_probe): SystemParamItem<'w, '_, Self::Param>,
        pass: &mut TrackedRenderPass<'w>,
    ) -> RenderCommandResult {
        let arena = arena.into_inner();
        let Some(batch) = batches.into_inner().0.get(&item.entity()) else {
            return RenderCommandResult::Skip;
        };
        let Some(bind_group) = &arena.bind_group else {
            return RenderCommandResult::Skip;
        };
        let cubes = &batch.cubes;
        if cubes.command_count != 0 {
            pass.set_bind_group(0, bind_group, &[view_offset.offset]);
            pass.set_index_buffer(arena.index_buffer.slice(..), IndexFormat::Uint32);
            pass.set_vertex_buffer(0, arena.vertex_offset_buffer.slice(..));
            pass.multi_draw_indexed_indirect(
                &arena.indirect_buffer,
                cubes.indirect_offset,
                cubes.command_count,
            );
        }
        frame_probe
            .into_inner()
            .record_mdi_draws(cubes.drawn_allocations.iter().copied());
        record_visibility_mdi_submissions(
            visibility_probe.into_inner(),
            view_entity,
            cubes
                .drawn_allocations
                .iter()
                .map(|(_, identity)| identity.key),
        );
        RenderCommandResult::Success
    }
}

#[cfg(test)]
#[allow(clippy::single_range_in_vec_init)] // A one-element list of draw runs is the expectation.
mod tests {
    use super::*;

    fn view(clip_from_view: Mat4, world_from_view: Transform) -> ExtractedView {
        ExtractedView {
            retained_view_entity: bevy::render::view::RetainedViewEntity::new(
                MainEntity::from(Entity::PLACEHOLDER),
                None,
                0,
            ),
            clip_from_view,
            world_from_view: GlobalTransform::from(world_from_view),
            clip_from_world: None,
            hdr: false,
            viewport: UVec4::new(0, 0, 1, 1),
            color_grading: default(),
            invert_culling: false,
        }
    }

    fn perspective() -> Mat4 {
        Mat4::perspective_infinite_reverse_rh(1.2, 16.0 / 9.0, 0.05)
    }

    #[test]
    fn perspective_eye_culls_and_every_other_view_keeps_all_directions() {
        let pose = Transform::from_xyz(40.5, 70.25, -3.0).looking_to(Vec3::NEG_X, Vec3::Y);
        assert_eq!(
            solid_cull_camera(&view(perspective(), pose), false),
            Some([40.5, 70.25, -3.0])
        );
        assert_eq!(solid_cull_camera(&view(perspective(), pose), true), None);
        let orthographic = Mat4::orthographic_rh(-8.0, 8.0, -8.0, 8.0, 0.1, 100.0);
        assert_eq!(solid_cull_camera(&view(orthographic, pose), false), None);
        let mirrored = pose.with_scale(Vec3::new(-1.0, 1.0, 1.0));
        assert_eq!(
            solid_cull_camera(&view(perspective(), mirrored), false),
            None
        );
        let mut inverted = view(perspective(), pose);
        inverted.invert_culling = true;
        assert_eq!(solid_cull_camera(&inverted, false), None);
        let mut explicit = view(perspective(), pose);
        explicit.clip_from_world = Some(perspective());
        assert_eq!(solid_cull_camera(&explicit, false), None);
    }

    fn allocation(layout: CubeQuadLayout) -> GpuChunkAllocation {
        GpuChunkAllocation {
            key: SubChunkKey::new(0, 1, 4, -1),
            generation: 1,
            tint_identity: ChunkBiomeTintIdentity::default(),
            quad_range: 100..112,
            cube_layout: layout,
            cube_lighting_range: Some(200..224),
            model_range: None,
            model_lighting_range: None,
            model_draw_range: None,
            transparent_model_draw_range: None,
            liquid_range: None,
            liquid_lighting_range: None,
            has_depth_liquid: false,
            has_transparent_liquid: false,
            depth_liquid_range: None,
            order_independent_liquid: false,
            metadata_index: 3,
        }
    }

    fn instances(commands: impl IntoIterator<Item = DrawIndexedIndirectArgs>) -> Vec<Range<u32>> {
        commands
            .into_iter()
            .map(|command| {
                assert_eq!(command.base_vertex, 12);
                assert_eq!(command.index_count, STATIC_QUAD_INDICES.len() as u32);
                command.first_instance..command.first_instance + command.instance_count
            })
            .collect()
    }

    #[test]
    fn solid_draws_cover_facing_runs_and_cutout_draws_the_rest() {
        // Face::ALL counts; slot order -X 1, -Y 2, -Z 1, +X 1, +Y 2, +Z 1 fills 100..108.
        let layout = CubeQuadLayout::from_solid_counts([1, 1, 2, 2, 1, 1]);
        let allocation = allocation(layout);
        let origin = chunk_origin(allocation.key);
        assert_eq!(origin, [16, 64, -16]);
        let everything = solid_indirect_commands(&allocation, None).unwrap();
        assert_eq!(instances(everything), [100..108]);
        // Above, east and south of the bounds: +X, +Y and +Z only.
        let camera = [40.0, 90.0, 10.0];
        let facing = solid_indirect_commands(&allocation, Some(camera)).unwrap();
        assert_eq!(instances(facing), [104..108]);
        // Inside the bounds every run is drawn.
        let inside = solid_indirect_commands(&allocation, Some([20.0, 70.0, -8.0])).unwrap();
        assert_eq!(instances(inside), [100..108]);
        assert_eq!(instances(cutout_indirect_command(&allocation)), [108..112]);
        assert!(cube_stream_drawable(&allocation));
    }

    #[test]
    fn default_or_oversized_layouts_draw_every_quad_through_the_cutout_path() {
        let default = allocation(CubeQuadLayout::default());
        assert_eq!(solid_indirect_commands(&default, None).unwrap().count(), 0);
        assert_eq!(instances(cutout_indirect_command(&default)), [100..112]);
        let oversized = allocation(CubeQuadLayout::from_solid_counts([3; 6]));
        assert_eq!(
            solid_indirect_commands(&oversized, None).unwrap().count(),
            0
        );
        assert_eq!(instances(cutout_indirect_command(&oversized)), [100..112]);
        let all_solid = allocation(CubeQuadLayout::from_solid_counts([2; 6]));
        assert!(cutout_indirect_command(&all_solid).is_none());
        let mut invalid = all_solid;
        invalid.cube_lighting_range = None;
        assert!(solid_indirect_commands(&invalid, None).is_none());
    }
}
