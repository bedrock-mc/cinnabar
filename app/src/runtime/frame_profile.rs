use bevy::ecs::system::NonSendMarker;
use bevy::prelude::*;
use bevy::window::{PrimaryWindow, WindowOccluded};
use bevy::winit::{UpdateMode, WINIT_WINDOWS, WinitSettings};
use render::{FrameBudgets, RuntimeStageProfiler};
use std::time::{Duration, Instant};

const INTERVAL_RECHECK: Duration = Duration::from_secs(1);

/// Feeds slow-frame thresholds the primary window's monitor interval, or a slower frame cap.
pub(crate) fn track_frame_interval(
    profiler: Res<RuntimeStageProfiler>,
    windows: Query<Entity, With<PrimaryWindow>>,
    pacing: Option<Res<WinitSettings>>,
    mut checked: Local<Option<Instant>>,
    _main_thread: NonSendMarker,
) {
    let now = Instant::now();
    if checked.is_some_and(|last| now.saturating_duration_since(last) < INTERVAL_RECHECK) {
        return;
    }
    *checked = Some(now);
    let refresh = windows.single().ok().and_then(|window| {
        WINIT_WINDOWS.with_borrow(|windows| {
            windows
                .get_window(window)?
                .current_monitor()?
                .refresh_rate_millihertz()
        })
    });
    let cap = pacing.and_then(|pacing| match pacing.focused_mode {
        UpdateMode::Reactive { wait, .. } => Some(wait),
        UpdateMode::Continuous => None,
    });
    profiler.set_frame_interval(FrameBudgets::display_interval(refresh, cap));
}

/// Records focus and OS occlusion with the optional bounded frame trace.
pub(crate) fn trace_frame_focus(
    profiler: Res<RuntimeStageProfiler>,
    windows: Query<&Window, With<PrimaryWindow>>,
    mut events: MessageReader<WindowOccluded>,
    mut occluded: Local<bool>,
) {
    for event in events.read() {
        *occluded = event.occluded;
    }
    let focused = windows.single().is_ok_and(|window| window.focused);
    profiler.begin_frame(focused, *occluded);
    profiler.trace_frame(focused, *occluded);
}

/// Saves recorded spans on the exit frame, before runner teardown can retain resources.
pub(crate) fn flush_trace_on_exit(
    profiler: Res<RuntimeStageProfiler>,
    mut exit: MessageReader<AppExit>,
) {
    if exit.read().next().is_some() {
        profiler.flush_trace();
    }
}
