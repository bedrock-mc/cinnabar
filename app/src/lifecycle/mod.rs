//! Process lifecycle around the client: crash capture, first-run asset preparation, update checks.

use anyhow::Result;
#[cfg(not(target_arch = "wasm32"))]
use {
    anyhow::Context,
    launcher::install_layout::InstallLayout,
    launcher_host::lifecycle::{crash, update},
};

#[cfg(not(target_arch = "wasm32"))]
mod client_logs;

/// Runs pre-window duties for a packaged install; a no-op for development checkouts.
/// `assets_overridden` skips asset preparation when the caller supplied its own carrier path.
/// Returns `false` when the user quit first-time setup, so the client should exit quietly.
#[cfg(not(target_arch = "wasm32"))]
pub fn before_run(assets_overridden: bool) -> Result<bool> {
    let layout = InstallLayout::discover().context("resolve install layout")?;
    client_logs::capture(&layout);
    client_logs::install_panic_hook(&layout);
    crash::prune_reports(&layout);
    if !layout.is_installed() {
        return Ok(true);
    }

    if !assets_overridden && first_run::ensure_prepared(&layout)? == first_run::Outcome::Quit {
        return Ok(false);
    }
    update::check_in_background(&layout);
    if let Some(notice) = update::available(&layout) {
        diagnostics::log_stderr!(
            "Cinnabar {} is available (running {}).",
            notice.latest,
            notice.current
        );
    }
    Ok(true)
}

/// Browsers have no packaged install to prepare, crash-capture or update.
#[cfg(target_arch = "wasm32")]
pub fn before_run(_assets_overridden: bool) -> Result<bool> {
    Ok(true)
}
