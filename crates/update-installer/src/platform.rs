//! Native package preparation starts only after signed artifact verification.

#[cfg(target_os = "macos")]
mod macos;
#[cfg(windows)]
mod windows;

use anyhow::{Result, bail};
use std::{
    ffi::OsString,
    path::{Path, PathBuf},
};

/// Prepares a complete macOS bundle or an executable AppImage on the install filesystem.
pub(crate) fn candidate(artifact: &Path, work: &Path, target: &Path) -> Result<PathBuf> {
    #[cfg(target_os = "macos")]
    {
        macos::candidate(artifact, work, target)
    }
    #[cfg(target_os = "linux")]
    {
        use std::{fs, os::unix::fs::PermissionsExt};
        let _ = (work, target);
        fs::set_permissions(artifact, fs::Permissions::from_mode(0o755))?;
        Ok(artifact.to_owned())
    }
    #[cfg(not(any(target_os = "macos", target_os = "linux")))]
    {
        let _ = (artifact, work, target);
        bail!("this platform uses its installer")
    }
}

/// The raw msiexec command line for a passive upgrade into the existing `target`, which may be a
/// custom `INSTALLFOLDER`. msiexec parses its own command line, so values are quoted as
/// `NAME="value"`; Windows paths never contain quotes.
#[cfg_attr(not(windows), allow(dead_code))]
pub(crate) fn msiexec_arguments(msi: &Path, target: &Path) -> Vec<OsString> {
    vec![
        "/i".into(),
        quoted("", msi),
        "/passive".into(),
        "/norestart".into(),
        "REBOOT=ReallySuppress".into(),
        // The helper owns the single client restart after installation completes.
        "MSIDISABLERMRESTART=1".into(),
        quoted("INSTALLFOLDER=", target),
    ]
}

/// `prefix"path"`, where no backslash may escape the closing quote: trailing separators are
/// dropped, and a bare drive or root keeps its backslash doubled, which parses as one.
#[cfg_attr(not(windows), allow(dead_code))]
fn quoted(prefix: &str, path: &Path) -> OsString {
    let bytes = path.as_os_str().as_encoded_bytes();
    let end = bytes
        .iter()
        .rposition(|&byte| !matches!(byte, b'\\' | b'/'))
        .map_or(0, |last| last + 1);
    // SAFETY: only trailing ASCII separators are cut, which leaves valid encoded bytes.
    let value = unsafe { std::ffi::OsStr::from_encoded_bytes_unchecked(&bytes[..end]) };
    let mut argument = OsString::from(format!("{prefix}\""));
    argument.push(value);
    if end == 0 || bytes[end - 1] == b':' {
        argument.push(r"\\");
    }
    argument.push("\"");
    argument
}

/// Runs Windows Installer passively with system-managed permission consent and no forced reboot.
pub(crate) fn install_windows(
    artifact: &Path,
    work: &Path,
    target: &Path,
    expected: &update_manifest::Artifact,
) -> Result<()> {
    #[cfg(windows)]
    {
        windows::install(artifact, work, target, expected)
    }
    #[cfg(not(windows))]
    {
        let _ = (artifact, work, target, expected);
        bail!("Windows installer cannot run on this platform")
    }
}

#[cfg(test)]
mod tests;
