use bevy::prelude::*;
use bevy::window::{PrimaryWindow, WindowOccluded};
use render::{FrameBudgets, FramePacing, RuntimeStageProfiler};
use std::time::Duration;
#[cfg(feature = "tracy")]
use std::time::Instant;

use crate::present_mode::DisplayRefresh;

/// Feeds slow-frame thresholds the primary window's monitor interval, or a slower frame cap.
pub(crate) fn track_frame_interval(
    profiler: Res<RuntimeStageProfiler>,
    display: Option<Res<DisplayRefresh>>,
    pacing: Option<Res<FramePacing>>,
) {
    if !display.as_ref().is_some_and(|display| display.is_changed())
        && !pacing.as_ref().is_some_and(|pacing| pacing.is_changed())
    {
        return;
    }
    let refresh = display
        .and_then(|display| display.0.refresh)
        .map(|rate| rate.millihertz());
    let cap = pacing
        .and_then(|pacing| pacing.rate)
        .map(|rate| Duration::from_nanos(rate.period_nanos()));
    profiler.set_frame_interval(FrameBudgets::display_interval(refresh, cap));
}

/// Records focus and OS occlusion with the optional bounded frame trace.
pub(crate) fn trace_frame_focus(
    time: Res<Time>,
    profiler: Res<RuntimeStageProfiler>,
    windows: Query<&Window, With<PrimaryWindow>>,
    mut events: MessageReader<WindowOccluded>,
    mut occluded: Local<bool>,
    #[cfg(feature = "tracy")] mut anchor: Local<Option<Instant>>,
) {
    #[cfg(feature = "tracy")]
    if anchor.is_none_or(|last| last.elapsed() >= Duration::from_secs(1)) {
        *anchor = Some(Instant::now());
        let _window = bevy::log::info_span!("profile.clock_window").entered();
        let unix_ns = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap_or_default()
            .as_nanos() as u64;
        let _zone = bevy::log::info_span!(
            "profile.clock_anchor",
            unix_ns,
            game_seconds = time.elapsed_secs_f64()
        )
        .entered();
    }
    for event in events.read() {
        *occluded = event.occluded;
    }
    let focused = windows.single().is_ok_and(|window| window.focused);
    profiler.begin_frame(focused, *occluded);
    profiler.trace_frame(focused, *occluded, time.elapsed_secs_f64());
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
