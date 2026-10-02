use bevy::prelude::*;
use bevy::window::{PrimaryWindow, WindowOccluded};
use render::RuntimeStageProfiler;

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
    if let Ok(window) = windows.single() {
        profiler.trace_frame(window.focused, *occluded);
    }
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
