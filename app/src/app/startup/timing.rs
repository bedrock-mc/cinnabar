//! Wall-clock instrumentation for startup carrier I/O and the first frame.
use assets::carriers::startup::CarrierLoadObserver;
use bevy::prelude::{App, IntoScheduleConfigs, Last, Local, Res, Resource};
use std::time::Instant;

/// When the event loop started.
#[derive(Resource)]
struct EventLoopStart(Instant);

#[derive(Resource)]
struct UpdateStart(Instant);

/// Logs, on the first frame, the time since launch and how much of it preceded the event loop.
pub(crate) fn install_first_frame_log(app: &mut App) {
    if let Some(timing) = render_scene::startup_timing::current() {
        timing.record(
            "before_event_loop",
            crate::lifecycle::launched_at().elapsed(),
        );
    }
    app.insert_resource(EventLoopStart(Instant::now()))
        .insert_resource(UpdateStart(Instant::now()))
        .add_systems(bevy::prelude::First, begin_update.run_if(recording))
        .add_systems(
            Last,
            (
                finish_update.run_if(recording),
                log_first_frame,
                log_presented_menu.run_if(report_pending),
            )
                .chain(),
        );
}

/// Emits a report only for completed menu content; world launches have a different endpoint.
pub(super) fn log_presented_menu(
    presentation: Res<render_scene::startup_presentation::StartupPresentation>,
    menu: Res<client_shell::menu::MenuRuntime>,
    mut exit: bevy::prelude::MessageWriter<bevy::prelude::AppExit>,
) {
    if !menu.is_visible() {
        return;
    }
    if let Some(timing) = render_scene::startup_timing::current()
        && let Some(presented) = presentation
            .content_presented_at()
            .or_else(|| timing.offscreen_content_at())
        && let Some(report) = timing.finish(presented)
    {
        let benchmark = cfg!(feature = "acceptance")
            && std::env::var_os(render_api::STARTUP_BENCHMARK_ENV).is_some();
        if benchmark {
            use std::io::Write as _;
            let _ = writeln!(
                std::io::stderr().lock(),
                "{} {report}",
                render_scene::startup_timing::MARKER,
            );
        } else {
            bevy::log::info!("{} {report}", render_scene::startup_timing::MARKER);
        }
        #[cfg(feature = "acceptance")]
        if benchmark {
            exit.write(bevy::prelude::AppExit::Success);
        }
    }
    #[cfg(not(feature = "acceptance"))]
    let _ = &mut exit;
}

/// Logs the first frame's launch latency once.
fn log_first_frame(start: Res<EventLoopStart>, mut logged: Local<bool>) {
    if std::mem::replace(&mut *logged, true) {
        return;
    }
    let launched = crate::lifecycle::launched_at();
    bevy::log::info!(
        "first frame {:.1} ms after launch ({:.1} ms before the event loop)",
        launched.elapsed().as_secs_f64() * 1e3,
        start.0.duration_since(launched).as_secs_f64() * 1e3
    );
}

/// Wall time of each startup carrier load, logged as one line.
#[derive(Default)]
pub(crate) struct LoadTimes(std::sync::Mutex<Vec<(&'static str, std::time::Duration)>>);

impl LoadTimes {
    /// Times one load while preserving its result and panic behavior.
    pub(crate) fn time<T>(&self, carrier: &'static str, load: impl FnOnce() -> T) -> T {
        let start = std::time::Instant::now();
        if let Some(timing) = render_scene::startup_timing::current() {
            timing.count(&format!("carrier_{carrier}"), 1);
        }
        let value = load();
        if let Some(timing) = render_scene::startup_timing::current() {
            timing.record(&format!("carrier_{carrier}"), start.elapsed());
        }
        self.0
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
            .push((carrier, start.elapsed()));
        value
    }

    /// The carriers loaded so far, in completion order.
    #[cfg(test)]
    pub(crate) fn carriers(&self) -> Vec<&'static str> {
        let times = self
            .0
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        times.iter().map(|(carrier, _)| *carrier).collect()
    }

    /// Reports total wall time and each carrier duration in descending order.
    pub(crate) fn summary(&self, wall: std::time::Duration) -> String {
        let mut times = self
            .0
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
            .clone();
        times.sort_by_key(|time| std::cmp::Reverse(time.1));
        let each = times
            .iter()
            .map(|(carrier, time)| format!("{carrier} {:.1}", time.as_secs_f64() * 1e3))
            .collect::<Vec<_>>()
            .join(", ");
        format!(
            "loaded startup carriers in {:.1} ms (ms each: {each})",
            wall.as_secs_f64() * 1e3
        )
    }
}

impl CarrierLoadObserver for LoadTimes {
    /// Records elapsed time while retaining the carrier loader's return value.
    fn observe<T>(&self, carrier: &'static str, load: impl FnOnce() -> T) -> T {
        self.time(carrier, load)
    }
}

/// Joins a scoped load, re-raising its panic on this thread.
pub(crate) fn join<T>(handle: std::thread::ScopedJoinHandle<'_, T>) -> T {
    handle
        .join()
        .unwrap_or_else(|panic| std::panic::resume_unwind(panic))
}

/// Starts a main-thread update sample without the opt-in gameplay profiler.
fn begin_update(mut started: bevy::prelude::ResMut<UpdateStart>) {
    started.0 = Instant::now();
}

/// Totals main updates up to the first presented menu.
fn finish_update(started: Res<UpdateStart>) {
    if let Some(timing) = render_scene::startup_timing::current() {
        let elapsed = started.0.elapsed();
        timing.record("main_updates", elapsed);
        timing.main_update(elapsed);
        timing.count("main_updates", 1);
    }
}

/// Leaves ordinary gameplay and direct world launches free of startup-only sampling.
fn recording() -> bool {
    render_scene::startup_timing::current().is_some_and(|timing| timing.is_recording())
}

/// Emits the sealed result even after render completion has stopped startup work sampling.
fn report_pending() -> bool {
    render_scene::startup_timing::current().is_some_and(|timing| timing.needs_report())
}
