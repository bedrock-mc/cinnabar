//! Background staging and menu state. Installation is only armed after the client has shut down.

mod apply;
mod storage;
#[cfg(test)]
mod tests;
mod worker;

use crate::install_layout::InstallLayout;
use launcher::menu::UpdateView;
use serde::{Deserialize, Serialize};
use std::{
    path::PathBuf,
    sync::{Arc, Mutex, MutexGuard, OnceLock},
};

const DISABLE_ENV: &str = "CINNABAR_UPDATE_CHECK";
const CHANNEL: &str = "stable";
static UPDATER: OnceLock<Updater> = OnceLock::new();

struct Updater {
    layout: InstallLayout,
    state: Mutex<State>,
}

#[derive(Default)]
struct State {
    enabled: bool,
    generation: u64,
    running: bool,
    child: Option<Arc<super::children::Spawned>>,
    status: Status,
    restart: bool,
}

#[derive(Default)]
enum Status {
    #[default]
    Idle,
    Checking,
    Downloading {
        downloaded: u64,
        total: u64,
    },
    Ready(Ready),
    Error(String),
}

#[derive(Clone, Debug, Deserialize, Serialize)]
struct Ready {
    current: String,
    latest: String,
    stage: PathBuf,
    #[serde(default)]
    notes_url: String,
}

impl State {
    /// Builds the menu snapshot without touching disk or blocking on the helper.
    fn view(&self) -> UpdateView {
        let mut view = UpdateView {
            enabled: self.enabled,
            ..Default::default()
        };
        match &self.status {
            Status::Idle => {}
            Status::Checking => view.message = "Checking for updates…".into(),
            Status::Downloading { downloaded, total } => {
                let percent = downloaded
                    .saturating_mul(100)
                    .checked_div(*total)
                    .unwrap_or(0)
                    .min(100);
                view.message = format!("Downloading update… {percent}%");
            }
            Status::Ready(ready) if self.enabled => {
                view.message = format!(
                    "Cinnabar {} is ready. Installs when you exit.",
                    ready.latest
                );
                view.ready = true;
                view.notes = notes_url(&ready.notes_url).is_some();
            }
            Status::Ready(_) => {}
            Status::Error(error) => {
                let summary: String = error.chars().take(100).collect();
                view.message = format!("Update: {summary}");
                view.retry = self.enabled;
            }
        }
        view
    }

    /// Only a ready update in an idle launcher may request a restart.
    fn request_restart(&mut self, idle_launcher: bool) -> bool {
        self.restart = idle_launcher && self.enabled && matches!(self.status, Status::Ready(_));
        self.restart
    }

    /// Invalidates worker replies and cancels an active download when checks are disabled.
    fn disable(&mut self) {
        self.enabled = false;
        self.generation = self.generation.wrapping_add(1);
        self.running = false;
        self.restart = false;
        if let Some(child) = self.child.take() {
            child.kill();
        }
        self.status = Status::Idle;
    }
}

impl Updater {
    /// Recovers the state after a poisoned lock so updater failures cannot crash the game.
    fn state(&self) -> MutexGuard<'_, State> {
        self.state
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
    }
}

/// Initializes installed-client update state and starts a due check off the main thread.
pub(crate) fn check_in_background(layout: &InstallLayout) {
    if !layout.is_installed() {
        return;
    }
    let enabled = storage::enabled(layout) && !environment_disabled();
    let status = if enabled {
        storage::restore(layout)
    } else {
        Status::Idle
    };
    let updater = UPDATER.get_or_init(|| Updater {
        layout: layout.clone(),
        state: Mutex::new(State {
            enabled,
            status,
            ..Default::default()
        }),
    });
    worker::start(updater, false);
}

/// Returns the latest menu state, including the saved opt-out.
pub(crate) fn view() -> UpdateView {
    UPDATER
        .get()
        .map_or_else(UpdateView::default, |updater| updater.state().view())
}

/// Requests another verified check after a reported error.
pub(crate) fn retry() {
    if let Some(updater) = UPDATER.get() {
        worker::start(updater, true);
    }
}

/// Persists the opt-out; an environment opt-out always wins over the setting.
pub(crate) fn toggle() {
    let Some(updater) = UPDATER.get() else {
        return;
    };
    let mut state = updater.state();
    if environment_disabled() {
        state.status = Status::Error(format!(
            "Automatic updates are disabled by {DISABLE_ENV}=0."
        ));
        return;
    }
    let enabled = !state.enabled;
    if let Err(error) = storage::save_enabled(&updater.layout, enabled) {
        state.status = Status::Error(format!("Could not save preference: {error}"));
        return;
    }
    if !enabled {
        state.disable();
    } else {
        state.enabled = true;
    }
    drop(state);
    if enabled {
        worker::start(updater, true);
    }
}

/// Arms a restart request without launching an installer or stopping a live session.
pub(crate) fn request_restart(idle_launcher: bool) -> bool {
    UPDATER
        .get()
        .is_some_and(|updater| updater.state().request_restart(idle_launcher))
}

/// Opens verified release notes using a direct process argument, never a command shell.
pub(crate) fn open_notes() {
    let Some(updater) = UPDATER.get() else {
        return;
    };
    let state = updater.state();
    let Status::Ready(ready) = &state.status else {
        return;
    };
    let Some(url) = notes_url(&ready.notes_url) else {
        return;
    };
    let opener = if cfg!(target_os = "macos") {
        "open"
    } else if cfg!(windows) {
        "explorer.exe"
    } else {
        "xdg-open"
    };
    let _ = std::process::Command::new(opener)
        .arg(url.as_str())
        .stdin(std::process::Stdio::null())
        .stdout(std::process::Stdio::null())
        .stderr(std::process::Stdio::null())
        .spawn();
}

/// Hands off only after a successful run has dropped the world and stopped its children.
pub(crate) fn after_run() {
    if let Some(updater) = UPDATER.get()
        && let Err(error) = apply::handoff(updater)
    {
        eprintln!("Update could not be installed: {error:#}");
        storage::record_error(&updater.layout, &format!("{error:#}"));
    }
}

/// Accepts only an ordinary HTTPS release-notes destination.
fn notes_url(raw: &str) -> Option<url::Url> {
    url::Url::parse(raw).ok().filter(|url| {
        url.scheme() == "https"
            && url.host_str().is_some()
            && url.username().is_empty()
            && url.password().is_none()
    })
}

/// Checks the process-wide override shared by startup and the Settings control.
fn environment_disabled() -> bool {
    std::env::var(DISABLE_ENV).is_ok_and(|value| value == "0")
}

/// Maps Rust platform names to the signed release manifest's platform keys.
fn platform_key(os: &str, arch: &str) -> Option<String> {
    let arch = match arch {
        "aarch64" => "arm64",
        "x86_64" => "x86_64",
        _ => return None,
    };
    matches!(os, "macos" | "windows" | "linux").then(|| format!("{os}-{arch}"))
}
