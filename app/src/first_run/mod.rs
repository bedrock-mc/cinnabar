//! First-run preparation of the Mojang-derived asset carriers, which installers never ship.
//!
//! Runs before the game window: consent, a download of the pinned public pack, then one
//! `assetc prepare`. A setup window in a child process shows it; without one, native dialogs do.
//! Progress is mirrored to `logs/first-run-status.json`.

mod download;
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

use crate::{
    install_layout::InstallLayout,
    native_dialog::{Consent, NativePrompter, Prompter},
};
use prepare::prepare;
use status::{Phase, Status};
pub(crate) use window::{SETUP_FLAG, run_setup_process};

const CONSENT_ENV: &str = "CINNABAR_ACCEPT_MOJANG_EULA";
const CONSENT_BODY: &str = "Cinnabar needs Minecraft's official sample resource pack. It is downloaded from Mojang's public release (a large one-time download), converted on this computer, and never redistributed by Cinnabar.\n\nContinuing confirms you accept the Minecraft EULA (https://www.minecraft.net/eula). Setup runs once and takes a few minutes.";
const EULA_URL: &str = "https://www.minecraft.net/eula";

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) enum Outcome {
    NotNeeded,
    Prepared,
    /// The user declined or cancelled; the client should exit quietly.
    Quit,
}

/// Prepares the per-user carriers when a packaged install has none or they predate the pins this
/// build carries; a no-op for development checkouts.
pub(crate) fn ensure_prepared(layout: &InstallLayout) -> Result<Outcome> {
    if !needs_preparation(layout) {
        return Ok(Outcome::NotNeeded);
    }
    match window::run_in_child() {
        window::ChildOutcome::Prepared => Ok(Outcome::Prepared),
        window::ChildOutcome::Quit => Ok(Outcome::Quit),
        window::ChildOutcome::Failed(code) => bail!(
            "first-time setup failed (exit {code}); details: {}",
            layout.log_dir().join("first-run.log").display()
        ),
        window::ChildOutcome::Unavailable => ensure_with(layout, &NativePrompter, env_consent()),
    }
}

/// An unreadable kit counts as needing preparation, which then reports it.
fn needs_preparation(layout: &InstallLayout) -> bool {
    layout.is_installed()
        && !prepare::selection(layout).is_ok_and(|selection| selection.is_current())
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

/// Mirrors each report to the status file, then hands it to `forward`.
fn reporter<'a>(
    layout: &InstallLayout,
    mut forward: impl FnMut(&Status) + 'a,
) -> impl FnMut(Status) + 'a {
    let path = layout.log_dir().join("first-run-status.json");
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
    let title = setup_title(std::env::var("CINNABAR_WINDOW_TITLE").ok().as_deref());
    let mut report = reporter(layout, |_| {});
    report(Status::new(
        Phase::AwaitingConsent,
        0,
        0,
        "Waiting for consent",
    ));
    if !env_consent && !consent_recorded(layout) {
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
                    layout.log_dir().join("first-run.log").display()
                ),
            );
            Err(error)
        }
    }
}

#[cfg(test)]
mod tests;
