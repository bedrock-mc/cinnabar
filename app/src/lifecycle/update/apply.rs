//! Post-shutdown handoff to a copied helper that can outlive application replacement.

use super::{CHANNEL, Status, Updater, platform_key, storage};
use anyhow::{Context, Result, bail};
use std::{
    fs,
    path::{Path, PathBuf},
    process::{Command, Stdio},
};

/// Arms a detached helper only after the client run loop and child cleanup have returned.
pub(super) fn handoff(updater: &Updater) -> Result<()> {
    let state = updater.state();
    if !state.enabled {
        return Ok(());
    }
    let Status::Ready(ready) = &state.status else {
        return Ok(());
    };
    let platform = platform_key(std::env::consts::OS, std::env::consts::ARCH)
        .context("unsupported update platform")?;
    let executable = std::env::current_exe().context("locate installed application")?;
    let target = install_target(
        std::env::consts::OS,
        &executable,
        std::env::var_os("APPIMAGE").map(PathBuf::from),
    )?;
    let root = storage::directory(&updater.layout);
    let helper_root = root.join(format!("helper-{}", std::process::id()));
    fs::create_dir_all(&helper_root).context("create updater helper directory")?;
    let helper = helper_root.join(
        updater
            .layout
            .core_executable
            .file_name()
            .context("missing core filename")?,
    );
    fs::copy(&updater.layout.core_executable, &helper)
        .context("copy updater outside installation")?;
    let log = fs::File::create(root.join("apply-log.txt")).context("create installer log")?;
    let mut command = Command::new(&helper);
    command
        .args(["apply-update", "--stage"])
        .arg(&ready.stage)
        .args(["--channel", CHANNEL, "--platform", &platform, "--target"])
        .arg(target)
        .arg("--parent-pid")
        .arg(std::process::id().to_string())
        .current_dir(&helper_root)
        .env_remove("BEDROCK_CORE_PARENT_PID")
        .stdin(Stdio::piped())
        .stdout(Stdio::null())
        .stderr(log);
    if state.restart {
        command.arg("--restart");
    }
    #[cfg(target_os = "linux")]
    for name in ["APPIMAGE", "APPDIR", "OWD", "LD_LIBRARY_PATH", "LD_PRELOAD"] {
        command.env_remove(name);
    }
    #[cfg(windows)]
    {
        use std::os::windows::process::CommandExt;
        // Detach from a closing console and avoid flashing a helper console.
        command.creation_flags(0x00000008 | 0x08000000);
    }
    // This child intentionally bypasses the ordinary core registry. EOF plus parent death
    // are both required by the helper before it touches the installed application.
    let mut child = command.spawn().context("start updater helper")?;
    drop(child.stdin.take());
    Ok(())
}

/// Resolves only installation formats for which an atomic or transactional upgrade exists.
fn install_target(os: &str, executable: &Path, appimage: Option<PathBuf>) -> Result<PathBuf> {
    match os {
        "macos" => executable
            .ancestors()
            .nth(3)
            .filter(|path| path.extension().is_some_and(|extension| extension == "app"))
            .map(Path::to_owned)
            .context("application is not inside a macOS bundle"),
        "windows" => Ok(executable.to_owned()),
        "linux" => appimage
            .filter(|path| path.is_absolute() && path.is_file())
            .context("Automatic replacement requires an AppImage installation"),
        _ => bail!("unsupported update platform"),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn targets_real_bundle_and_rejects_unpacked_linux() {
        assert_eq!(
            install_target(
                "macos",
                Path::new("/Applications/Cinnabar.app/Contents/MacOS/bedrock-client"),
                None
            )
            .unwrap(),
            Path::new("/Applications/Cinnabar.app")
        );
        assert!(install_target("macos", Path::new("/tmp/bedrock-client"), None).is_err());
        assert!(
            install_target("linux", Path::new("/opt/cinnabar/bin/bedrock-client"), None).is_err()
        );
    }
}
