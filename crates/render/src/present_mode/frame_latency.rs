//! Refreshes Bevy's private surface configuration when the requested frame latency changes.

use std::num::NonZeroU32;

#[cfg(test)]
use bevy::prelude::{Result, World};
use bevy::{
    prelude::{Commands, Entity, IntoScheduleConfigs, Query, Res, ResMut, Resource, SubApp, With},
    render::{
        Extract, ExtractSchedule, Render, RenderSystems,
        view::{
            ViewTarget, ViewTargetAttachments,
            window::{ExtractedWindows, WindowSurfaces, create_surfaces},
        },
    },
    window::Window,
};

#[cfg(test)]
mod tests;

/// One queued frame with display pacing, two to overlap CPU and GPU work without VSync.
#[must_use]
pub const fn frame_latency_for_vsync(vsync: bool) -> NonZeroU32 {
    if vsync {
        NonZeroU32::MIN
    } else {
        NonZeroU32::new(2).expect("two is nonzero")
    }
}

#[derive(Resource, Default)]
struct ChangedFrameLatency(bool);

/// Installs extraction and surface invalidation before Bevy configures or acquires a frame.
pub(super) fn install(render_app: &mut SubApp) {
    render_app
        .init_resource::<ChangedFrameLatency>()
        .add_systems(ExtractSchedule, extract_frame_latency)
        .add_systems(
            Render,
            recreate_surfaces
                .run_if(frame_latency_changed)
                .after(RenderSystems::ExtractCommands)
                .before(create_surfaces),
        );
}

/// Keeps native surface work, including its main-thread dispatch, off unchanged frames.
fn frame_latency_changed(changed: Res<ChangedFrameLatency>) -> bool {
    changed.0
}

/// Copies live latency changes; Bevy only copies this field on the first extraction.
fn extract_frame_latency(
    windows: Extract<Query<(Entity, &Window)>>,
    mut extracted: ResMut<ExtractedWindows>,
    mut changed: ResMut<ChangedFrameLatency>,
) {
    for (entity, window) in &windows {
        if let Some(extracted) = extracted.windows.get_mut(&entity)
            && extracted.desired_maximum_frame_latency != window.desired_maximum_frame_latency
        {
            extracted.desired_maximum_frame_latency = window.desired_maximum_frame_latency;
            changed.0 = true;
        }
    }
}

/// Drops retained outputs before rebuilding the surface cache with the new frame latency.
fn recreate_surfaces(
    #[cfg(any(target_os = "macos", target_os = "ios"))] _marker: bevy::ecs::system::NonSendMarker,
    mut commands: Commands,
    mut changed: ResMut<ChangedFrameLatency>,
    mut windows: ResMut<ExtractedWindows>,
    mut surfaces: ResMut<WindowSurfaces>,
    mut attachments: ResMut<ViewTargetAttachments>,
    targets: Query<Entity, With<ViewTarget>>,
) {
    if !std::mem::take(&mut changed.0) {
        return;
    }
    for entity in &targets {
        commands.entity(entity).remove::<ViewTarget>();
    }
    attachments.clear();
    for window in windows.values_mut() {
        window.swap_chain_texture_view = None;
        window.swap_chain_texture = None;
    }
    // Bevy exposes no per-window surface removal or mutable configuration. Rebuilding this
    // cache makes create_surfaces consume the updated latency, including for latency-only edits.
    *surfaces = WindowSurfaces::default();
}
