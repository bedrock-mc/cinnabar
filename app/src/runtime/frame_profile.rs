use bevy::ecs::system::NonSendMarker;
use bevy::prelude::*;
use bevy::window::{PrimaryWindow, WindowOccluded};
use bevy::winit::WINIT_WINDOWS;
use render::{FrameBudgets, FramePacing, RuntimeStageProfiler};
use std::time::{Duration, Instant};

const INTERVAL_RECHECK: Duration = Duration::from_secs(1);

/// Feeds slow-frame thresholds the primary window's monitor interval, or a slower frame cap.
pub(crate) fn track_frame_interval(
    profiler: Res<RuntimeStageProfiler>,
    windows: Query<Entity, With<PrimaryWindow>>,
    pacing: Option<Res<FramePacing>>,
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
