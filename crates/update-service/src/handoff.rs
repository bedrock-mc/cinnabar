//! Prepares a verified package for the platform installer when the helper cannot install.

use anyhow::{Context, Result};
use std::{
    fs::{self, File},
    path::{Path, PathBuf},
    time::SystemTime,
};
use time::OffsetDateTime;
use update_manifest::select::Check;

/// A verified native installer held independently of later stage replacement or deletion.
pub struct ManualPackage {
    pub path: PathBuf,
    lease: File,
}

impl ManualPackage {
    /// Marks the handoff, keeping the copy for a full window after the desktop opens it.
    pub fn opened(&self) -> std::io::Result<()> {
        self.lease.set_modified(SystemTime::now())
    }
}

/// Verifies and copies one stage under the shared writer lease, then retains the independent copy.
pub(crate) fn snapshot_package(
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
pub(crate) fn named_package(
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

#[cfg(test)]
mod tests {
    use super::*;
    use crate::tests::{TEST_SEED, keys};
    use sha2::Digest;
    use update_manifest::channel::Channel;

    /// Publishes a signed fixture through the real stage writer without network or installers.
    fn manual_stage(root: &Path, version: &str, payload: &[u8]) -> PathBuf {
        let platform = update_manifest::platform_key("macos", "arm64").unwrap();
        let manifest = update_manifest::Manifest {
            schema: update_manifest::SCHEMA,
            channel: Channel::Beta.name().into(),
            version: version.into(),
            build_number: 2,
            expires: OffsetDateTime::UNIX_EPOCH + time::Duration::days(1),
            notes_url: String::new(),
            artifacts: [(
                platform.clone(),
                update_manifest::Artifact {
                    url: "https://example.test/manual.dmg".into(),
                    size: payload.len() as i64,
                    sha256: format!("{:x}", sha2::Sha256::digest(payload)),
                },
            )]
            .into(),
        };
        let envelope = update_manifest::sign(&manifest, "test", &TEST_SEED).unwrap();
        let selection = update_installer::stage::Selection::verify(
            &envelope,
            &keys(),
            Check {
                channel: Channel::Beta.name(),
                platform: &platform,
                current: "1.0.0",
                current_build: 1,
                switch_now: true,
            },
            OffsetDateTime::UNIX_EPOCH,
        )
        .unwrap();
        let mut download = update_installer::stage::Download::begin(root, selection).unwrap();
        download.write(payload).unwrap();
        download.finish(&root.join("ready")).unwrap()
    }

    #[test]
    fn an_opened_package_survives_a_replaced_stage_until_its_lease_ends() {
        let root = tempfile::tempdir().unwrap();
        let stage = manual_stage(root.path(), "2.0.0", b"accepted manual installer");
        let platform = update_manifest::platform_key("macos", "arm64").unwrap();
        let package = snapshot_package(
            &stage,
            update_manifest::platforms::artifact_name("macos", "arm64").unwrap(),
            &keys(),
            Check {
                channel: Channel::Beta.name(),
                platform: &platform,
                current: "1.0.0",
                current_build: 1,
                switch_now: false,
            },
            || {
                assert!(update_installer::lease::Lease::try_acquire(root.path()).is_err());
                OffsetDateTime::UNIX_EPOCH
            },
        )
        .unwrap();
        let path = package.path.clone();
        assert!(!path.starts_with(&stage));
        manual_stage(root.path(), "3.0.0", b"replacement installer");
        fs::remove_dir_all(stage).unwrap();
        let later = SystemTime::now() + update_installer::helper::HANDOFF_WINDOW * 2;
        update_installer::helper::prune(root.path(), later);
        assert_eq!(fs::read(&path).unwrap(), b"accepted manual installer");
        drop(package);
        update_installer::helper::prune(root.path(), later);
        assert!(!path.exists());
    }

    #[cfg(target_os = "linux")]
    #[test]
    fn a_manually_downloaded_appimage_is_executable() {
        use std::os::unix::fs::PermissionsExt;
        let stage = tempfile::tempdir().unwrap();
        let payload = b"verified AppImage";
        fs::write(
            stage.path().join(update_installer::stage::ARTIFACT_FILE),
            payload,
        )
        .unwrap();
        let artifact = update_manifest::Artifact {
            url: "https://example.test/Cinnabar-x86_64.AppImage".into(),
            sha256: format!("{:x}", sha2::Sha256::digest(payload)),
            size: payload.len() as i64,
        };
        let package = named_package(
            stage.path(),
            update_manifest::platforms::artifact_name("linux", "x86_64").unwrap(),
            &artifact,
        )
        .unwrap();
        let mode = fs::metadata(package).unwrap().permissions().mode();
        assert_eq!(mode & 0o755, 0o755);
    }
}
