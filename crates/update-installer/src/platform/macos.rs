//! Copy complete signed bundles from a read-only mounted release DMG.
use anyhow::{Context, Result, ensure};
use std::{
    fs,
    path::{Path, PathBuf},
    process::Command,
};

/// Keeps both the bundle's code signature and its stapled notarization ticket intact.
pub(super) fn candidate(artifact: &Path, work: &Path, target: &Path) -> Result<PathBuf> {
    ensure!(
        target
            .extension()
            .is_some_and(|extension| extension == "app"),
        "target must be an app bundle"
    );
    let mount = work.join("mount");
    fs::create_dir(&mount)?;
    run(Command::new("/usr/bin/hdiutil")
        .args([
            "attach",
            "-readonly",
            "-nobrowse",
            "-noautoopen",
            "-mountpoint",
        ])
        .arg(&mount)
        .arg(artifact))?;
    let mounted = Mounted(mount);
    let apps = fs::read_dir(&mounted.0)?
        .filter_map(|entry| entry.ok())
        .map(|entry| entry.path())
        .filter(|path| {
            path.extension().is_some_and(|extension| extension == "app") && path.is_dir()
        })
        .collect::<Vec<_>>();
    ensure!(apps.len() == 1, "DMG must contain exactly one app bundle");
    let candidate = work.join("candidate.app");
    run(Command::new("/usr/bin/ditto").arg(&apps[0]).arg(&candidate))?;
    run(Command::new("/usr/bin/codesign")
        .args(["--verify", "--deep", "--strict"])
        .arg(&candidate))?;
    ensure!(
        candidate.join("Contents/MacOS/cinnabar").is_file(),
        "bundle is missing its entry point"
    );
    Ok(candidate)
}

/// Reports native tool failures without invoking a shell.
fn run(command: &mut Command) -> Result<()> {
    ensure!(
        command
            .status()
            .context("start macOS package tool")?
            .success(),
        "macOS package tool failed"
    );
    Ok(())
}

struct Mounted(PathBuf);
impl Drop for Mounted {
    /// Detaches even when validation or replacement failed.
    fn drop(&mut self) {
        let _ = Command::new("/usr/bin/hdiutil")
            .arg("detach")
            .arg(&self.0)
            .status();
    }
}
