//! Per-frame lifetime of Bevy's binned opaque phase.

use bevy::{
    app::SubApp,
    core_pipeline::core_3d::Opaque3d,
    prelude::{IntoScheduleConfigs, ResMut, Resource},
    render::{Render, RenderSystems, render_phase::ViewBinnedRenderPhases},
};

#[derive(Resource)]
struct OpaquePhaseResetInstalled;

/// Every Opaque3d queue re-adds its items each frame, so bins must not outlive the frame.
/// Bevy's retained sweep ships with `bevy_pbr` and assumes one item per main entity.
pub(crate) fn install_opaque_phase_reset(render_app: &mut SubApp) {
    if render_app
        .world()
        .contains_resource::<OpaquePhaseResetInstalled>()
    {
        return;
    }
    render_app
        .insert_resource(OpaquePhaseResetInstalled)
        .add_systems(Render, reset_opaque_phases.in_set(RenderSystems::Cleanup));
}

/// Dropping the phases lets extraction rebuild them empty, so a pipeline keyed on a stale
/// view state (HDR, MSAA) is never drawn into the new target.
fn reset_opaque_phases(phases: Option<ResMut<ViewBinnedRenderPhases<Opaque3d>>>) {
    if let Some(mut phases) = phases {
        phases.clear();
    }
}

#[cfg(test)]
mod tests {
    use bevy::{
        asset::{AssetId, UntypedAssetId},
        core_pipeline::core_3d::{Opaque3d, Opaque3dBatchSetKey, Opaque3dBinKey},
        ecs::{change_detection::Tick, schedule::Schedule},
        mesh::Mesh,
        prelude::*,
        render::{
            batching::gpu_preprocessing::GpuPreprocessingMode,
            render_phase::{
                BinnedRenderPhaseType, Draw, DrawError, DrawFunctionId, DrawFunctions,
                InputUniformIndex, TrackedRenderPass, ViewBinnedRenderPhases,
            },
            render_resource::CachedRenderPipelineId,
            sync_world::MainEntity,
            view::RetainedViewEntity,
        },
    };

    /// Queue one view item; the bind-group index stands in for the HDR-keyed pipeline.
    fn queue(world: &mut World, view: RetainedViewEntity, hdr: bool) {
        let mut phases = world.resource_mut::<ViewBinnedRenderPhases<Opaque3d>>();
        phases.prepare_for_new_frame(view, GpuPreprocessingMode::None);
        phases.get_mut(&view).unwrap().add(
            Opaque3dBatchSetKey {
                draw_function: default_draw_function(),
                pipeline: CachedRenderPipelineId::INVALID,
                material_bind_group_index: Some(u32::from(hdr)),
                lightmap_slab: None,
                vertex_slab: default(),
                index_slab: None,
            },
            Opaque3dBinKey {
                asset_id: UntypedAssetId::from(AssetId::<Mesh>::invalid()),
            },
            (Entity::PLACEHOLDER, MainEntity::from(Entity::PLACEHOLDER)),
            InputUniformIndex::default(),
            BinnedRenderPhaseType::NonMesh,
            Tick::new(u32::from(hdr) + 1),
        );
    }

    struct NoDraw;

    impl Draw<Opaque3d> for NoDraw {
        fn draw<'w>(
            &mut self,
            _: &'w World,
            _: &mut TrackedRenderPass<'w>,
            _: Entity,
            _: &Opaque3d,
        ) -> Result<(), DrawError> {
            Ok(())
        }
    }

    fn default_draw_function() -> DrawFunctionId {
        DrawFunctions::<Opaque3d>::default().write().add(NoDraw)
    }

    #[test]
    fn hdr_switch_frame_draws_no_item_queued_for_the_old_target() {
        let mut world = World::new();
        world.init_resource::<ViewBinnedRenderPhases<Opaque3d>>();
        let mut cleanup = Schedule::default();
        cleanup.add_systems(super::reset_opaque_phases);
        let view = RetainedViewEntity::new(MainEntity::from(Entity::PLACEHOLDER), None, 0);

        queue(&mut world, view, false);
        cleanup.run(&mut world);
        queue(&mut world, view, true);

        let phases = world.resource::<ViewBinnedRenderPhases<Opaque3d>>();
        let keys = phases[&view]
            .non_mesh_items
            .keys()
            .map(|(batch_set, _)| batch_set.material_bind_group_index)
            .collect::<Vec<_>>();
        assert_eq!(keys, [Some(1)]);
    }
}
