//! First-run preparation of the game resources, which installers never ship.
//!
//! Runs before the game window: consent, a download of the pinned public pack, then one
//! `assetc prepare`. Android also tries to prepare local worlds before starting the game. A setup
//! window in a child process shows desktop setup; without one, native dialogs do. Progress is
//! mirrored to `logs/first-run-status.json`.

mod download;
mod fs_retry;
mod headless;
mod prepare;
mod runner;
mod screen;
mod status;
#[cfg(test)]
mod test_support;
mod window;

use std::{fs, path::PathBuf, sync::atomic::AtomicBool};

use anyhow::{Context, Result, bail};
use sha2::{Digest, Sha256};

pub use headless::prepare_headless;
use prepare::prepare;
use status::Status;
pub use window::{SETUP_FLAG, run_setup_process};
use {
    launcher::install_layout::InstallLayout,
    launcher_host::native_dialog::{Consent, NativePrompter, Prompter},
};

const CONSENT_ENV: &str = "CINNABAR_ACCEPT_MOJANG_EULA";
#[cfg(not(target_os = "android"))]
const CONSENT_BODY: &str = "Cinnabar needs Minecraft's official sample resource pack. It is downloaded from Mojang's public release (a large one-time download), converted on this computer, and never redistributed by Cinnabar.\n\nContinuing confirms you accept the Minecraft EULA (https://www.minecraft.net/eula). Setup runs once and takes a few minutes.";
#[cfg(target_os = "android")]
const CONSENT_BODY: &str = "Cinnabar needs Minecraft's official sample resource pack and dedicated server. They are downloaded from Mojang (a large one-time download) and prepared on this device for game assets and local worlds. Cinnabar never redistributes these downloads. Local worlds are prepared during setup and may need preparation again after an app update.\n\nContinuing confirms you accept the Minecraft EULA (https://www.minecraft.net/eula). Setup takes a few minutes.";
const EULA_URL: &str = "https://www.minecraft.net/eula";

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum Outcome {
    NotNeeded,
    Prepared,
    /// The user declined or cancelled; the client should exit quietly.
    Quit,
}

/// Prepares a packaged install's resources when they are missing or stale; a no-op for development
/// checkouts. Android's setup activity also tries to prepare local worlds before starting the game.
pub fn ensure_prepared(layout: &InstallLayout) -> Result<Outcome> {
    if !needs_preparation(layout) {
        return Ok(Outcome::NotNeeded);
    }
    if cfg!(target_os = "android") {
        // Android's setup activity runs `prepare_headless` before the game activity starts.
        bail!("setup is incomplete; close and reopen the app to finish setup");
    }
    // An earlier run's report must not describe this one.
    let _ = fs::remove_file(progress_file(layout));
    match window::run_in_child() {
        window::ChildOutcome::Prepared => Ok(Outcome::Prepared),
        window::ChildOutcome::Quit => Ok(Outcome::Quit),
        window::ChildOutcome::Failed(code) => bail!(
            "first-time setup failed (exit {code}); details: {}",
            prepare::log_path(layout).display()
        ),
        window::ChildOutcome::Unavailable => ensure_with(layout, &NativePrompter, env_consent()),
    }
}

/// Whether required assets need setup. Local worlds are optional, so an unprepared server
/// never blocks remote play. An unreadable kit is reported during setup.
fn needs_preparation(layout: &InstallLayout) -> bool {
    headless::assets_needed(layout)
}

/// An earlier set exists, so this run updates rather than sets up.
fn updating(layout: &InstallLayout) -> bool {
    layout.prepared_assets_dir().is_dir()
}

fn env_consent() -> bool {
    std::env::var_os(CONSENT_ENV).is_some_and(|v| v == "1")
}

fn consent_marker(layout: &InstallLayout) -> PathBuf {
    layout.prepare_workspace().join("eula-accepted")
}

/// What the user agreed to; new consent text or a new EULA link asks again.
fn consent_identity() -> String {
    format!(
        "{:x}\n",
        Sha256::digest(format!("{CONSENT_BODY}\n{EULA_URL}"))
    )
}

fn consent_recorded(layout: &InstallLayout) -> bool {
    fs::read_to_string(consent_marker(layout)).is_ok_and(|marker| marker == consent_identity())
}

fn record_consent(layout: &InstallLayout) -> Result<()> {
    let marker = consent_marker(layout);
    if let Some(parent) = marker.parent() {
        fs::create_dir_all(parent)?;
    }
    fs::write(&marker, consent_identity()).context("record consent")
}

/// The status file setup rewrites on every progress report, from either setup process.
pub fn progress_file(layout: &InstallLayout) -> PathBuf {
    layout.log_dir().join("first-run-status.json")
}

/// Whether `report`, read from [`progress_file`], says setup is waiting on the user.
pub fn awaiting_user(report: &[u8]) -> bool {
    status::awaiting_user(report)
}

/// Mirrors each report to the status file, then hands it to `forward`.
fn reporter<'a>(
    layout: &InstallLayout,
    mut forward: impl FnMut(&Status) + 'a,
) -> impl FnMut(Status) + 'a {
    let path = progress_file(layout);
    move |status| {
        let _ = status::write(&path, &status);
        forward(&status);
    }
}

fn setup_title(override_title: Option<&str>) -> String {
    format!(
        "{} first-time setup",
        launcher::window_title(override_title)
    )
}

/// The native-dialog flow, used when no setup window can open.
fn ensure_with(
    layout: &InstallLayout,
    prompter: &dyn Prompter,
    env_consent: bool,
) -> Result<Outcome> {
    if !needs_preparation(layout) {
        return Ok(Outcome::NotNeeded);
    }
    let title = setup_title(
        std::env::var(bootstrap_ui::window::TITLE_ENV)
            .ok()
            .as_deref(),
    );
    let mut report = reporter(layout, |_| {});
    if !env_consent && !consent_recorded(layout) {
        report(Status::awaiting_consent());
        match prompter.confirm(&title, CONSENT_BODY) {
            Consent::Accepted => {}
            Consent::Declined => {
                report(Status::failed("Declined", "setup declined"));
                return Ok(Outcome::Quit);
            }
            Consent::Unavailable => {
                let message = format!(
                    "Setup needs your consent, but no setup window, dialog or terminal could \
                     ask for it. Install zenity or kdialog, or start Cinnabar with \
                     {CONSENT_ENV}=1 to accept the Minecraft EULA ({EULA_URL})."
                );
                report(Status::failed("No consent prompt", &message));
                prompter.alert(&title, &message);
                bail!("{message}");
            }
        }
    }
    report(Status::starting());
    let is_update = updating(layout);
    record_consent(layout)?;
    prompter.info(
        &title,
        if is_update {
            "Updating game assets for this version of Cinnabar. This takes a few minutes."
        } else {
            "Preparing game assets. This happens once and takes a few minutes."
        },
    );
    match prepare(layout, &AtomicBool::new(false), &mut report) {
        Ok(()) => {
            prompter.info(&title, "Setup finished. Starting Cinnabar.");
            Ok(Outcome::Prepared)
        }
        Err(error) => {
            let message = format!("{error:#}");
            prompter.alert(
                &title,
                &format!(
                    "Setup failed: {message}\n\nDetails: {}",
                    prepare::log_path(layout).display()
                ),
            );
            Err(error)
        }
    }
}

#[cfg(test)]
mod tests;
