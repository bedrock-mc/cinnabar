use super::*;
use crate::fixtures::*;
use crate::recovery::{cleanup_confirmed_backup, confirm_start, confirmation, remove};

#[cfg(target_os = "linux")]
#[test]
fn a_signed_appdir_update_rolls_back_failed_startup_then_relaunches_the_replacement() {
    let root = tempfile::tempdir().unwrap();
    let platform = update_manifest::platform_key("linux", std::env::consts::ARCH).unwrap();
    let (directory, keys) = staged_payload(root.path(), &platform, &linux_payload());
    let target = root.path().join("installed build");
    fs::create_dir(&target).unwrap();
    fs::write(target.join("retained"), "old").unwrap();
    let request = || Request {
        stage_dir: &directory,
        target: &target,
        keys: &keys,
        check: check(&platform),
        now: now(),
    };
    assert!(
        after_exit(
            request(),
            crate::lease::Lease::acquire(root.path()).unwrap(),
            |installed| {
                assert!(installed.is_dir());
                let backup = target::backup(installed).unwrap();
                assert_eq!(fs::read(backup.join("retained")).unwrap(), b"old");
                assert_eq!(target::installed_build(installed)?.version, "2.0.0");
                anyhow::bail!("failed startup")
            }
        )
        .is_err()
    );
    assert_eq!(fs::read(target.join("retained")).unwrap(), b"old");
    after_exit(
        request(),
        crate::lease::Lease::acquire(root.path()).unwrap(),
        |installed| {
            assert!(crate::lease::Lease::try_acquire(root.path()).is_err());
            assert_eq!(target::installed_build(installed)?.number, 2);
            Ok(())
        },
    )
    .unwrap();
    assert!(!target.join("retained").exists());
    assert!(!target::backup(&target).unwrap().exists());
    assert_eq!(
        target::resolve("linux", &target::linux_executable(&target), None).unwrap(),
        target
    );
}

#[test]
fn an_equal_or_newer_installed_build_is_relaunched_without_swapping() {
    let platform =
        update_manifest::platform_key(std::env::consts::OS, std::env::consts::ARCH).unwrap();
    for (version, build) in [("2.0.0", 2), ("2.0.0", 3), ("3.0.0", 1)] {
        let root = tempfile::tempdir().unwrap();
        let (directory, keys) = staged(root.path(), &platform);
        let target = root.path().join("installed-client");
        fs::write(&target, "newer installation").unwrap();
        let mut relaunched = false;
        after_exit(
            Request {
                stage_dir: &directory,
                target: &target,
                keys: &keys,
                check: Check {
                    current: version,
                    current_build: build,
                    ..check(&platform)
                },
                now: now(),
            },
            crate::lease::Lease::acquire(root.path()).unwrap(),
            |installed| {
                assert!(crate::lease::Lease::try_acquire(root.path()).is_err());
                assert_eq!(fs::read(installed).unwrap(), b"newer installation");
                assert!(!crate::target::backup(installed).unwrap().exists());
                relaunched = true;
                Ok(())
            },
        )
        .unwrap();
        assert!(relaunched);
        assert_eq!(fs::read(&target).unwrap(), b"newer installation");
    }
}

#[test]
fn file_and_whole_bundle_swaps_keep_the_previous_copy_until_startup() {
    for bundle in [false, true] {
        let root = tempfile::tempdir().unwrap();
        let (target, candidate, backup) = (
            root.path().join("target"),
            root.path().join("candidate"),
            root.path().join("previous"),
        );
        if bundle {
            fs::create_dir(&target).unwrap();
            fs::create_dir(&candidate).unwrap();
            fs::write(target.join("signature"), "old").unwrap();
            fs::write(candidate.join("signature"), "new").unwrap();
        } else {
            fs::write(&target, "old").unwrap();
            fs::write(&candidate, "new").unwrap();
        }
        swap(&target, &candidate, &backup).unwrap();
        assert!(backup.exists());
        if bundle {
            assert_eq!(fs::read(backup.join("signature")).unwrap(), b"old");
        } else {
            assert_eq!(fs::read(&backup).unwrap(), b"old");
        }
        remove(&backup).unwrap();
        assert!(!backup.exists());
    }
}

#[test]
fn failed_bundle_or_appimage_start_restores_the_previous_payload() {
    for bundle in [false, true] {
        let root = tempfile::tempdir().unwrap();
        let (target, candidate, backup, failed) = (
            root.path().join("target"),
            root.path().join("candidate"),
            root.path().join("previous"),
            root.path().join("failed"),
        );
        if bundle {
            fs::create_dir(&target).unwrap();
            fs::create_dir(&candidate).unwrap();
            fs::write(target.join("signature"), "old").unwrap();
            fs::write(candidate.join("signature"), "new").unwrap();
        } else {
            fs::write(&target, "old").unwrap();
            fs::write(&candidate, "new").unwrap();
        }
        swap(&target, &candidate, &backup).unwrap();
        rollback(&target, &backup, &failed).unwrap();
        if bundle {
            assert_eq!(fs::read(target.join("signature")).unwrap(), b"old");
        } else {
            assert_eq!(fs::read(&target).unwrap(), b"old");
        }
        assert!(failed.exists());
    }
}

#[test]
fn a_failed_promotion_restores_the_old_file_without_discarding_a_retained_backup() {
    let root = tempfile::tempdir().unwrap();
    let (target, candidate, backup) = (
        root.path().join("target"),
        root.path().join("missing"),
        root.path().join("previous"),
    );
    fs::write(&target, "old").unwrap();
    assert!(swap(&target, &candidate, &backup).is_err());
    assert_eq!(fs::read(&target).unwrap(), b"old");
    fs::write(&backup, "keep").unwrap();
    assert!(swap(&target, &candidate, &backup).is_err());
    assert_eq!(fs::read(&backup).unwrap(), b"keep");
}

#[test]
fn a_confirmed_start_recovers_leftover_backup_cleanup_before_the_next_swap() {
    for bundle in [false, true] {
        let root = tempfile::tempdir().unwrap();
        let target = root.path().join("target");
        let candidate = root.path().join("candidate");
        let backup = target::backup(&target).unwrap();
        if bundle {
            fs::create_dir_all(target.join("Contents/MacOS")).unwrap();
            fs::create_dir_all(candidate.join("Contents/MacOS")).unwrap();
        }
        let payload = |path: &Path| {
            if bundle {
                path.join("Contents/MacOS/client")
            } else {
                path.to_owned()
            }
        };
        fs::write(payload(&target), "first").unwrap();
        fs::write(payload(&candidate), "second").unwrap();
        swap(&target, &candidate, &backup).unwrap();
        confirm_start(&backup).unwrap();
        // Leave the confirmed backup as if cleanup was interrupted after startup.
        assert!(backup.exists());
        if bundle {
            fs::create_dir_all(candidate.join("Contents/MacOS")).unwrap();
        }
        fs::write(payload(&candidate), "third").unwrap();
        swap(&target, &candidate, &backup).unwrap();
        assert_eq!(fs::read(payload(&target)).unwrap(), b"third");
        assert_eq!(fs::read(payload(&backup)).unwrap(), b"second");
        assert!(!confirmation(&backup).exists());
        confirm_start(&backup).unwrap();
        cleanup_confirmed_backup(&target, &backup).unwrap();
        assert!(!backup.exists());
        assert!(!confirmation(&backup).exists());
    }
}

#[test]
fn a_confirmed_backup_is_preserved_if_the_replacement_is_missing() {
    let root = tempfile::tempdir().unwrap();
    let target = root.path().join("missing");
    let backup = target::backup(&target).unwrap();
    fs::write(&backup, "previous").unwrap();
    confirm_start(&backup).unwrap();
    assert!(cleanup_confirmed_backup(&target, &backup).is_err());
    assert_eq!(fs::read(&backup).unwrap(), b"previous");
    assert!(confirmation(&backup).exists());
}

#[test]
fn previous_downloaded_windows_installer_is_kept_for_manual_repair() {
    let root = tempfile::tempdir().unwrap();
    let artifact = root.path().join("payload");
    fs::write(&artifact, "first MSI").unwrap();
    remember_installer(root.path(), &artifact).unwrap();
    fs::write(&artifact, "second MSI").unwrap();
    remember_installer(root.path(), &artifact).unwrap();
    assert_eq!(
        fs::read(root.path().join("previous.msi")).unwrap(),
        b"first MSI"
    );
    assert_eq!(
        fs::read(root.path().join("installed.msi")).unwrap(),
        b"second MSI"
    );
}

#[cfg(target_os = "linux")]
#[test]
fn apply_keeps_the_writer_lease_and_rolls_back_a_failed_new_start() {
    let root = tempfile::tempdir().unwrap();
    let (directory, keys) = staged(root.path(), "linux-x86_64");
    let target = root.path().join("client.AppImage");
    fs::write(&target, "old").unwrap();
    let request = || Request {
        stage_dir: &directory,
        target: &target,
        keys: &keys,
        check: check("linux-x86_64"),
        now: now(),
    };
    let other_client = crate::lease::Lease::acquire(root.path()).unwrap();
    assert!(crate::lease::Lease::try_acquire(root.path()).is_err());
    drop(other_client);
    assert_eq!(fs::read(&target).unwrap(), b"old");
    assert!(
        after_exit(
            request(),
            crate::lease::Lease::acquire(root.path()).unwrap(),
            |_| anyhow::bail!("failed startup")
        )
        .is_err()
    );
    assert_eq!(fs::read(&target).unwrap(), b"old");
    after_exit(
        request(),
        crate::lease::Lease::acquire(root.path()).unwrap(),
        |target| {
            assert_eq!(fs::read(target).unwrap(), PAYLOAD);
            assert!(crate::target::backup(target).unwrap().exists());
            Ok(())
        },
    )
    .unwrap();
    assert!(!crate::target::backup(&target).unwrap().exists());
}

#[cfg(target_os = "linux")]
#[test]
fn all_tampered_stages_are_refused_before_start_or_swap() {
    for case in [
        "signature",
        "hash",
        "truncated",
        "channel",
        "expired",
        "platform",
    ] {
        let root = tempfile::tempdir().unwrap();
        let (directory, keys) = staged(root.path(), "linux-x86_64");
        let target = root.path().join("client.AppImage");
        fs::write(&target, "old").unwrap();
        let mut policy = check("linux-x86_64");
        let mut clock = now();
        match case {
            "signature" => fs::write(directory.join(stage::ENVELOPE_FILE), "{}").unwrap(),
            "hash" => fs::write(
                directory.join(stage::ARTIFACT_FILE),
                vec![b'x'; PAYLOAD.len()],
            )
            .unwrap(),
            "truncated" => fs::write(directory.join(stage::ARTIFACT_FILE), &PAYLOAD[..2]).unwrap(),
            "channel" => policy.channel = update_manifest::channel::Channel::Stable.name(),
            "expired" => clock += time::Duration::days(2),
            "platform" => policy.platform = "windows-x86_64",
            _ => unreachable!(),
        }
        assert!(
            after_exit(
                Request {
                    stage_dir: &directory,
                    target: &target,
                    keys: &keys,
                    check: policy,
                    now: clock
                },
                crate::lease::Lease::acquire(root.path()).unwrap(),
                |_| panic!("unverified release executed for {case}")
            )
            .is_err()
        );
        assert_eq!(fs::read(&target).unwrap(), b"old");
    }
}

#[cfg(target_os = "linux")]
#[test]
fn a_refused_swap_cannot_restore_another_clients_unconfirmed_predecessor() {
    let root = tempfile::tempdir().unwrap();
    let (directory, keys) = staged(root.path(), "linux-x86_64");
    let target = root.path().join("client.AppImage");
    let backup = crate::target::backup(&target).unwrap();
    fs::write(&target, "other client's installation").unwrap();
    fs::write(&backup, "other client's predecessor").unwrap();
    let error = after_exit(
        Request {
            stage_dir: &directory,
            target: &target,
            keys: &keys,
            check: check("linux-x86_64"),
            now: now(),
        },
        crate::lease::Lease::acquire(root.path()).unwrap(),
        |_| panic!("a refused swap must not launch a replacement"),
    )
    .unwrap_err();
    assert!(error.is::<Untouched>());
    assert_eq!(fs::read(&target).unwrap(), b"other client's installation");
    assert_eq!(fs::read(&backup).unwrap(), b"other client's predecessor");
}
