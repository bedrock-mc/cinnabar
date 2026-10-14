//! Process lifecycle around the client: crash capture, first-run asset preparation, update checks.

use anyhow::{Context, Result};
use launcher_host::lifecycle::{crash, update};

mod client_logs;

use launcher::install_layout::InstallLayout;
use std::{sync::OnceLock, time::Instant};

static LAUNCHED_AT: OnceLock<Instant> = OnceLock::new();

/// When client startup began; the first call records it.
pub(crate) fn launched_at() -> Instant {
    *LAUNCHED_AT.get_or_init(Instant::now)
}

/// The `--first-run-setup` child process: logs as the client does, through the inherited
/// stderr the client captures, then runs the setup window and returns its exit code.
pub fn run_first_run_setup() -> i32 {
    crate::app::logging::install(crate::app::logging::Process::Setup);
    let code = first_run::run_setup_process();
    diagnostics::console::flush_before_exit();
    code
}

/// Runs pre-window duties for a packaged install; a no-op for development checkouts.
/// `assets_overridden` skips asset preparation when the caller supplied its own carrier path.
/// Returns `false` when the user quit first-time setup, so the client should exit quietly.
pub fn before_run(assets_overridden: bool) -> Result<bool> {
    crate::app::logging::install(crate::app::logging::Process::Client);
    launched_at();
    let layout = InstallLayout::discover().context("resolve install layout")?;
    client_logs::capture(&layout);
    client_logs::install_panic_hook(&layout);
    crash::prune_reports(&layout);
    if !layout.is_installed() {
        return Ok(true);
    }
    update::check_in_background(&layout);

    // An update helper waiting on this start must not mistake a long asset download for a hang.
    if !assets_overridden
        && update::while_preparing(
            &layout,
            &first_run::progress_file(&layout),
            first_run::awaiting_user,
            || first_run::ensure_prepared(&layout),
        )? == first_run::Outcome::Quit
    {
        return Ok(false);
    }
    Ok(true)
}

/// Acknowledges startup once the renderer confirms several completed primary-window frames.
pub fn acknowledge_startup(
    presented: bevy::prelude::Res<render_scene::startup_presentation::StartupPresentation>,
    mut sent: bevy::prelude::Local<bool>,
) {
    acknowledge_if_presented(&presented, &mut sent, update::acknowledge_startup);
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
