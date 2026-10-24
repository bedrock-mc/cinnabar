//! Process lifecycle around the client: crash capture, first-run asset preparation, update checks.

use anyhow::{Context, Result};
use launcher_host::lifecycle::{crash, update};

mod client_logs;
pub(crate) mod heartbeat;

use launcher::install_layout::InstallLayout;
use launcher_host::lifecycle::crash::protocol::Stage;
use std::{
    sync::{
        Once, OnceLock,
        atomic::{AtomicBool, Ordering},
    },
    time::Instant,
};

static LAUNCHED_AT: OnceLock<Instant> = OnceLock::new();
/// Set once startup is acknowledged: a failure from then on happened while the game ran.
static STARTED: AtomicBool = AtomicBool::new(false);

/// When client startup began; the first call records it.
pub(crate) fn launched_at() -> Instant {
    *LAUNCHED_AT.get_or_init(Instant::now)
}

/// Anchors launch timing before argument parsing and logging setup.
pub fn record_launch() {
    render_scene::startup_timing::StartupTiming::start(launched_at());
}

/// The `--first-run-setup` child process: logs as the client does, through the inherited
/// stderr the client captures, then runs the setup window and returns its exit code.
pub fn run_first_run_setup() -> i32 {
    crate::app::logging::install(crate::app::logging::Process::Setup);
    let code = first_run::run_setup_process();
    diagnostics::console::flush_before_exit();
    code
}

/// The `--crash-monitor` child process: logs to its own `crash-monitor.log`, then watches the
/// client until it exits and reports a crash; returns its exit code.
pub fn run_crash_monitor() -> i32 {
    crate::app::logging::install(crate::app::logging::Process::Monitor);
    let args: Vec<std::ffi::OsString> = std::env::args_os().skip(2).collect();
    let Some(args) = crash_monitor::Args::parse(&args) else {
        diagnostics::log_stderr!(
            "usage: cinnabar {} <socket> <parent pid> <crash dir>",
            crash::MONITOR_FLAG
        );
        return 2;
    };
    let layout = InstallLayout::discover().ok();
    let code = match crash_monitor::run(&args, layout.as_ref()) {
        Ok(_) => 0,
        Err(error) => {
            bevy::log::error!("{error}");
            1
        }
    };
    diagnostics::console::flush_before_exit();
    code
}

/// Runs pre-window duties for a packaged install; a no-op for development checkouts.
/// `assets_overridden` skips asset preparation when the caller supplied its own carrier path.
/// Returns `false` when the user quit first-time setup, so the client should exit quietly.
pub fn before_run(assets_overridden: bool) -> Result<bool> {
    record_launch();
    let layout = start_session()?;
    if !layout.is_installed() {
        return Ok(true);
    }
    update::check_in_background(&layout);

    if !assets_overridden {
        // An update helper waiting on this start must not mistake a long asset download for a
        // hang.
        let outcome = update::while_preparing(
            &layout,
            &first_run::progress_file(&layout),
            first_run::awaiting_user,
            || first_run::ensure_prepared(&layout),
        )?;
        match outcome {
            first_run::Outcome::Quit => return Ok(false),
            // Preparation rewrote carriers that a preload may already hold.
            first_run::Outcome::Prepared => crate::app::startup::discard_preloaded(),
            first_run::Outcome::NotNeeded => {}
        }
    }
    Ok(true)
}

/// Starts the client session and loads the default startup carriers on the calling thread for
/// the next [`crate::run`]. Android's setup activity calls this before it starts the game
/// activity, whose UI thread waits on the game thread until the event loop starts; Android
/// reports the app as not responding when that wait reaches 5 s. Failures are left for
/// [`before_run`] and [`crate::run`] to report. A retried setup reuses an earlier preload unless
/// `rewritten` says setup has rewritten the carriers since.
pub fn preload(rewritten: bool) {
    record_launch();
    let Ok(layout) = start_session() else {
        return;
    };
    crate::thread_budget::ThreadBudget::configure_global_rayon();
    crate::app::startup::preload(&layout, rewritten);
}

/// Starts client logging, crash capture and this session's log files on the first call in the
/// process, then resolves the install layout on every call.
fn start_session() -> Result<InstallLayout> {
    static LOGGING: Once = Once::new();
    static SESSION_FILES: Once = Once::new();
    LOGGING.call_once(|| crate::app::logging::install(crate::app::logging::Process::Client));
    let layout = InstallLayout::discover().context("resolve install layout")?;
    SESSION_FILES.call_once(|| {
        crate::app::logging::apply_saved_verbose_setting(&layout);
        crash::prune_reports(&layout);
        // The monitor starts first, so it can report even a failure of the rest of startup.
        let monitor = if cfg!(target_os = "android") {
            None
        } else {
            crash::monitor::MonitorCommand::current_exe().ok()
        };
        client_logs::install_crash_capture(&layout, monitor.as_ref());
        client_logs::capture(&layout);
        client_logs::capture_core(&layout);
        crash::logs_ready();
    });
    Ok(layout)
}

/// Shows why `main` failed when this build has no console for it: the crash monitor shows it
/// in the crash window once this process exits, else a native alert does. Before startup is
/// acknowledged that says the game couldn't start, afterwards that it stopped.
pub fn show_failure(error: &anyhow::Error) {
    if !launcher_host::desktop::gui_subsystem() {
        return;
    }
    let stage = if STARTED.load(Ordering::Acquire) {
        Stage::Runtime
    } else {
        Stage::Startup
    };
    let text = format!("{error:#}");
    if !crash::report_failure(stage, &text) {
        use launcher_host::native_dialog::{NativePrompter, Prompter};
        NativePrompter.alert(&crash_monitor::window::failure_title(stage), &text);
    }
}

/// Acknowledges startup once the renderer confirms several completed primary-window frames.
pub fn acknowledge_startup(
    presented: bevy::prelude::Res<render_scene::startup_presentation::StartupPresentation>,
    mut sent: bevy::prelude::Local<bool>,
) {
    acknowledge_if_presented(&presented, &mut sent, || {
        STARTED.store(true, Ordering::Release);
        update::acknowledge_startup();
    });
}

/// Main-world progress cannot confirm startup without render-completion feedback.
fn acknowledge_if_presented(
    presented: &render_scene::startup_presentation::StartupPresentation,
    sent: &mut bool,
    acknowledge: impl FnOnce(),
) {
    if !*sent && presented.is_complete() {
        *sent = true;
        acknowledge();
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use bevy::prelude::*;
    use render_scene::startup_presentation::StartupPresentation;

    #[derive(Default, Resource)]
    struct Acknowledgements(u8);

    /// Records the lifecycle gate's side effect without touching a real install or global state.
    fn probe(
        presented: Res<StartupPresentation>,
        mut sent: Local<bool>,
        mut acknowledgements: ResMut<Acknowledgements>,
    ) {
        acknowledge_if_presented(&presented, &mut sent, || acknowledgements.0 += 1);
    }

    #[test]
    fn main_updates_cannot_acknowledge_startup_until_presentations_complete() {
        let mut app = App::new();
        app.init_resource::<StartupPresentation>()
            .init_resource::<Acknowledgements>()
            .add_systems(First, probe);
        for _ in 0..10 {
            app.update();
        }
        assert_eq!(app.world().resource::<Acknowledgements>().0, 0);
        let renderer = app.world().resource::<StartupPresentation>().clone();
        for _ in 1..StartupPresentation::REQUIRED_FRAMES {
            renderer.record_completed_frame();
            for _ in 0..10 {
                app.update();
            }
            assert_eq!(app.world().resource::<Acknowledgements>().0, 0);
        }
        renderer.record_completed_frame();
        app.update();
        assert_eq!(app.world().resource::<Acknowledgements>().0, 1);
        for _ in 0..10 {
            app.update();
        }
        assert_eq!(app.world().resource::<Acknowledgements>().0, 1);
    }
}
