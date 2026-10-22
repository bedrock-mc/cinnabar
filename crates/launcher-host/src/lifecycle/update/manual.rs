//! Recovers the installation destination when an in-app download is launched separately.

use super::Ready;
use anyhow::{Context, Result, ensure};
use launcher::install_layout::InstallLayout;
use std::{
    fs::{self, File},
    io::Read,
    path::{Component, Path, PathBuf},
};
use time::OffsetDateTime;
use update_installer::{helper, stage, target};
use update_manifest::{TrustedKeys, channel::Channel, select::Check};

/// Holds the running download against cache pruning until the client exits.
pub(super) struct Running {
    image: PathBuf,
    _reader: File,
}

impl Running {
    /// Recognizes only an AppImage handed off by this installation's own updater.
    pub(super) fn detect(layout: &InstallLayout) -> Option<Self> {
        let image = PathBuf::from(std::env::var_os(update_installer::startup::APPIMAGE)?);
        installed_appdir(&layout.user_data_root).ok()?;
        Self::open(&layout.update_dir(), &image)
    }

    /// Validates the cache location before acquiring the retained package's reader lease.
    fn open(update_root: &Path, image: &Path) -> Option<Self> {
        let root = fs::canonicalize(update_root).ok()?;
        let image = fs::canonicalize(image).ok()?;
        let directory = image.parent()?;
        if directory.parent()? != root {
            return None;
        }
        let reader = helper::hold_snapshot(directory).ok()?;
        Some(Self {
            image,
            _reader: reader,
        })
    }

    /// Reauthenticates the downloaded image and stages it for the older installed AppDir.
    pub(super) fn offer(
        &self,
        user_data: &Path,
        keys: &TrustedKeys,
        platform: &str,
        writer: &update_installer::lease::Lease,
        now: OffsetDateTime,
    ) -> Result<Option<Ready>> {
        let target = installed_appdir(user_data)?;
        let installed = target::installed_build(&target)?;
        let mut envelope = Vec::new();
        File::open(
            self.image
                .parent()
                .context("missing manual package directory")?
                .join(stage::ENVELOPE_FILE),
        )?
        .take(update_manifest::MAX_ENVELOPE_BYTES as u64 + 1)
        .read_to_end(&mut envelope)?;
        let manifest = update_manifest::verify(&envelope, keys, now)?;
        let channel =
            Channel::parse(&manifest.channel).context("unknown downloaded update channel")?;
        let selected = stage::Selection::verify(
            &envelope,
            keys,
            Check {
                channel: channel.name(),
                platform,
                current: &installed.version,
                current_build: installed.number,
                switch_now: true,
            },
            now,
        )?;
        stage::verify_file(&self.image, &selected.artifact)?;
        if !update_manifest::select::newer_build(
            &selected.manifest.version,
            selected.manifest.build_number,
            &installed.version,
            installed.number,
        )? {
            return Ok(None);
        }
        ensure!(
            target::access(&target)?.permits_update(),
            "the installed copy is not writable; reinstall using an account with access to it"
        );
        let (directory, reader) = helper::snapshot_directory(writer.root())?;
        fs::write(directory.path().join(stage::ENVELOPE_FILE), envelope)?;
        fs::copy(&self.image, directory.path().join(stage::ARTIFACT_FILE))?;
        let check = Check {
            channel: channel.name(),
            platform,
            current: &installed.version,
            current_build: installed.number,
            switch_now: false,
        };
        let selected = stage::verify(directory.path(), keys, check, now)?;
        let mut ready = Ready::new(directory.keep(), reader, selected, false, true);
        ready.target = Some(target);
        Ok(Some(ready))
    }
}

/// Follows the installer's single-component pointer, refusing paths outside its AppDir root.
fn installed_appdir(user_data: &Path) -> Result<PathBuf> {
    let names = update_manifest::platforms::linux_install();
    let root = fs::canonicalize(user_data.join(&names.app_dir))?;
    let pointer = fs::read_to_string(root.join(&names.current))?;
    let build = pointer.trim_end_matches(['\r', '\n']);
    let mut components = Path::new(build).components();
    ensure!(
        matches!(components.next(), Some(Component::Normal(_))) && components.next().is_none(),
        "invalid installed build pointer"
    );
    let installed = fs::canonicalize(root.join(build))?;
    ensure!(
        installed.parent() == Some(root.as_path()),
        "installed build is outside the application directory"
    );
    target::resolve("linux", &target::linux_executable(&installed), None)
}

#[cfg(test)]
mod tests;
