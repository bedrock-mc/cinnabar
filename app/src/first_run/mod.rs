//! First-run preparation of the Mojang-derived asset carriers, which installers never ship.
//!
//! Runs before the game window: consent, a download of the pinned public pack, then `assetc`. A
//! setup window in a child process shows it; without one, native dialogs do. Progress is mirrored
//! to `logs/first-run-status.json`.

mod download;
mod plan;
mod prepare;
mod runner;
mod screen;
mod stamp;
mod status;
#[cfg(test)]
mod test_support;
mod window;

use std::{fs, path::PathBuf, sync::atomic::AtomicBool};

use anyhow::{Context, Result, bail};
use sha2::{Digest, Sha256};

use crate::{
    install_layout::InstallLayout,
    native_dialog::{NativePrompter, Prompter},
};
use prepare::prepare;
use status::{Phase, Status};
pub(crate) use window::{SETUP_FLAG, run_setup_process};

const CONSENT_ENV: &str = "CINNABAR_ACCEPT_MOJANG_EULA";
const TITLE: &str = "Cinnabar first-time setup";
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
        && !prepare::selection(layout).is_ok_and(|(_, selection)| selection.is_current())
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

/// The native-dialog flow, used when no setup window can open.
fn ensure_with(
    layout: &InstallLayout,
    prompter: &dyn Prompter,
    env_consent: bool,
) -> Result<Outcome> {
    if !needs_preparation(layout) {
        return Ok(Outcome::NotNeeded);
    }
    let mut report = reporter(layout, |_| {});
    report(Status::new(
        Phase::AwaitingConsent,
        0,
        0,
        "Waiting for consent",
    ));
    if !env_consent && !consent_recorded(layout) && !prompter.confirm(TITLE, CONSENT_BODY) {
        report(Status::failed("Declined", "setup declined"));
        return Ok(Outcome::Quit);
    }
    let is_update = updating(layout);
    record_consent(layout)?;
    prompter.info(
        TITLE,
        if is_update {
            "Updating game assets for this version of Cinnabar. This takes a few minutes."
        } else {
            "Preparing game assets. This happens once and takes a few minutes."
        },
    );
    match prepare(layout, &AtomicBool::new(false), &mut report) {
        Ok(()) => {
            prompter.info(TITLE, "Setup finished. Starting Cinnabar.");
            Ok(Outcome::Prepared)
        }
        Err(error) => {
            let message = format!("{error:#}");
            prompter.alert(
                TITLE,
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
mod tests {
    use std::{cell::Cell, path::PathBuf};

    use super::*;
    use crate::install_layout::{InstallEnvironment, Platform};
    use test_support::Dir;

    struct Fake {
        accept: bool,
        asked: Cell<u32>,
    }

    impl Prompter for Fake {
        fn confirm(&self, _: &str, _: &str) -> bool {
            self.asked.set(self.asked.get() + 1);
            self.accept
        }
        fn info(&self, _: &str, _: &str) {}
        fn alert(&self, _: &str, _: &str) {}
    }

    pub(super) fn installed_layout(data: &Dir, executable: &str) -> InstallLayout {
        let mut layout = InstallLayout::resolve(
            Platform::Linux,
            &InstallEnvironment {
                executable: PathBuf::from(executable),
                home: Some(PathBuf::from("/home/dev")),
                local_app_data: None,
                xdg_config_home: Some(data.path().join("cfg")),
                xdg_data_home: Some(data.path().join("data")),
                xdg_runtime_dir: None,
            },
        )
        .unwrap();
        // Linux layout fixtures also run on Windows, where native scratch paths are not XDG
        // absolute paths. Keep their filesystem writes out of the shared Linux home fallback.
        layout.user_config_root = data.path().join("cfg/cinnabar");
        layout.user_data_root = data.path().join("data/cinnabar");
        layout.with_prepared_assets()
    }

    #[test]
    fn development_layout_needs_no_preparation() {
        let data = Dir::new("dev");
        let layout = installed_layout(&data, "/work/cinnabar/target/release/bedrock-client");
        let fake = Fake {
            accept: false,
            asked: Cell::new(0),
        };
        assert_eq!(
            ensure_with(&layout, &fake, false).unwrap(),
            Outcome::NotNeeded
        );
        assert_eq!(fake.asked.get(), 0);
    }

    #[test]
    fn declined_consent_quits_without_running_anything() {
        let data = Dir::new("decline");
        let layout = installed_layout(&data, "/nonexistent/opt/cinnabar/bin/bedrock-client");
        let fake = Fake {
            accept: false,
            asked: Cell::new(0),
        };
        assert_eq!(ensure_with(&layout, &fake, false).unwrap(), Outcome::Quit);
        assert!(!consent_marker(&layout).exists());
    }

    #[test]
    fn recorded_consent_to_the_same_terms_is_not_asked_again() {
        let data = Dir::new("consent");
        let layout = installed_layout(&data, "/nonexistent/opt/cinnabar/bin/bedrock-client");
        let fake = Fake {
            accept: false,
            asked: Cell::new(0),
        };
        record_consent(&layout).unwrap();
        let _ = ensure_with(&layout, &fake, false);
        assert_eq!(fake.asked.get(), 0);
        // Consent recorded for other terms (or by an older build) asks again.
        fs::write(consent_marker(&layout), b"accepted\n").unwrap();
        assert_eq!(ensure_with(&layout, &fake, false).unwrap(), Outcome::Quit);
        assert_eq!(fake.asked.get(), 1);
    }

    #[test]
    fn missing_kit_is_reported_after_consent_and_recorded() {
        let data = Dir::new("nokit");
        let layout = installed_layout(&data, "/nonexistent/opt/cinnabar/bin/bedrock-client");
        let fake = Fake {
            accept: true,
            asked: Cell::new(0),
        };
        let error = ensure_with(&layout, &fake, false).unwrap_err();
        assert!(format!("{error:#}").contains("preparation kit"));
        let status = fs::read_to_string(layout.log_dir().join("first-run-status.json")).unwrap();
        assert!(status.contains("\"failed\"") && status.contains("preparation kit"));
        // Consent is remembered, so the retry does not prompt again.
        let _ = ensure_with(&layout, &fake, false);
        assert_eq!(fake.asked.get(), 1);
    }
}
