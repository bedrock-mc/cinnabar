use bevy::prelude::*;
use bevy::window::{PrimaryWindow, WindowOccluded};
use std::time::Duration;
#[cfg(feature = "tracy")]
use std::time::Instant;
use {
    render::FramePacing,
    render_scene::{FrameBudgets, RuntimeStageProfiler},
};

use crate::present_mode::DisplayRefresh;

/// Builds rolling attribution and sends owned reports to the background console.
pub(crate) fn profiler(
    enabled: bool,
    trace_path: Option<std::path::PathBuf>,
) -> RuntimeStageProfiler {
    RuntimeStageProfiler::for_gameplay(enabled, trace_path)
        .with_diagnostic_sink(diagnostics::console::defer_stderr)
}

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

/// Starts trace export on the exit frame, before runner teardown can retain resources.
pub(crate) fn flush_trace_on_exit(
    profiler: Res<RuntimeStageProfiler>,
    mut exit: MessageReader<AppExit>,
) {
    if exit.read().next().is_some() {
        profiler.flush_trace();
    }
}

/// Completes export when native termination clears the world without returning from the runner.
#[derive(Resource)]
struct TraceExportOnTeardown(RuntimeStageProfiler);

impl Drop for TraceExportOnTeardown {
    /// Flushes and joins even while the runner's profiler handle remains alive.
    fn drop(&mut self) {
        bevy::log::info!(shutdown_phase = "trace_export_drop_body_begin");
        self.0.flush_trace();
        self.0.wait_for_trace_export();
        bevy::log::info!(shutdown_phase = "trace_export_drop_body_done");
    }
}

/// Completes export during world teardown or after a runner retains its world and returns.
pub(crate) fn run(app: &mut App) -> AppExit {
    let profiler = app.world().resource::<RuntimeStageProfiler>().clone();
    // Like TeardownWatchdog, install before runner cleanup adds blocking render teardown resources.
    app.insert_resource(TraceExportOnTeardown(profiler.clone()));
    bevy::log::info!(shutdown_phase = "app_run_enter");
    let exit = app.run();
    bevy::log::info!(shutdown_phase = "app_run_returned");
    bevy::log::info!(shutdown_phase = "post_run_trace_export_begin");
    profiler.flush_trace();
    profiler.wait_for_trace_export();
    bevy::log::info!(shutdown_phase = "post_run_trace_export_done");
    exit
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Native teardown must finish export before the runner returns, without an exit message.
    #[test]
    fn trace_export_finishes_during_world_teardown_before_runner_returns() {
        let root = tempfile::tempdir().unwrap();
        let path = root.path().join("teardown.json");
        let profiler = RuntimeStageProfiler::with_trace(true, Some(path.clone()));
        profiler.begin_frame(true, false);
        profiler.trace_frame(true, false, 0.0);
        let mut app = App::new();
        app.insert_resource(profiler);
        app.set_runner(move |mut owned| {
            assert!(owned.should_exit().is_none());
            owned.world_mut().clear_all();
            let saved: serde_json::Value = serde_json::from_slice(
                &std::fs::read(&path).expect("trace exported during teardown"),
            )
            .unwrap();
            assert_eq!(saved["traceEvents"][0]["frame_id"], 1);
            AppExit::Success
        });
        assert!(run(&mut app).is_success());
    }

    /// A runner retaining its world cannot leave an unfinished export at process exit.
    #[test]
    fn trace_export_finishes_after_a_runner_retains_the_world() {
        let root = tempfile::tempdir().unwrap();
        let path = root.path().join("shutdown.json");
        let profiler = RuntimeStageProfiler::with_trace(true, Some(path.clone()));
        profiler.begin_frame(true, false);
        profiler.trace_frame(true, false, 0.0);
        let retained = std::rc::Rc::new(std::cell::RefCell::new(None));
        let runner_app = retained.clone();
        let mut app = App::new();
        app.insert_resource(profiler);
        app.set_runner(move |app| {
            *runner_app.borrow_mut() = Some(app);
            AppExit::Success
        });
        assert!(run(&mut app).is_success());
        let saved: serde_json::Value =
            serde_json::from_slice(&std::fs::read(path).unwrap()).unwrap();
        assert_eq!(saved["traceEvents"][0]["frame_id"], 1);
        assert!(retained.borrow().is_some());
    }
}
