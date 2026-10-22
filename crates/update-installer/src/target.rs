//! Supported installed targets; unpacked developer builds are never replaced.
use anyhow::{Context, Result, bail, ensure};
use std::{
    fs,
    path::{Path, PathBuf},
    process::Stdio,
};

/// Entry point supplied by the Linux release's AppDir.
pub const APP_RUN: &str = "AppRun";

/// The packaged client inside an extracted Linux AppDir.
pub fn linux_executable(appdir: &Path) -> PathBuf {
    appdir
        .join("usr/bin")
        .join(update_manifest::runtime::executable("CLIENT", "linux"))
}

/// Queries the installed executable itself, without loading assets or using the copied helper's version.
pub fn installed_build(target: &Path) -> Result<update_manifest::build::Build> {
    let output = crate::startup::client_command(target)
        .arg(update_manifest::build::QUERY_FLAG)
        .stdout(Stdio::piped())
        .stderr(Stdio::null())
        .output()
        .context("query installed build")?;
    ensure!(output.status.success(), "installed build query failed");
    serde_json::from_slice(&output.stdout).context("invalid installed build identity")
}

/// Resolves the replaceable bundle, MSI directory, AppImage source or extracted AppDir.
pub fn resolve(os: &str, executable: &Path, appimage: Option<&Path>) -> Result<PathBuf> {
    ensure!(
        executable.is_absolute(),
        "installed executable must be absolute"
    );
    match os {
        "macos" => executable
            .ancestors()
            .nth(3)
            .filter(|path| path.extension().is_some_and(|extension| extension == "app"))
            .filter(|_| {
                executable
                    .parent()
                    .is_some_and(|p| p.ends_with("Contents/MacOS"))
            })
            .map(Path::to_owned)
            .context("executable is not in an app bundle"),
        "windows" => executable
            .parent()
            .map(Path::to_owned)
            .context("missing MSI install directory"),
        "linux" if appimage.is_some() => appimage
            .filter(|path| path.is_absolute())
            .map(Path::to_owned)
            .context("automatic updates require an AppImage install"),
        "linux" => executable
            .ancestors()
            .nth(3)
            .filter(|appdir| linux_executable(appdir) == executable)
            .filter(|appdir| appdir.join(APP_RUN).is_file() && executable.is_file())
            .map(Path::to_owned)
            .context("automatic updates require an AppImage or extracted AppDir install"),
        _ => bail!("unsupported update platform"),
    }
}

/// Sibling used by the native install transaction and retained until startup is confirmed.
pub fn backup(target: &Path) -> Result<PathBuf> {
    let mut name = target
        .file_name()
        .context("missing install name")?
        .to_os_string();
    name.push(".previous");
    Ok(target.with_file_name(name))
}

/// How the helper can replace this installation after shutdown.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum Access {
    Writable,
    /// Windows Installer owns writes and any required administrator authorization.
    Installer,
    Manual,
}

impl Access {
    /// Whether the update can use the helper's exit barrier and managed relaunch.
    pub fn permits_update(self) -> bool {
        matches!(self, Self::Writable | Self::Installer)
    }
}

/// Uses Windows Installer for Windows; probes direct replacement rights on other platforms.
pub fn access(target: &Path) -> Result<Access> {
    ensure!(
        target.is_absolute() && target.exists(),
        "installed target is missing"
    );
    let access = access_with(target, std::env::consts::OS, |directory| {
        tempfile::NamedTempFile::new_in(directory)
            .and_then(|file| file.close())
            .is_ok()
    })?;
    #[cfg(any(target_os = "macos", target_os = "linux"))]
    if access == Access::Writable
        && !crate::recovery::supports_exchange(target.parent().context("missing install parent")?)
    {
        return Ok(Access::Manual);
    }
    Ok(access)
}

/// Keeps permission decisions testable without chmod, elevation, or a platform-specific account.
fn access_with(target: &Path, os: &str, mut probe: impl FnMut(&Path) -> bool) -> Result<Access> {
    if os == "windows" {
        ensure!(
            target
                .join(update_manifest::runtime::executable("CLIENT", os))
                .is_file(),
            "target is not an installed client"
        );
        return Ok(Access::Installer);
    }
    let directory = target.parent().context("missing install parent")?;
    if !probe(directory) {
        return Ok(Access::Manual);
    }
    let previous = backup(target)?;
    if !removable_tree(target, &mut probe)
        || (previous.exists() && !removable_tree(&previous, &mut probe))
    {
        return Ok(Access::Manual);
    }
    Ok(Access::Writable)
}

/// Checks every directory needed to delete a bundle, without following its symlinks.
fn removable_tree(target: &Path, probe: &mut impl FnMut(&Path) -> bool) -> bool {
    let mut pending = vec![target.to_owned()];
    while let Some(path) = pending.pop() {
        let Ok(metadata) = fs::symlink_metadata(&path) else {
            return false;
        };
        if !metadata.is_dir() {
            continue;
        }
        if !probe(&path) {
            return false;
        }
        let Ok(entries) = fs::read_dir(&path) else {
            return false;
        };
        for entry in entries {
            let Ok(entry) = entry else {
                return false;
            };
            let Ok(kind) = entry.file_type() else {
                return false;
            };
            if kind.is_dir() {
                pending.push(entry.path());
            }
        }
    }
    true
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn an_extracted_linux_release_is_a_replaceable_appdir() {
        let root = tempfile::tempdir().unwrap();
        let appdir = root.path().join("installed-build");
        let executable = linux_executable(&appdir);
        fs::create_dir_all(executable.parent().unwrap()).unwrap();
        fs::write(&executable, "client").unwrap();
        assert!(resolve("linux", &executable, None).is_err());
        fs::write(appdir.join(APP_RUN), "launcher").unwrap();
        assert_eq!(resolve("linux", &executable, None).unwrap(), appdir);
        assert!(resolve("linux", &root.path().join("target/debug/cinnabar"), None).is_err());
    }

    #[test]
    fn windows_target_is_the_whole_install_directory() {
        let root = tempfile::tempdir().unwrap();
        let executable = root
            .path()
            .join(update_manifest::runtime::executable("CLIENT", "windows"));
        assert_eq!(resolve("windows", &executable, None).unwrap(), root.path());
    }

    #[cfg(unix)]
    #[test]
    fn resolves_only_native_install_shapes() {
        assert_eq!(
            resolve(
                "macos",
                Path::new("/Applications/Client.app/Contents/MacOS/cinnabar"),
                None
            )
            .unwrap(),
            Path::new("/Applications/Client.app")
        );
        assert!(resolve("macos", Path::new("/tmp/Client.app/wrong/bin/client"), None).is_err());
        assert!(resolve("linux", Path::new("/opt/cinnabar/bin/client"), None).is_err());
        assert!(
            resolve(
                "linux",
                Path::new("/opt/cinnabar/bin/client"),
                Some(Path::new("relative.AppImage"))
            )
            .is_err()
        );
        assert_eq!(
            resolve(
                "linux",
                Path::new("/tmp/mount/usr/bin/client"),
                Some(Path::new("/opt/Client.AppImage"))
            )
            .unwrap(),
            Path::new("/opt/Client.AppImage")
        );
    }
    #[test]
    fn not_writable_installs_offer_the_manual_download() {
        let root = tempfile::tempdir().unwrap();
        let target = root.path().join("client");
        fs::write(&target, "installed").unwrap();
        assert_eq!(
            access_with(&target, "linux", |_| false).unwrap(),
            Access::Manual
        );
        assert_eq!(
            access_with(&target, "linux", |_| true).unwrap(),
            Access::Writable
        );
    }

    #[test]
    fn windows_updates_use_the_exit_barrier_even_without_direct_write_rights() {
        let root = tempfile::tempdir().unwrap();
        fs::write(
            root.path()
                .join(update_manifest::runtime::executable("CLIENT", "windows")),
            "installed client",
        )
        .unwrap();
        let access =
            access_with(root.path(), "windows", |_| panic!("MSI owns file writes")).unwrap();
        assert_eq!(access, Access::Installer);
        assert!(access.permits_update());
        assert!(!Access::Manual.permits_update());
    }

    #[cfg(not(windows))]
    #[test]
    fn writable_parent_with_a_non_writable_bundle_directory_requires_manual_download() {
        let root = tempfile::tempdir().unwrap();
        let target = root.path().join("Client.app");
        let internal = target.join("Contents/MacOS");
        fs::create_dir_all(&internal).unwrap();
        fs::write(internal.join("client"), "installed").unwrap();
        let mut checked_internal = false;
        let access = access_with(&target, "macos", |directory| {
            if directory == internal.as_path() {
                checked_internal = true;
                false
            } else {
                true
            }
        })
        .unwrap();
        assert!(checked_internal);
        assert_eq!(access, Access::Manual);
        assert_eq!(
            access_with(&target, "macos", |_| true).unwrap(),
            Access::Writable
        );
    }

    #[cfg(not(windows))]
    #[test]
    fn a_retained_bundle_that_cannot_be_removed_requires_manual_download() {
        let root = tempfile::tempdir().unwrap();
        let target = root.path().join("Client.app");
        fs::create_dir_all(target.join("Contents/MacOS")).unwrap();
        let internal = backup(&target).unwrap().join("Contents/MacOS");
        fs::create_dir_all(&internal).unwrap();
        assert_eq!(
            access_with(&target, "macos", |directory| directory
                != internal.as_path())
            .unwrap(),
            Access::Manual
        );
    }

    #[cfg(unix)]
    #[test]
    fn bundle_cleanup_does_not_probe_symlinked_external_directories() {
        let root = tempfile::tempdir().unwrap();
        let target = root.path().join("Client.app");
        let external = root.path().join("external");
        fs::create_dir(&target).unwrap();
        fs::create_dir(&external).unwrap();
        std::os::unix::fs::symlink(&external, target.join("linked")).unwrap();
        assert_eq!(
            access_with(&target, "macos", |directory| directory
                != external.as_path())
            .unwrap(),
            Access::Writable
        );
    }
}
