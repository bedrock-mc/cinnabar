//! Defers engine-owned world pipeline initializers while preserving their systems and resources.

use bevy::core_pipeline::deferred::copy_lighting_id::{
    CopyDeferredLightingIdPipeline, init_copy_deferred_lighting_id_pipeline,
};
use bevy::core_pipeline::mip_generation::experimental::depth::create_downsample_depth_pipelines;
use bevy::{
    ecs::schedule::{
        IntoSystemSet, ScheduleCleanupPolicy, ScheduleError, ScheduleLabel, common_conditions::not,
    },
    prelude::*,
    render::{
        Render, RenderStartup, RenderSystems,
        render_resource::{
            SparseBufferUpdateBindGroups, init_sparse_buffer_update_bind_groups,
            update_sparse_buffers,
        },
        renderer::{RenderGraph, RenderGraphSystems},
    },
};
use render_scene::startup_work::{WorldWorkAdmission, world_work_allowed};

/// Moves world-only initializers after the menu; direct joins retain the normal engine schedule.
pub(super) fn install(app: &mut SubApp) {
    if app
        .world()
        .get_resource::<WorldWorkAdmission>()
        .is_none_or(WorldWorkAdmission::is_admitted)
    {
        return;
    }
    if take(app, RenderStartup, init_copy_deferred_lighting_id_pipeline) {
        app.add_systems(
            Render,
            init_copy_deferred_lighting_id_pipeline
                .before(RenderSystems::PrepareViews)
                .run_if(not(resource_exists::<CopyDeferredLightingIdPipeline>))
                .run_if(world_work_allowed),
        );
    }
    if take(app, Render, create_downsample_depth_pipelines) {
        app.add_systems(
            Render,
            create_downsample_depth_pipelines
                .in_set(RenderSystems::Prepare)
                .run_if(world_work_allowed),
        );
    }
    if take(app, RenderStartup, init_sparse_buffer_update_bind_groups) {
        app.add_systems(
            Render,
            init_sparse_buffer_update_bind_groups
                .before(RenderSystems::PrepareViews)
                .run_if(not(resource_exists::<SparseBufferUpdateBindGroups>))
                .run_if(world_work_allowed),
        );
        if take(app, RenderGraph, update_sparse_buffers) {
            app.add_systems(
                RenderGraph,
                update_sparse_buffers
                    .in_set(RenderGraphSystems::Begin)
                    .run_if(resource_exists::<SparseBufferUpdateBindGroups>),
            );
        }
    }
}

/// Removes a typed initializer once, without relying on its diagnostic name or schedule layout.
fn take<M>(app: &mut SubApp, label: impl ScheduleLabel, set: impl IntoSystemSet<M>) -> bool {
    match app
        .world_mut()
        .try_schedule_scope(label, |world, schedule| {
            schedule.remove_systems_in_set(set, world, ScheduleCleanupPolicy::RemoveSystemsOnly)
        }) {
        Ok(Ok(count)) => count != 0,
        Ok(Err(ScheduleError::SetNotFound)) | Err(_) => false,
        Ok(Err(error)) => panic!("could not defer world startup pipeline: {error}"),
    }
}
