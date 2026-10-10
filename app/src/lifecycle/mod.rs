//! Process lifecycle around the client: crash capture, first-run asset preparation, update checks.

use anyhow::{Context, Result};
use launcher_host::lifecycle::{core_health, crash, update};

use launcher::install_layout::InstallLayout;

/// Argument that turns this process into the first-run setup window.
pub const FIRST_RUN_SETUP_FLAG: &str = first_run::SETUP_FLAG;

/// Runs the first-run setup window; returns the process exit code.
#[must_use]
pub fn run_first_run_setup() -> i32 {
    first_run::run_setup_process()
}

/// Runs pre-window duties for a packaged install; a no-op for development checkouts.
/// `assets_overridden` skips asset preparation when the caller supplied its own carrier path.
/// Returns `false` when the user quit first-time setup, so the client should exit quietly.
pub fn before_run(assets_overridden: bool) -> Result<bool> {
    let layout = InstallLayout::discover().context("resolve install layout")?;
    if !layout.is_installed() {
        return Ok(true);
    }
    core_health::capture_client_stderr(&layout);
    crash::install_panic_hook(&layout);
    crash::prune_reports(&layout);
    if !assets_overridden && first_run::ensure_prepared(&layout)? == first_run::Outcome::Quit {
        return Ok(false);
    }
    update::check_in_background(&layout);
    if let Some(notice) = update::available(&layout) {
        eprintln!(
            "Cinnabar {} is available (running {}).",
            notice.latest, notice.current
        );
    }
    Ok(true)
}
