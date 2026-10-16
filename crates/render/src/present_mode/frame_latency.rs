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
            window::{ExtractedWindow, SurfaceData, create_surfaces},
        },
    },
    window::Window,
};

#[cfg(test)]
mod tests;

/// The surface latency for this platform's presentation path; see [`frame_latency`].
#[must_use]
pub fn frame_latency_for_vsync(vsync: bool) -> NonZeroU32 {
    frame_latency(vsync, metal_display_link_delivery())
}

/// One queued frame with VSync, or two when a display link holds a prefetched drawable;
/// two without VSync to overlap CPU and GPU work.
const fn frame_latency(vsync: bool, display_link: bool) -> NonZeroU32 {
    if vsync && !display_link {
        NonZeroU32::MIN
    } else {
        NonZeroU32::new(2).expect("two is nonzero")
    }
}

/// Whether the Metal backend delivers synchronized drawables through a display link.
fn metal_display_link_delivery() -> bool {
    #[cfg(target_os = "macos")]
    {
        wgpu::hal::metal::display_link_delivery_supported()
    }
    #[cfg(not(target_os = "macos"))]
    {
        false
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
    windows: Extract<Query<&Window>>,
    mut extracted: Query<(bevy::render::sync_world::MainEntity, &mut ExtractedWindow)>,
    mut changed: ResMut<ChangedFrameLatency>,
) {
    for (entity, mut extracted) in &mut extracted {
        if let Ok(window) = windows.get(entity)
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
    mut windows: Query<(Entity, &mut ExtractedWindow)>,
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
    for (entity, mut window) in &mut windows {
        commands.entity(entity).remove::<SurfaceData>();
        window.swap_chain_texture_view = None;
        window.swap_chain_texture = None;
    }
}
