use super::*;
use sha2::Digest;
use std::os::unix::fs::PermissionsExt;

struct Fixture {
    _root: tempfile::TempDir,
    data: PathBuf,
    update: PathBuf,
    installed: PathBuf,
    image: PathBuf,
    keys: TrustedKeys,
    platform: String,
}

impl Fixture {
    /// Creates an older script install and an independently launched signed manual package.
    fn new() -> Self {
        let root = tempfile::tempdir().unwrap();
        let data = root.path().join("profile");
        let names = update_manifest::platforms::linux_install();
        let app = data.join(&names.app_dir);
        let installed = app.join("installed build");
        let executable = target::linux_executable(&installed);
        fs::create_dir_all(executable.parent().unwrap()).unwrap();
        fs::write(executable, "installed executable").unwrap();
        fs::write(app.join(&names.current), "installed build\n").unwrap();
        let update = data.join("update");
        fs::create_dir(&update).unwrap();
        let (directory, reader) = helper::snapshot_directory(&update).unwrap();
        let image = directory.path().join(
            update_manifest::platforms::artifact_name("linux", std::env::consts::ARCH).unwrap(),
        );
        let payload = b"original signed manual update";
        fs::write(&image, payload).unwrap();
        let platform = update_manifest::platform_key("linux", std::env::consts::ARCH).unwrap();
        let seed = super::super::tests::TEST_SEED;
        let manifest = update_manifest::Manifest {
            schema: update_manifest::SCHEMA,
            channel: Channel::Beta.name().into(),
            version: "2.0.0".into(),
            build_number: 2,
            expires: OffsetDateTime::UNIX_EPOCH + time::Duration::days(1),
            notes_url: String::new(),
            artifacts: [(
                platform.clone(),
                update_manifest::Artifact {
                    url: "https://example.test/update.AppImage".into(),
                    size: payload.len() as i64,
                    sha256: format!("{:x}", sha2::Sha256::digest(payload)),
                },
            )]
            .into(),
        };
        fs::write(
            directory.path().join(stage::ENVELOPE_FILE),
            update_manifest::sign(&manifest, "test", &seed).unwrap(),
        )
        .unwrap();
        let _ = directory.keep();
        drop(reader);
        let result = Self {
            _root: root,
            data,
            update,
            installed,
            image,
            keys: TrustedKeys::from([("test".into(), update_manifest::public_key(&seed).unwrap())]),
            platform,
        };
        result.installed_build("1.0.0", 1);
        result
    }

    /// Supplies the installed executable's early version query without opening a window.
    fn installed_build(&self, version: &str, number: u64) {
        let identity = serde_json::to_string(&update_manifest::build::Build {
            version: version.into(),
            number,
        })
        .unwrap();
        let entry = self.installed.join(target::APP_RUN);
        fs::write(&entry, format!("#!/bin/sh\nprintf '%s\\n' '{identity}'\n")).unwrap();
        fs::set_permissions(entry, fs::Permissions::from_mode(0o755)).unwrap();
    }

    /// Imports the package with the worker's verification path and a deterministic clock.
    fn offer(&self) -> Result<Option<Ready>> {
        let running = Running::open(&self.update, &self.image).unwrap();
        let writer = update_installer::lease::Lease::acquire(&self.update).unwrap();
        running.offer(
            &self.data,
            &self.keys,
            None,
            &self.platform,
            &writer,
            OffsetDateTime::UNIX_EPOCH,
        )
    }
}

#[test]
fn a_manual_download_targets_the_installed_copy_and_requires_a_click() {
    let fixture = Fixture::new();
    let ready = fixture.offer().unwrap().unwrap();
    assert_eq!(
        super::super::handoff::apply_target(&ready).unwrap(),
        fixture.installed
    );
    assert_ne!(ready.target.as_ref().unwrap(), &fixture.image);
    assert_eq!(ready.notice.action(), super::super::Action::Restart);
    let selected = stage::verify(
        &ready.stage,
        &fixture.keys,
        Check {
            channel: Channel::Beta.name(),
            platform: &fixture.platform,
            current: "1.0.0",
            current_build: 1,
            switch_now: false,
        },
        OffsetDateTime::UNIX_EPOCH,
    )
    .unwrap();
    assert_eq!(selected.manifest.version, "2.0.0");
    let mut state = super::super::State::new();
    state.preferences.install_on_quit = true;
    state.ready = Some(ready);
    assert!(!state.applies_on_exit(OffsetDateTime::UNIX_EPOCH));
    assert!(state.request_restart(OffsetDateTime::UNIX_EPOCH));
    let accepted = state
        .handoff_after_exit(OffsetDateTime::UNIX_EPOCH)
        .unwrap();
    assert_eq!(accepted.target, Some(fixture.installed));
}

#[test]
fn manual_import_rejects_tampering_and_never_downgrades_an_updated_install() {
    let fixture = Fixture::new();
    fixture.installed_build("3.0.0", 3);
    assert!(fixture.offer().unwrap().is_none());
    fixture.installed_build("1.0.0", 1);
    fs::write(&fixture.image, b"tampered download").unwrap();
    assert!(fixture.offer().is_err());
}

#[test]
fn detection_ignores_other_downloads_and_protects_the_running_package_from_pruning() {
    let fixture = Fixture::new();
    let unrelated = fixture.data.join("download.AppImage");
    fs::copy(&fixture.image, &unrelated).unwrap();
    assert!(Running::open(&fixture.update, &unrelated).is_none());
    let running = Running::open(&fixture.update, &fixture.image).unwrap();
    let future =
        std::time::SystemTime::UNIX_EPOCH + std::time::Duration::from_secs(u32::MAX as u64);
    helper::prune(&fixture.update, future);
    assert!(fixture.image.exists());
    drop(running);
    helper::prune(&fixture.update, future);
    assert!(!fixture.image.exists());
}

#[test]
fn pointers_cannot_escape_the_managed_application_directory() {
    let fixture = Fixture::new();
    let names = update_manifest::platforms::linux_install();
    let pointer = fixture.data.join(&names.app_dir).join(&names.current);
    fs::write(&pointer, "../outside").unwrap();
    assert!(installed_appdir(&fixture.data).is_err());
    std::os::unix::fs::symlink(
        &fixture.update,
        pointer.parent().unwrap().join("linked-build"),
    )
    .unwrap();
    fs::write(pointer, "linked-build").unwrap();
    assert!(installed_appdir(&fixture.data).is_err());
}

#[test]
fn a_manual_copy_takes_over_promptly_after_the_installed_client_closes() {
    let fixture = Fixture::new();
    let original = update_installer::lease::Updater::try_acquire(&fixture.update).unwrap();
    let mut state = super::super::State::new();
    state.manual_download = true;
    assert!(!state.tick(&fixture.update, 10));
    drop(original);
    assert!(!state.tick(&fixture.update, 10));
    assert!(state.tick(&fixture.update, 11));
    state.ready = fixture.offer().unwrap();
    assert!(state.tick(&fixture.update, 10 + super::super::CHECK_INTERVAL.as_secs()));
}

#[test]
fn optional_recovery_ignores_other_channels_and_expiry() {
    let fixture = Fixture::new();
    let running = Running::open(&fixture.update, &fixture.image).unwrap();
    let writer = update_installer::lease::Lease::acquire(&fixture.update).unwrap();
    assert!(
        running
            .recover(
                &fixture.data,
                &fixture.keys,
                Channel::Nightly,
                &fixture.platform,
                &writer,
                OffsetDateTime::UNIX_EPOCH
            )
            .is_none()
    );
    assert!(
        running
            .recover(
                &fixture.data,
                &fixture.keys,
                Channel::Beta,
                &fixture.platform,
                &writer,
                OffsetDateTime::UNIX_EPOCH + time::Duration::days(2)
            )
            .is_none()
    );
    assert!(
        running
            .recover(
                &fixture.data,
                &fixture.keys,
                Channel::Beta,
                &fixture.platform,
                &writer,
                OffsetDateTime::UNIX_EPOCH
            )
            .is_some()
    );
}
