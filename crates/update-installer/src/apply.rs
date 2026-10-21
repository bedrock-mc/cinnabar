//! One swap, one previous copy, and rollback when the newly launched client cannot start.

use crate::{platform, recovery, stage, target};
use anyhow::{Context, Result, ensure};
use std::{fs, path::Path};
use time::OffsetDateTime;
use update_manifest::{TrustedKeys, select::Check};

/// Local, signed inputs passed by the offline helper after the client chooses to update.
pub struct Request<'a> {
    pub stage_dir: &'a Path,
    pub target: &'a Path,
    pub keys: &'a TrustedKeys,
    pub check: Check<'a>,
    pub now: OffsetDateTime,
}

/// Verifies and applies an independently retained stage while owning the shared writer lease.
/// The caller must first observe the original client's exit.
pub fn after_exit(
    request: Request<'_>,
    lease: crate::lease::Lease,
    start: impl FnOnce(&Path) -> Result<()>,
) -> Result<()> {
    let target_for_recovery = request.target;
    let mut touched = false;
    let mut started = false;
    let result = (|| {
        let Request {
            stage_dir,
            target,
            keys,
            check,
            now,
        } = request;
        let root = stage_dir.parent().context("missing update directory")?;
        ensure!(
            root == lease.root(),
            "installation is outside the leased update directory"
        );
        // Authenticate even an obsolete queued stage before deciding to skip it.
        let selected = stage::verify(
            stage_dir,
            keys,
            Check {
                switch_now: true,
                ..check
            },
            now,
        )?;
        if !update_manifest::select::newer_build(
            &selected.manifest.version,
            selected.manifest.build_number,
            check.current,
            check.current_build,
        )? {
            start(target)?;
            started = true;
            return Ok(());
        }
        ensure!(
            target::access(target)?.permits_update(),
            "install is not writable; use Download update"
        );
        ensure!(
            !fs::symlink_metadata(target)?.file_type().is_symlink(),
            "unsafe install target"
        );
        let os = check
            .platform
            .split_once('-')
            .context("invalid platform key")?
            .0;
        ensure!(
            os == std::env::consts::OS,
            "helper cannot apply another platform"
        );
        let parent = if os == "windows" {
            root
        } else {
            target.parent().context("missing install parent")?
        };
        let work = tempfile::Builder::new()
            .prefix(".cinnabar-update-")
            .tempdir_in(parent)?;
        let artifact = work.path().join("payload");
        fs::copy(stage_dir.join(stage::ARTIFACT_FILE), &artifact)?;
        stage::verify_file(&artifact, &selected.artifact)?;
        if os == "windows" {
            touched = true;
            platform::install_windows(&artifact, work.path(), target, &selected.artifact)?;
            remember_installer(root, &artifact)?;
            start(target)?;
            started = true;
        } else {
            let backup = target::backup(target)?;
            let candidate = platform::candidate(&artifact, work.path(), target)?;
            swap(target, &candidate, &backup)?;
            touched = true;
            if let Err(error) = start(target) {
                rollback(target, &backup, &work.path().join("failed"))?;
                return Err(error).context("new version did not start; previous version restored");
            }
            started = true;
            recovery::confirm_start(&backup)?;
            recovery::cleanup_confirmed_backup(target, &backup)?;
        }
        // The handoff's reader lease and retention scan own snapshot cleanup.
        Ok(())
    })();
    match result {
        Err(error) if !touched => Err(error.context(Untouched)),
        Err(error) if !started => match recovery::restore_with_lease(target_for_recovery, &lease) {
            Ok(_) => Err(error),
            Err(recovery) => {
                Err(error.context(format!("recover changed installation: {recovery:#}")))
            }
        },
        result => result,
    }
}

/// A failed attempt made no changes to the installation and needs no rollback lease.
#[derive(Debug)]
pub struct Untouched;

impl std::fmt::Display for Untouched {
    /// Identifies pre-install failures without replacing their underlying error.
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter.write_str("installation was not changed")
    }
}

/// Keeps the most recently downloaded previous MSI for manual repair, without copying Program Files.
fn remember_installer(root: &Path, artifact: &Path) -> Result<()> {
    let current = root.join("installed.msi");
    let previous = root.join("previous.msi");
    if current.exists() {
        if previous.exists() {
            fs::remove_file(&previous)?;
        }
        fs::rename(&current, &previous)?;
    }
    fs::copy(artifact, current)?;
    Ok(())
}

/// Promotes a file or complete app bundle, restoring it if the promotion itself fails.
pub(crate) fn swap(target: &Path, candidate: &Path, backup: &Path) -> Result<()> {
    recovery::cleanup_confirmed_backup(target, backup)?;
    ensure!(!backup.exists(), "previous installation is still retained");
    #[cfg(any(target_os = "macos", target_os = "linux"))]
    {
        recovery::prepare_exchange(target, backup)?;
        if let Err(error) = fs::rename(candidate, backup) {
            recovery::clear_preparation(backup)?;
            return Err(error).context("prepare replacement beside installation");
        }
        if let Err(error) = recovery::exchange(target, backup) {
            fs::rename(backup, candidate).context("retain uninstalled replacement")?;
            recovery::clear_preparation(backup)?;
            return Err(error).context("install update");
        }
    }
    #[cfg(not(any(target_os = "macos", target_os = "linux")))]
    {
        fs::rename(target, backup).context("retain previous installation")?;
        if let Err(error) = fs::rename(candidate, target) {
            fs::rename(backup, target).context("restore previous installation")?;
            return Err(error).context("install update");
        }
    }
    Ok(())
}

/// Moves a failed new install aside before restoring the previous signed payload.
pub(crate) fn rollback(target: &Path, backup: &Path, failed: &Path) -> Result<()> {
    #[cfg(any(target_os = "macos", target_os = "linux"))]
    {
        recovery::exchange(target, backup).context("restore previous installation")?;
        fs::rename(backup, failed).context("move failed new installation aside")?;
        recovery::clear_preparation(backup)?;
    }
    #[cfg(not(any(target_os = "macos", target_os = "linux")))]
    {
        fs::rename(target, failed).context("move failed new installation aside")?;
        fs::rename(backup, target).context("restore previous installation")?;
    }
    Ok(())
}

#[cfg(test)]
mod tests;
