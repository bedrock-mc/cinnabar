//! Copies one offline helper or opens a verified package for a manual install.
use super::*;
use anyhow::{Context, Result, ensure};
use std::{
    ffi::OsString,
    fs::{self, File},
    path::{Path, PathBuf},
    process::{Command, Stdio},
    time::SystemTime,
};
use time::OffsetDateTime;
use update_manifest::select::Check;

/// Resolves only native app bundles, MSI layouts and mounted AppImage source files.
pub(super) fn installed_target() -> Result<PathBuf> {
    update_installer::target::resolve(
        std::env::consts::OS,
        &std::env::current_exe()?,
        std::env::var_os(update_installer::startup::APPIMAGE)
            .map(PathBuf::from)
            .as_deref(),
    )
}

/// Reopens the retained client if the helper cannot be copied or started, keeping the apply error.
pub(super) fn spawn_or_restart(
    spawn: impl FnOnce() -> Result<()>,
    restart: impl FnOnce() -> Result<()>,
) -> Result<()> {
    match spawn() {
        Ok(()) => Ok(()),
        Err(error) => match restart() {
            Ok(()) => Err(error),
            Err(restart) => Err(error.context(format!("restart retained client: {restart:#}"))),
        },
    }
}

/// Starts the copied helper with the exact parent PID and retained channel stage.
pub(super) fn spawn(layout: &InstallLayout, ready: &Ready) -> Result<()> {
    if let Some(lease) = &ready.lease {
        lease.set_modified(SystemTime::now())?;
    }
    let args = [
        ready.stage.as_os_str().to_owned(),
        installed_target()?.into_os_string(),
        std::process::id().to_string().into(),
        ready.channel.name().into(),
    ];
    spawn_with_args(layout, &args)?;
    if let Some(lease) = &ready.lease {
        // Keep the parent reader through actual process exit, just like the helper copy reader.
        std::mem::forget(lease.clone());
    }
    Ok(())
}

/// Starts rollback recovery with the same exact-parent exit barrier as an ordinary apply.
#[cfg(unix)]
pub(super) fn spawn_recovery(layout: &InstallLayout, target: PathBuf) -> Result<()> {
    spawn_with_args(
        layout,
        &[
            update_installer::recovery::RECOVER_FLAG.into(),
            layout.update_dir().into_os_string(),
            target.into_os_string(),
            std::process::id().to_string().into(),
        ],
    )
}

/// Runs a copied helper and retains its stdin pipe until the client actually exits.
fn spawn_with_args(layout: &InstallLayout, args: &[OsString]) -> Result<()> {
    let source = std::env::current_exe()?
        .parent()
        .context("missing installed directory")?
        .join(update_installer::helper_filename());
    update_installer::helper::prune(&layout.update_dir(), std::time::SystemTime::now());
    let (helper, copy_reader) = update_installer::helper::copy(&layout.update_dir(), &source)?;
    let directory = helper.parent().context("missing helper directory")?;
    let mut command = Command::new(&helper);
    command
        .args(args)
        .current_dir(directory)
        .stdin(Stdio::piped())
        .stdout(Stdio::null())
        .stderr(fs::File::create(directory.join("apply.log"))?);
    #[cfg(target_os = "linux")]
    for name in update_installer::startup::APPIMAGE_RUNTIME_ENV {
        command.env_remove(name);
    }
    #[cfg(windows)]
    {
        use std::os::windows::process::CommandExt;
        command.creation_flags(0x00000008 | 0x08000000);
    }
    let mut child = crate::lifecycle::children::handoff_after_stopping(|| {
        let child = command.spawn()?;
        #[cfg(windows)]
        let child = {
            use std::os::windows::io::AsHandle;
            let mut child = child;
            let monitor = crate::lifecycle::crash::monitor::process_handle()?;
            update_installer::parent::transfer_monitor(
                &mut child,
                monitor.as_ref().map(AsHandle::as_handle),
            )?;
            child
        };
        Ok(child)
    })?;
    if let Some(pipe) = child.stdin.take() {
        std::mem::forget(pipe);
    }
    // The helper's shared reader overlaps this one, including a delayed process start.
    std::mem::forget(copy_reader);
    Ok(())
}

/// Revalidates before exposing a native package or its download directory to the desktop.
pub(super) fn open_download(ready: Ready) {
    std::thread::spawn(move || {
        let result = manual_package(&ready).and_then(|package| {
            ensure!(
                ready.expires > OffsetDateTime::now_utc(),
                "release expired before manual handoff"
            );
            let path = if cfg!(any(target_os = "linux", windows)) {
                package.path.parent().context("missing package directory")?
            } else {
                &package.path
            };
            let argument = if cfg!(windows) {
                path.to_str()
                    .context("invalid downloaded package path")?
                    .to_owned()
            } else {
                url::Url::from_file_path(path)
                    .map_err(|_| anyhow::anyhow!("invalid download path"))?
                    .to_string()
            };
            ensure!(
                crate::desktop::open_with_default(&argument),
                "could not open the downloaded update"
            );
            // Retain the copy for a full handoff window after the desktop accepts the path.
            package.lease.set_modified(SystemTime::now())?;
            Ok(())
        });
        if let Err(error) = result
            && let Ok(mut state) = STATE.lock()
        {
            state.message = Some(format!("Could not open update: {error:#}"));
        }
    });
}

/// Gives a verified file its published extension, so macOS and Windows open the correct installer.
fn manual_package(ready: &Ready) -> Result<ManualPackage> {
    let keys = update_manifest::parse_keys(update_installer::TRUSTED_KEYS)?;
    let platform = update_manifest::platform_key(std::env::consts::OS, std::env::consts::ARCH)
        .context("unsupported platform")?;
    let name =
        update_manifest::platforms::artifact_name(std::env::consts::OS, std::env::consts::ARCH)
            .context("unknown native artifact")?;
    snapshot_package(
        &ready.stage,
        name,
        &keys,
        Check {
            channel: ready.channel.name(),
            platform: &platform,
            current: env!("CARGO_PKG_VERSION"),
            current_build: update_manifest::build_number(),
            switch_now: ready.switch_now,
        },
        OffsetDateTime::now_utc,
    )
}

/// A verified manual installer held independently of further stage replacement or deletion.
pub(super) struct ManualPackage {
    pub(super) path: PathBuf,
    lease: File,
}

/// Verifies and copies one stage under the shared writer lease, then retains the independent copy.
pub(super) fn snapshot_package(
    stage: &Path,
    name: &str,
    keys: &update_manifest::TrustedKeys,
    check: Check<'_>,
    now: impl FnOnce() -> OffsetDateTime,
) -> Result<ManualPackage> {
    let root = stage.parent().context("missing update directory")?;
    let writer = update_installer::lease::Lease::acquire(root)?;
    let selected = update_installer::stage::verify(stage, keys, check, now())?;
    let (directory, lease) = update_installer::stage::retain(stage, &writer, &selected)?;
    let path = named_package(&directory, name, &selected.artifact)?;
    Ok(ManualPackage { path, lease })
}

/// Names a retained artifact for its desktop installer; Linux AppImages become runnable.
pub(super) fn named_package(
    directory: &Path,
    name: &str,
    artifact: &update_manifest::Artifact,
) -> Result<PathBuf> {
    let destination = directory.join(name);
    fs::rename(
        directory.join(update_installer::stage::ARTIFACT_FILE),
        &destination,
    )?;
    update_installer::stage::verify_file(&destination, artifact)?;
    #[cfg(target_os = "linux")]
    {
        use std::os::unix::fs::PermissionsExt;
        fs::set_permissions(&destination, fs::Permissions::from_mode(0o755))?;
    }
    Ok(destination)
}
