//! Reports native Metal presentation and software pacing when either changes.

use std::time::Instant;

use bevy::{
    ecs::system::NonSendMarker,
    prelude::{App, Entity, IntoScheduleConfigs, Last, Local, Query, Res, With},
    window::{PresentMode, PrimaryWindow, Window},
    winit::WINIT_WINDOWS,
};
use objc2::{MainThreadMarker, Message, rc::Retained};
use objc2_app_kit::{NSView, NSWindowStyleMask};
use objc2_quartz_core::CAMetalLayer;
use render::FramePacing;
use render_model::{FrameRate, PresentModeKind, VrrStatus};
use winit::raw_window_handle::{HasWindowHandle, RawWindowHandle};

use super::{DISPLAY_RECHECK, DisplayRefresh, update_frame_pacing};

#[derive(Clone, Copy, Debug, PartialEq)]
struct PresentationSnapshot {
    requested: PresentMode,
    display_sync_enabled: bool,
    presents_with_transaction: bool,
    maximum_drawable_count: usize,
    fullscreen: bool,
    software_cap: Option<FrameRate>,
    refresh: Option<FrameRate>,
    vrr: VrrStatus,
}

/// Samples on AppKit's thread after the current software cadence has been selected.
pub(crate) fn install(app: &mut App) {
    app.add_systems(Last, log_presentation.after(update_frame_pacing));
}

/// Reads the existing renderer layer without creating a probe surface or changing its state.
fn metal_layer(window: &winit::window::Window) -> Option<(Retained<CAMetalLayer>, bool)> {
    let _main_thread = MainThreadMarker::new()?;
    let RawWindowHandle::AppKit(handle) = window.window_handle().ok()?.as_raw() else {
        return None;
    };
    // SAFETY: winit owns this live NSView, and AppKit access stays on the main thread.
    let view = unsafe { &*handle.ns_view.as_ptr().cast::<NSView>() };
    let fullscreen = view
        .window()?
        .styleMask()
        .contains(NSWindowStyleMask::FullScreen);
    let root = view.layer()?;
    let layer = root
        .downcast_ref::<CAMetalLayer>()
        .map(|layer| layer.retain())
        .or_else(|| {
            // SAFETY: The layer tree is read on the main thread. wgpu attaches its Metal layer
            // directly below the view's backing layer when it is not itself a Metal layer.
            unsafe { root.sublayers() }?.iter().find_map(|layer| {
                layer
                    .downcast_ref::<CAMetalLayer>()
                    .map(|layer| layer.retain())
            })
        })?;
    layer.device()?;
    Some((layer, fullscreen))
}

/// Logs actual layer properties and the application cap, at most once per display recheck.
fn log_presentation(
    windows: Query<(Entity, &Window), With<PrimaryWindow>>,
    pacing: Res<FramePacing>,
    display: Res<DisplayRefresh>,
    settings: Res<client_state::RuntimeSettings>,
    mut checked: Local<Option<Instant>>,
    mut previous: Local<Option<(Entity, PresentationSnapshot)>>,
    _main_thread: NonSendMarker,
) {
    let now = Instant::now();
    if checked.is_some_and(|last| now.saturating_duration_since(last) < DISPLAY_RECHECK) {
        return;
    }
    *checked = Some(now);
    let Ok((entity, window)) = windows.single() else {
        *previous = None;
        return;
    };
    let native = WINIT_WINDOWS.with_borrow(|windows| metal_layer(windows.get_window(entity)?));
    let Some((layer, fullscreen)) = native else {
        *previous = None;
        return;
    };
    let timing = display
        .0
        .with_vrr_preference(settings.user_settings_update().1.video.vrr);
    let snapshot = PresentationSnapshot {
        requested: window.present_mode,
        display_sync_enabled: layer.displaySyncEnabled(),
        presents_with_transaction: layer.presentsWithTransaction(),
        maximum_drawable_count: layer.maximumDrawableCount(),
        fullscreen,
        software_cap: pacing.rate,
        refresh: timing.refresh,
        vrr: timing.vrr,
    };
    if previous
        .as_ref()
        .is_some_and(|last| *last == (entity, snapshot))
    {
        return;
    }
    let effective = if snapshot.display_sync_enabled {
        PresentModeKind::Fifo
    } else {
        PresentModeKind::Immediate
    };
    bevy::log::info!(
        requested = ?snapshot.requested,
        effective = effective.name(),
        displaySyncEnabled = snapshot.display_sync_enabled,
        presentsWithTransaction = snapshot.presents_with_transaction,
        maximumDrawableCount = snapshot.maximum_drawable_count,
        fullscreen = snapshot.fullscreen,
        software_cap_hz = ?snapshot.software_cap.map(|rate| f64::from(rate.millihertz()) / 1_000.0),
        refresh_hz = ?snapshot.refresh.map(|rate| f64::from(rate.millihertz()) / 1_000.0),
        vrr = ?snapshot.vrr,
        "Metal presentation"
    );
    *previous = Some((entity, snapshot));
}
