use super::*;
use crate::fixtures::*;

#[test]
fn refreshing_an_envelope_refuses_a_different_payload_and_keeps_the_old_envelope() {
    let root = tempfile::tempdir().unwrap();
    let (directory, keys) = staged(root.path(), "linux-x86_64");
    let original = fs::read(directory.join(ENVELOPE_FILE)).unwrap();
    let (envelope, _) = signed("linux-x86_64", "2.0.0", b"different payload");
    let selected = Selection::verify(&envelope, &keys, check("linux-x86_64"), now()).unwrap();
    let lease = crate::lease::Lease::acquire(root.path()).unwrap();
    assert!(!refresh_envelope(&directory, &lease, &selected).unwrap());
    assert_eq!(fs::read(directory.join(ENVELOPE_FILE)).unwrap(), original);
    assert_eq!(fs::read(directory.join(ARTIFACT_FILE)).unwrap(), PAYLOAD);
}

/// Downloads a newer signed release to exercise replacement of a postponed stage.
fn newer_download(root: &Path, platform: &str, keys: &TrustedKeys) -> Download {
    let payload = b"a newer verified release payload";
    let (envelope, _) = signed(platform, "3.0.0", payload);
    let selected = Selection::verify(&envelope, keys, check(platform), now()).unwrap();
    let mut download = Download::begin(root, selected).unwrap();
    download.write(payload).unwrap();
    download
}

#[test]
fn signed_manifest_download_verify_stage_and_reopen_without_network() {
    let root = tempfile::tempdir().unwrap();
    let (directory, keys) = staged(root.path(), "linux-x86_64");
    assert_eq!(fs::read(directory.join(ARTIFACT_FILE)).unwrap(), PAYLOAD);
    assert_eq!(
        verify(&directory, &keys, check("linux-x86_64"), now())
            .unwrap()
            .manifest
            .version,
        "2.0.0"
    );
}

#[test]
fn nightly_reopens_a_signed_beta_stage_and_refuses_it_after_a_newer_nightly_is_installed() {
    let root = tempfile::tempdir().unwrap();
    let platform = "linux-x86_64";
    let (directory, keys) = staged(root.path(), platform);
    let nightly = Check {
        channel: update_manifest::channel::Channel::Nightly.name(),
        current: "2.0.0-nightly",
        current_build: 1,
        ..check(platform)
    };
    let selected = verify(&directory, &keys, nightly, now()).unwrap();
    assert_eq!(
        selected.manifest.channel,
        update_manifest::channel::Channel::Beta.name()
    );
    assert!(
        verify(
            &directory,
            &keys,
            Check {
                current_build: 3,
                ..nightly
            },
            now()
        )
        .is_err()
    );
    let mut invalid: update_manifest::Envelope =
        serde_json::from_slice(&fs::read(directory.join(ENVELOPE_FILE)).unwrap()).unwrap();
    invalid.signature = "AAAA".into();
    fs::write(
        directory.join(ENVELOPE_FILE),
        serde_json::to_vec(&invalid).unwrap(),
    )
    .unwrap();
    assert!(verify(&directory, &keys, nightly, now()).is_err());
}

#[test]
fn bad_signature_channel_expiry_platform_and_downgrade_are_refused() {
    let (envelope, keys) = signed("linux-x86_64", "2.0.0", PAYLOAD);
    let mut bad: update_manifest::Envelope = serde_json::from_slice(&envelope).unwrap();
    bad.signature = "AAAA".into();
    assert!(
        Selection::verify(
            &serde_json::to_vec(&bad).unwrap(),
            &keys,
            check("linux-x86_64"),
            now()
        )
        .is_err()
    );
    for policy in [
        Check {
            channel: "stable",
            ..check("linux-x86_64")
        },
        check("windows-x86_64"),
        Check {
            current: "3.0.0",
            ..check("linux-x86_64")
        },
    ] {
        assert!(Selection::verify(&envelope, &keys, policy, now()).is_err());
    }
    assert!(
        Selection::verify(
            &envelope,
            &keys,
            check("linux-x86_64"),
            now() + time::Duration::days(2)
        )
        .is_err()
    );
}

#[test]
fn only_explicit_switch_now_permits_an_older_signed_release_on_the_chosen_channel() {
    let (envelope, keys) = signed("linux-x86_64", "0.9.0", PAYLOAD);
    assert!(Selection::verify(&envelope, &keys, check("linux-x86_64"), now()).is_err());
    assert!(
        Selection::verify(
            &envelope,
            &keys,
            Check {
                switch_now: true,
                ..check("linux-x86_64")
            },
            now()
        )
        .is_ok()
    );
    assert!(
        Selection::verify(
            &envelope,
            &keys,
            Check {
                channel: "stable",
                switch_now: true,
                ..check("linux-x86_64")
            },
            now()
        )
        .is_err()
    );
}

#[test]
fn interrupted_transfer_resumes_its_digest_named_partial_file() {
    let root = tempfile::tempdir().unwrap();
    let (envelope, keys) = signed("linux-x86_64", "2.0.0", PAYLOAD);
    let selected = Selection::verify(&envelope, &keys, check("linux-x86_64"), now()).unwrap();
    let part = root
        .path()
        .join(format!("{}.part", selected.artifact.sha256));
    let mut first = Download::begin(root.path(), selected).unwrap();
    first.write(&PAYLOAD[..7]).unwrap();
    drop(first);
    assert_eq!(fs::read(&part).unwrap(), &PAYLOAD[..7]);
    let selected = Selection::verify(&envelope, &keys, check("linux-x86_64"), now()).unwrap();
    let mut second = Download::begin(root.path(), selected).unwrap();
    assert_eq!(second.offset(), 7);
    second.write(&PAYLOAD[7..]).unwrap();
    let directory = second.finish(&root.path().join("ready")).unwrap();
    assert!(!part.exists());
    assert_eq!(fs::read(directory.join(ARTIFACT_FILE)).unwrap(), PAYLOAD);
}

#[test]
fn full_response_can_safely_restart_an_unsupported_range() {
    let root = tempfile::tempdir().unwrap();
    let (envelope, keys) = signed("linux-x86_64", "2.0.0", PAYLOAD);
    let selected = Selection::verify(&envelope, &keys, check("linux-x86_64"), now()).unwrap();
    let mut download = Download::begin(root.path(), selected).unwrap();
    download.write(&PAYLOAD[..7]).unwrap();
    download.restart().unwrap();
    assert_eq!(download.offset(), 0);
    download.write(PAYLOAD).unwrap();
    assert!(download.finish(&root.path().join("ready")).is_ok());
}

#[test]
fn short_oversized_and_corrupt_transfers_never_publish_ready_artifacts() {
    for payload in [
        PAYLOAD[..3].to_vec(),
        vec![b'x'; PAYLOAD.len()],
        vec![b'x'; PAYLOAD.len() + 1],
    ] {
        let root = tempfile::tempdir().unwrap();
        let (envelope, keys) = signed("linux-x86_64", "2.0.0", PAYLOAD);
        let selected = Selection::verify(&envelope, &keys, check("linux-x86_64"), now()).unwrap();
        let mut download = Download::begin(root.path(), selected).unwrap();
        if download.write(&payload).is_ok() {
            assert!(download.finish(&root.path().join("ready")).is_err());
        }
        assert!(!root.path().join("ready").exists());
    }
}

#[test]
fn signed_artifacts_still_obey_the_absolute_size_bound() {
    let (envelope, keys) = signed("linux-x86_64", "2.0.0", PAYLOAD);
    let mut manifest = update_manifest::verify(&envelope, &keys, now()).unwrap();
    manifest.artifacts.get_mut("linux-x86_64").unwrap().size = (MAX_ARTIFACT_BYTES + 1) as i64;
    let envelope = update_manifest::sign(&manifest, "test", &SEED).unwrap();
    assert!(Selection::verify(&envelope, &keys, check("linux-x86_64"), now()).is_err());
}

#[test]
fn an_unnumbered_signed_release_cannot_bypass_build_ordering() {
    let (envelope, keys) = signed("linux-x86_64", "2.0.0", PAYLOAD);
    let mut manifest = update_manifest::verify(&envelope, &keys, now()).unwrap();
    manifest.build_number = 0;
    let envelope = update_manifest::sign(&manifest, "test", &SEED).unwrap();
    for switch_now in [false, true] {
        assert!(
            Selection::verify(
                &envelope,
                &keys,
                Check {
                    switch_now,
                    ..check("linux-x86_64")
                },
                now(),
            )
            .is_err()
        );
    }
}

#[test]
fn a_newer_download_replaces_a_postponed_stage_without_leftovers() {
    let root = tempfile::tempdir().unwrap();
    let platform = "linux-x86_64";
    let (ready, keys) = staged(root.path(), platform);
    let download = newer_download(root.path(), platform, &keys);
    assert_eq!(download.finish(&ready).unwrap(), ready);
    let selected = verify(&ready, &keys, check(platform), now()).unwrap();
    assert_eq!(selected.manifest.version, "3.0.0");
    let names: Vec<_> = fs::read_dir(root.path())
        .unwrap()
        .map(|entry| entry.unwrap().file_name())
        .collect();
    assert_eq!(names, ["ready"]);
}

#[test]
fn failed_stage_promotion_restores_the_verified_postponed_release() {
    let root = tempfile::tempdir().unwrap();
    let platform = "linux-x86_64";
    let (ready, keys) = staged(root.path(), platform);
    let old_envelope = fs::read(ready.join(ENVELOPE_FILE)).unwrap();
    let download = newer_download(root.path(), platform, &keys);
    let mut failed = false;
    assert!(
        download
            .finish_with(&ready, |from, to| {
                if to == ready.as_path() && !failed {
                    failed = true;
                    return Err(std::io::Error::other("injected promotion failure"));
                }
                fs::rename(from, to)
            })
            .is_err()
    );
    assert!(failed);
    assert_eq!(fs::read(ready.join(ENVELOPE_FILE)).unwrap(), old_envelope);
    assert_eq!(fs::read(ready.join(ARTIFACT_FILE)).unwrap(), PAYLOAD);
    assert_eq!(
        verify(&ready, &keys, check(platform), now())
            .unwrap()
            .manifest
            .version,
        "2.0.0"
    );
    let names: Vec<_> = fs::read_dir(root.path())
        .unwrap()
        .map(|entry| entry.unwrap().file_name())
        .collect();
    assert_eq!(names, ["ready"]);
}

#[test]
fn failed_stage_restoration_keeps_the_previous_payload_for_recovery() {
    let root = tempfile::tempdir().unwrap();
    let platform = "linux-x86_64";
    let (ready, keys) = staged(root.path(), platform);
    let download = newer_download(root.path(), platform, &keys);
    assert!(
        download
            .finish_with(&ready, |from, to| {
                if to == ready.as_path() {
                    return Err(std::io::Error::other("injected rename failure"));
                }
                fs::rename(from, to)
            })
            .is_err()
    );
    assert!(!ready.exists());
    let retained = fs::read_dir(root.path())
        .unwrap()
        .next()
        .unwrap()
        .unwrap()
        .path()
        .join("stage");
    assert_eq!(fs::read(retained.join(ARTIFACT_FILE)).unwrap(), PAYLOAD);
    assert_eq!(
        verify(&retained, &keys, check(platform), now())
            .unwrap()
            .manifest
            .version,
        "2.0.0"
    );
    recover(&ready, &keys, check(platform), now()).unwrap();
    assert_eq!(
        verify(&ready, &keys, check(platform), now())
            .unwrap()
            .manifest
            .version,
        "2.0.0"
    );
}

#[test]
fn interrupted_stage_promotion_recovers_the_newest_verified_payload_without_network() {
    let root = tempfile::tempdir().unwrap();
    let platform = "linux-x86_64";
    let (ready, keys) = staged(root.path(), platform);
    let pending = root.path().join(format!("{STAGE_PREFIX}interrupted"));
    newer_download(root.path(), platform, &keys)
        .finish(&pending)
        .unwrap();
    let retired = root.path().join(format!("{RETIRED_PREFIX}interrupted"));
    fs::create_dir(&retired).unwrap();
    fs::rename(&ready, retired.join(RETIRED_STAGE)).unwrap();
    recover(&ready, &keys, check(platform), now()).unwrap();
    assert_eq!(
        verify(&ready, &keys, check(platform), now())
            .unwrap()
            .manifest
            .version,
        "3.0.0"
    );
    assert!(!pending.exists());
    assert!(!retired.exists());
}

#[test]
fn an_interrupted_unverified_stage_is_never_offered() {
    let root = tempfile::tempdir().unwrap();
    let pending = root.path().join(format!("{STAGE_PREFIX}interrupted"));
    fs::create_dir(&pending).unwrap();
    fs::write(pending.join(ENVELOPE_FILE), "{}").unwrap();
    fs::write(pending.join(ARTIFACT_FILE), "unverified").unwrap();
    let ready = root.path().join("ready");
    recover(&ready, &TrustedKeys::new(), check("linux-x86_64"), now()).unwrap();
    assert!(!ready.exists());
}
