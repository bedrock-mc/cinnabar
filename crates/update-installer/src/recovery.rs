//! Startup receipts and recovery of installations whose helper was interrupted.

use anyhow::{Context, Result, ensure};
use std::{
    fs::{self, File},
    path::{Path, PathBuf},
};

/// Helper mode used only after a client fails before its first good frames.
pub const RECOVER_FLAG: &str = "--recover";

/// Whether an orphaned, unconfirmed swap can be rolled back after this process exits.
pub fn needs_rollback(target: &Path, update_dir: &Path) -> bool {
    let probe = || -> Result<bool> {
        let backup = crate::target::backup(target)?;
        if !backup.exists() || confirmation(&backup).exists() {
            return Ok(false);
        }
        #[cfg(unix)]
        if target.exists() && before_exchange(target, &backup)? {
            return Ok(false);
        }
        Ok(crate::lease::Lease::try_acquire(update_dir).is_ok())
    };
    probe().unwrap_or(false)
}

/// Restores an orphaned backup after the failed client exits, unless startup was already confirmed.
pub fn restore(target: &Path, update_dir: &Path) -> Result<bool> {
    let lease = crate::lease::Lease::try_acquire(update_dir)
        .context("another updater is recovering this installation")?;
    restore_with_lease(target, &lease)
}

/// Restores this attempt's interrupted swap before releasing the installation's writer lease.
pub(crate) fn restore_with_lease(target: &Path, _lease: &crate::lease::Lease) -> Result<bool> {
    let backup = crate::target::backup(target)?;
    if !backup.exists() || confirmation(&backup).exists() {
        return Ok(false);
    }
    ensure!(
        !fs::symlink_metadata(&backup)?.file_type().is_symlink(),
        "unsafe previous installation"
    );
    #[cfg(unix)]
    if target.exists() && before_exchange(target, &backup)? {
        remove(&backup)?;
        clear_preparation(&backup)?;
        return Ok(false);
    }
    let parent = target.parent().context("missing install parent")?;
    let work = tempfile::Builder::new()
        .prefix(".cinnabar-rollback-")
        .tempdir_in(parent)?;
    if target.exists() {
        crate::apply::rollback(target, &backup, &work.path().join("failed"))?;
    } else {
        fs::rename(&backup, target).context("restore missing installation")?;
    }
    clear_preparation(&backup)?;
    Ok(true)
}

/// Journal beside the backup, distinguishing a prepared candidate from an exchanged predecessor.
fn preparation(backup: &Path) -> PathBuf {
    let mut name = backup.as_os_str().to_os_string();
    name.push(".preparing");
    PathBuf::from(name)
}

/// Object identity stays attached to an installation through renames and atomic exchanges.
#[cfg(unix)]
fn identity(metadata: &fs::Metadata) -> (u64, u64) {
    use std::os::unix::fs::MetadataExt;
    (metadata.dev(), metadata.ino())
}

/// Records the current installation before moving its candidate into the backup location.
#[cfg(any(target_os = "macos", target_os = "linux"))]
pub(crate) fn prepare_exchange(target: &Path, backup: &Path) -> Result<()> {
    use std::io::Write;
    let (device, inode) = identity(&fs::symlink_metadata(target)?);
    let mut file = File::options()
        .write(true)
        .create_new(true)
        .open(preparation(backup))?;
    write!(file, "{device}:{inode}")?;
    file.sync_all()?;
    Ok(())
}

/// Whether a recorded exchange has not happened, or a rollback has already restored its target.
#[cfg(unix)]
fn before_exchange(target: &Path, backup: &Path) -> Result<bool> {
    let record = match fs::read_to_string(preparation(backup)) {
        Ok(record) => record,
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => return Ok(false),
        Err(error) => return Err(error.into()),
    };
    let (device, inode) = record.split_once(':').context("invalid exchange journal")?;
    let expected = (device.parse::<u64>()?, inode.parse::<u64>()?);
    Ok(identity(&fs::symlink_metadata(target)?) == expected)
}

/// Removes a completed or abandoned exchange journal; absence is already clean.
pub(crate) fn clear_preparation(backup: &Path) -> Result<()> {
    match fs::remove_file(preparation(backup)) {
        Ok(()) => Ok(()),
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => Ok(()),
        Err(error) => Err(error.into()),
    }
}

/// The receipt beside a backup, written only after its replacement confirms startup.
pub(crate) fn confirmation(backup: &Path) -> PathBuf {
    let mut name = backup.as_os_str().to_os_string();
    name.push(".started");
    PathBuf::from(name)
}

/// Persists successful startup before cleanup; repeated acknowledgements are harmless.
pub(crate) fn confirm_start(backup: &Path) -> Result<()> {
    let receipt = confirmation(backup);
    match File::options().write(true).create_new(true).open(&receipt) {
        Ok(file) => file.sync_all()?,
        Err(error) if error.kind() == std::io::ErrorKind::AlreadyExists => {
            ensure!(
                fs::symlink_metadata(receipt)?.is_file(),
                "unsafe startup receipt"
            );
        }
        Err(error) => return Err(error.into()),
    }
    Ok(())
}

/// Retries only confirmed cleanup; unconfirmed backups remain available for rollback.
pub(crate) fn cleanup_confirmed_backup(target: &Path, backup: &Path) -> Result<()> {
    let receipt = confirmation(backup);
    if !receipt.is_file() {
        if !backup.exists() {
            clear_preparation(backup)?;
        }
        return Ok(());
    }
    ensure!(
        target.exists(),
        "confirmed replacement is missing; keep previous installation"
    );
    if backup.exists() {
        remove(backup).context("remove confirmed previous installation")?;
    }
    clear_preparation(backup)?;
    fs::remove_file(receipt).context("remove startup confirmation")?;
    Ok(())
}

/// Deletes a known previous file or complete bundle without following a symlink.
pub(crate) fn remove(path: &Path) -> Result<()> {
    if fs::symlink_metadata(path)?.is_dir() {
        fs::remove_dir_all(path)?;
    } else {
        fs::remove_file(path)?;
    }
    Ok(())
}

/// Atomically exchanges a bundle or AppImage, leaving its installed path present at every step.
#[cfg(any(target_os = "macos", target_os = "linux"))]
pub(crate) fn exchange(left: &Path, right: &Path) -> Result<()> {
    use rustix::fs::{CWD, RenameFlags, renameat_with};
    renameat_with(CWD, left, CWD, right, RenameFlags::EXCHANGE)?;
    Ok(())
}

/// Checks whether this filesystem supports the atomic directory swaps used by native updates.
#[cfg(any(target_os = "macos", target_os = "linux"))]
pub(crate) fn supports_exchange(directory: &Path) -> bool {
    let probe = || -> Result<()> {
        let left = tempfile::tempdir_in(directory)?;
        let right = tempfile::tempdir_in(directory)?;
        exchange(left.path(), right.path())
    };
    probe().is_ok()
}

/// The installation observed before this process starts rendering frames.
#[cfg(unix)]
pub struct Startup {
    target: PathBuf,
    metadata: fs::Metadata,
}

#[cfg(unix)]
impl Startup {
    /// Captures the installed file or bundle identity before any helper can replace it.
    pub fn observe(target: PathBuf) -> Result<Self> {
        let metadata = fs::symlink_metadata(&target)?;
        ensure!(!metadata.file_type().is_symlink(), "unsafe install target");
        Ok(Self { target, metadata })
    }

    /// Confirms this installation after good frames, including starts without a live helper.
    pub fn confirm(&self, update_dir: &Path) -> Result<()> {
        fs::create_dir_all(update_dir)?;
        // A live helper consumes its own acknowledgement before releasing this lease.
        let _lease = crate::lease::Lease::acquire(update_dir)?;
        let current = fs::symlink_metadata(&self.target)?;
        if identity(&current) != identity(&self.metadata) {
            return Ok(());
        }
        let backup = crate::target::backup(&self.target)?;
        if backup.exists() {
            confirm_start(&backup)?;
        }
        cleanup_confirmed_backup(&self.target, &backup)
    }
}

#[cfg(all(test, unix))]
mod tests;
