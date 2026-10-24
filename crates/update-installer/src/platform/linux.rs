//! Prepares Linux releases in the same form as the installed target.

use anyhow::{Context, Result, ensure};
use std::{
    fs,
    os::unix::fs::PermissionsExt,
    path::{Path, PathBuf},
    process::{Command, Stdio},
};

/// Extracts a verified AppImage beside an AppDir, leaving file installs as executable images.
pub(super) fn candidate(artifact: &Path, work: &Path, target: &Path) -> Result<PathBuf> {
    fs::set_permissions(artifact, fs::Permissions::from_mode(0o755))?;
    if !target.is_dir() {
        return Ok(artifact.to_owned());
    }
    let mut command = Command::new(artifact);
    command
        .arg("--appimage-extract")
        .current_dir(work)
        .stdin(Stdio::null())
        .stdout(Stdio::null());
    for name in crate::startup::APPIMAGE_RUNTIME_ENV {
        command.env_remove(name);
    }
    ensure!(
        command
            .status()
            .context("extract verified AppImage")?
            .success(),
        "AppImage extraction failed"
    );
    let candidate = work.join("squashfs-root");
    for executable in [
        candidate.join(crate::target::APP_RUN),
        crate::target::linux_executable(&candidate),
    ] {
        let metadata = fs::metadata(&executable).context("AppDir is missing its entry point")?;
        ensure!(
            metadata.is_file() && metadata.permissions().mode() & 0o111 != 0,
            "AppDir entry point is not executable"
        );
    }
    Ok(candidate)
}
