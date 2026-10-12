//! Supported installed targets; unpacked developer builds are never replaced.
use anyhow::{Context, Result, bail, ensure};
use std::{
    fs,
    path::{Path, PathBuf},
    process::Stdio,
};

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

/// Resolves the replaceable bundle, MSI directory, or mounted AppImage source.
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
        "linux" => appimage
            .filter(|path| path.is_absolute())
            .map(Path::to_owned)
            .context("automatic updates require an AppImage install"),
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

/// Whether the helper can write this install without changing user privileges.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum Access {
    Writable,
    Manual,
}

/// Probes the install filesystem; permission failures become the manual installer action.
pub fn access(target: &Path) -> Result<Access> {
    ensure!(
        target.is_absolute() && target.exists(),
        "installed target is missing"
    );
    #[cfg(windows)]
    if !machine_install_rights() {
        return Ok(Access::Manual);
    }
    let access = access_with(target, |directory| {
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

#[cfg(windows)]
/// Requires existing machine-install rights for the per-machine MSI, without requesting them.
fn machine_install_rights() -> bool {
    use windows_sys::Win32::{
        Foundation::CloseHandle,
        Security::{GetTokenInformation, TOKEN_ELEVATION, TOKEN_QUERY, TokenElevation},
        System::Threading::{GetCurrentProcess, OpenProcessToken},
    };
    let mut token = std::ptr::null_mut();
    let mut elevation = TOKEN_ELEVATION::default();
    let mut length = 0;
    // SAFETY: valid output buffers; the opened token is checked and closed exactly once.
    unsafe {
        if OpenProcessToken(GetCurrentProcess(), TOKEN_QUERY, &mut token) == 0 {
            return false;
        }
        let read = GetTokenInformation(
            token,
            TokenElevation,
            std::ptr::from_mut(&mut elevation).cast(),
            std::mem::size_of::<TOKEN_ELEVATION>() as u32,
            &mut length,
        );
        CloseHandle(token);
        read != 0 && elevation.TokenIsElevated != 0
    }
}

/// Keeps permission decisions testable without chmod, elevation, or a platform-specific account.
fn access_with(target: &Path, mut probe: impl FnMut(&Path) -> bool) -> Result<Access> {
    let directory = if cfg!(windows) {
        target
    } else {
        target.parent().context("missing install parent")?
    };
    if !probe(directory) {
        return Ok(Access::Manual);
    }
    if !cfg!(windows) {
        let previous = backup(target)?;
        if !removable_tree(target, &mut probe)
            || (previous.exists() && !removable_tree(&previous, &mut probe))
        {
            return Ok(Access::Manual);
        }
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
    fn windows_target_is_the_whole_install_directory() {
        let root = tempfile::tempdir().unwrap();
        let executable = root.path().join("cinnabar.exe");
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
        assert_eq!(access_with(&target, |_| false).unwrap(), Access::Manual);
        assert_eq!(access_with(&target, |_| true).unwrap(), Access::Writable);
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
        let access = access_with(&target, |directory| {
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
        assert_eq!(access_with(&target, |_| true).unwrap(), Access::Writable);
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
            access_with(&target, |directory| directory != internal.as_path()).unwrap(),
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
            access_with(&target, |directory| directory != external.as_path()).unwrap(),
            Access::Writable
        );
    }
}
