//! Consent and ordered preparation for setup without a window or native dialogs.

use std::{
    fs::{self, OpenOptions},
    io::Write,
    sync::atomic::{AtomicBool, Ordering},
};

use anyhow::{Result, anyhow};
use launcher::install_layout::InstallLayout;
use launcher_host::local_worlds::WorldError;

use crate::{
    CONSENT_BODY, Outcome, consent_recorded, prepare, record_consent, reporter, runner,
    setup_title,
    status::{Phase, Status},
};

const LOCAL_WORLDS_LABEL: &str = "Preparing local worlds";
const LOCAL_WORLDS_FAILED: &str = "Could not prepare local worlds. Please retry setup.";

/// Independent setup requirements, so server preparation never forces a carrier rebuild.
#[derive(Clone, Copy, Default)]
pub(super) struct Pending {
    assets: bool,
    local_worlds: bool,
}

impl Pending {
    /// Checks the install's carriers and, on Android, its local-world runtime.
    pub(super) fn for_install(layout: &InstallLayout) -> Self {
        if !layout.is_installed() {
            return Self::default();
        }
        Self {
            assets: !prepare::selection(layout).is_ok_and(|selection| selection.is_current()),
            local_worlds: local_worlds_needed(layout),
        }
    }

    /// Whether setup has any work to perform.
    pub(super) fn any(self) -> bool {
        self.assets || self.local_worlds
    }
}

/// Desktop setup never downloads a server, even when the host can run local worlds.
fn local_worlds_needed(layout: &InstallLayout) -> bool {
    #[cfg(target_os = "android")]
    {
        launcher_host::local_worlds::android_server_needs_preparation(layout)
    }
    #[cfg(not(target_os = "android"))]
    {
        let _ = layout;
        false
    }
}

/// Setup operations whose ordering and cancellation can be exercised without downloads.
trait Preparer {
    /// Reports only missing or stale stages.
    fn pending(&self, layout: &InstallLayout) -> Pending;

    /// Publishes carriers without reporting overall setup completion.
    fn assets(
        &mut self,
        layout: &InstallLayout,
        cancel: &AtomicBool,
        report: &mut dyn FnMut(Status),
    ) -> Result<()>;

    /// Prepares local worlds after the caller has resolved Minecraft EULA consent.
    fn local_worlds(
        &mut self,
        layout: &InstallLayout,
        cancel: &AtomicBool,
        progress: &mut dyn FnMut(&str),
    ) -> Result<(), WorldError>;
}

struct Installed;

impl Preparer for Installed {
    fn pending(&self, layout: &InstallLayout) -> Pending {
        Pending::for_install(layout)
    }

    fn assets(
        &mut self,
        layout: &InstallLayout,
        cancel: &AtomicBool,
        report: &mut dyn FnMut(Status),
    ) -> Result<()> {
        prepare::prepare_assets(layout, cancel, report)
    }

    fn local_worlds(
        &mut self,
        layout: &InstallLayout,
        cancel: &AtomicBool,
        progress: &mut dyn FnMut(&str),
    ) -> Result<(), WorldError> {
        #[cfg(target_os = "android")]
        {
            launcher_host::local_worlds::prepare_android_server(layout, cancel, progress)
        }
        #[cfg(not(target_os = "android"))]
        {
            let _ = (layout, cancel, progress);
            Ok(())
        }
    }
}

/// Prepares without a window or dialogs, as Android's setup activity does. `consent` asks with a
/// title and body only when none is recorded, and `progress` receives each status line. Android
/// prepares local worlds after assets; only completion of both allows the game to start.
/// Setting `cancel` quits setup without reporting readiness.
pub fn prepare_headless(
    layout: &InstallLayout,
    cancel: &AtomicBool,
    consent: impl FnOnce(&str, &str) -> bool,
    progress: impl FnMut(&str),
) -> Result<Outcome> {
    prepare_with(layout, cancel, consent, progress, &mut Installed)
}

/// Runs the same setup flow with explicit preparation operations.
fn prepare_with(
    layout: &InstallLayout,
    cancel: &AtomicBool,
    consent: impl FnOnce(&str, &str) -> bool,
    mut progress: impl FnMut(&str),
    preparer: &mut impl Preparer,
) -> Result<Outcome> {
    if cancel.load(Ordering::Relaxed) {
        return Ok(Outcome::Quit);
    }
    let pending = preparer.pending(layout);
    if cancel.load(Ordering::Relaxed) {
        return Ok(Outcome::Quit);
    }
    if !pending.any() {
        return Ok(Outcome::NotNeeded);
    }
    let mut report = reporter(layout, |status: &Status| progress(&status.summary()));
    let needs_consent = !consent_recorded(layout);
    if needs_consent {
        report(Status::awaiting_consent());
        if !consent(&setup_title(None), CONSENT_BODY) {
            report(Status::failed("Declined", "setup declined"));
            return Ok(Outcome::Quit);
        }
    }
    let result = (|| {
        ensure_active(cancel)?;
        if needs_consent {
            record_consent(layout)?;
        }
        report(Status::starting());
        ensure_active(cancel)?;
        if pending.assets {
            preparer.assets(layout, cancel, &mut report)?;
            ensure_active(cancel)?;
        }
        if pending.local_worlds {
            report(Status::new(Phase::Running, 1, 1, LOCAL_WORLDS_LABEL));
            ensure_active(cancel)?;
            let mut previous = LOCAL_WORLDS_LABEL.to_owned();
            match preparer.local_worlds(layout, cancel, &mut |line| {
                if line != previous {
                    report(Status::new(Phase::Running, 1, 1, line));
                    previous = line.to_owned();
                }
            }) {
                Ok(()) => {}
                Err(WorldError::Cancelled) => return Err(runner::Cancelled.into()),
                Err(error) => {
                    ensure_active(cancel)?;
                    log_local_worlds_failure(layout, &error);
                    return Err(anyhow!(LOCAL_WORLDS_FAILED));
                }
            }
        }
        ensure_active(cancel)
    })();
    match result {
        Ok(()) => {
            report(Status::ready());
            Ok(Outcome::Prepared)
        }
        Err(error) if error.is::<runner::Cancelled>() => Ok(Outcome::Quit),
        Err(error) => {
            report(Status::failed("Failed", &format!("{error:#}")));
            Err(error)
        }
    }
}

/// Stops between stages and before readiness, including when a stage finishes after cancellation.
fn ensure_active(cancel: &AtomicBool) -> Result<()> {
    if cancel.load(Ordering::Relaxed) {
        return Err(runner::Cancelled.into());
    }
    Ok(())
}

/// Keeps detailed server failures in the setup log while the activity receives a setup message.
fn log_local_worlds_failure(layout: &InstallLayout, error: &WorldError) {
    if fs::create_dir_all(layout.log_dir()).is_err() {
        return;
    }
    let path = prepare::log_path(layout);
    if let Ok(mut log) = OpenOptions::new().create(true).append(true).open(path) {
        let _ = writeln!(log, "local-world preparation failed: {error}");
    }
}

#[cfg(test)]
mod tests;
