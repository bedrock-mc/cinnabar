//! Bevy stock frame work that no client camera or entity feeds.
//!
//! `DefaultPlugins` installs systems for features the client never enables: 2D cameras;
//! prepass textures; order-independent transparency; occlusion depth pyramids; visibility
//! ranges; mesh entities; GPU-culled entities; depth of field; the lens effect stack and Bevy's
//! own motion blur. Each would still be dispatched every frame only to find no input.
//!
//! Removing a schedule node can reorder systems that have no ordering between them, so the
//! stock per-view passes and queue systems stay: unordered passes may share an attachment, and
//! unordered queue systems may add equal-distance transparent items. Stock extraction systems
//! also stay, because `ExtractSchedule` can only be rebuilt while the main world is inserted.

use bevy::{
    app::{App, Plugin, PostStartup, PostUpdate},
    camera::visibility::{calculate_bounds, check_visibility_gpu_culling, check_visibility_ranges},
    core_pipeline::{
        core_2d::{Transparent2d, prepare_core_2d_depth_textures},
        core_3d::{check_msaa, prepare_prepass_textures},
        mip_generation::experimental::depth::create_downsample_depth_pipelines,
        oit::{prepare_oit_buffers, resolve::prepare_oit_resolve_bind_group},
    },
    ecs::schedule::{
        InternedSystemSet, ScheduleCleanupPolicy, ScheduleLabel, common_conditions::run_once,
    },
    mesh::{
        mark_2d_meshes_as_changed_if_their_assets_changed,
        mark_3d_meshes_as_changed_if_their_assets_changed,
    },
    post_process::{bloom::BloomPlugin, msaa_writeback::MsaaWritebackPlugin},
    prelude::{IntoScheduleConfigs, IntoSystemSet, SystemSet, World},
    render::{
        Render, RenderApp, RenderStartup, render_phase::sort_phase_system,
        view::visibility::write_render_visibility_ranges,
    },
    transform::StaticTransformOptimizations,
};

#[cfg(test)]
mod tests;

/// Replaces Bevy's `PostProcessPlugin` with the two effects client cameras use, MSAA writeback
/// and bloom, and removes the stock systems whose inputs the client never creates.
///
/// Add it in place of `PostProcessPlugin`, after `CorePipelinePlugin`, `CameraPlugin`,
/// `MeshPlugin` and `TransformPlugin` have built.
pub struct StockSystemsPlugin;

impl Plugin for StockSystemsPlugin {
    fn build(&self, app: &mut App) {
        // No client entity has a parent, so static-tree dirty marking would track nothing.
        app.insert_resource(StaticTransformOptimizations::Disabled)
            .add_plugins((MsaaWritebackPlugin, BloomPlugin))
            .add_systems(PostStartup, remove_unused_main_systems);
        if let Some(render_app) = app.get_sub_app_mut(RenderApp) {
            // RenderStartup reruns after device recovery; the removed systems stay removed.
            render_app.add_systems(RenderStartup, remove_unused_render_systems.run_if(run_once));
        }
    }
}

/// Whether Bevy schedules a stock system on every adapter or only where its feature works.
#[derive(Clone, Copy, PartialEq, Eq)]
enum Scheduled {
    Always,
    WhereSupported,
}

/// Mesh-entity bounds and change marking, visibility ranges, GPU-culled entities and the
/// deferred-camera MSAA check: the client spawns no `Mesh3d`, `Mesh2d`, `VisibilityRange`,
/// `NoCpuCulling` or `DeferredPrepass` component.
fn remove_unused_main_systems(world: &mut World) {
    remove(
        world,
        PostUpdate,
        Scheduled::Always,
        [
            calculate_bounds.into_system_set().intern(),
            check_visibility_ranges.into_system_set().intern(),
            check_visibility_gpu_culling.into_system_set().intern(),
            mark_2d_meshes_as_changed_if_their_assets_changed
                .into_system_set()
                .intern(),
            mark_3d_meshes_as_changed_if_their_assets_changed
                .into_system_set()
                .intern(),
            check_msaa.into_system_set().intern(),
        ],
    );
}

/// 2D phase sorting and depth, prepass textures, order-independent transparency buffers, the
/// occlusion depth-pyramid pipelines and visibility-range uploads: no client camera carries
/// `Camera2d`, a prepass marker, `OrderIndependentTransparencySettings` or `OcclusionCulling`,
/// and no entity carries `VisibilityRange`. Without the depth-pyramid pipelines, Bevy's
/// pyramid preparation stays behind its existing `resource_exists` condition.
fn remove_unused_render_systems(world: &mut World) {
    remove(
        world,
        Render,
        Scheduled::Always,
        [
            sort_phase_system::<Transparent2d>
                .into_system_set()
                .intern(),
            prepare_core_2d_depth_textures.into_system_set().intern(),
            prepare_prepass_textures.into_system_set().intern(),
            prepare_oit_buffers.into_system_set().intern(),
            create_downsample_depth_pipelines.into_system_set().intern(),
            write_render_visibility_ranges.into_system_set().intern(),
        ],
    );
    // Bevy rebuilds this resolve bind group every frame, but only on adapters with OIT support.
    remove(
        world,
        Render,
        Scheduled::WhereSupported,
        [prepare_oit_resolve_bind_group.into_system_set().intern()],
    );
}

/// Removes each stock system from `label` while keeping the order between its neighbours.
///
/// Panics when Bevy no longer schedules an [`Scheduled::Always`] system there, so an entry
/// cannot go stale silently.
fn remove(
    world: &mut World,
    label: impl ScheduleLabel,
    scheduled: Scheduled,
    systems: impl IntoIterator<Item = InternedSystemSet>,
) {
    world.schedule_scope(label, |world, schedule| {
        for set in systems {
            let removed = schedule.remove_systems_in_set(
                set,
                world,
                ScheduleCleanupPolicy::RemoveSystemsOnly,
            );
            assert!(
                scheduled == Scheduled::WhereSupported
                    || removed.as_ref().is_ok_and(|&count| count != 0),
                "stock system {set:?} is no longer scheduled in {:?}: {removed:?}",
                schedule.label()
            );
        }
    });
}
