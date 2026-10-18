use super::*;
use update_manifest::TrustedKeys;

pub(super) const TEST_SEED: [u8; update_manifest::KEY_BYTES] = [7; update_manifest::KEY_BYTES];

#[test]
fn running_clients_check_each_interval_from_completion_without_skipped_ticks_deferring_it() {
    let root = tempfile::tempdir().unwrap();
    let mut state = State::new();
    let ticks_per_interval = CHECK_INTERVAL.as_secs() / CHECK_POLL_INTERVAL.as_secs();
    let check = update_manifest::select::Check {
        channel: Channel::Beta.name(),
        platform: "macos-arm64",
        current: env!("CARGO_PKG_VERSION"),
        current_build: update_manifest::build_number(),
        switch_now: false,
    };
    let mut finish = None;
    let mut starts = Vec::new();
    for tick in 0..=3 * (ticks_per_interval + 1) {
        let now = tick * CHECK_POLL_INTERVAL.as_secs();
        if finish == Some(tick) {
            worker::record(&worker::stamp_path(root.path(), Channel::Beta), check, now).unwrap();
            state.running = false;
            finish = None;
        }
        if state.tick(root.path(), now) {
            starts.push(tick);
            state.running = true;
            finish = Some(tick + 1);
        }
    }
    assert_eq!(
        starts,
        [
            0,
            ticks_per_interval + 1,
            2 * (ticks_per_interval + 1),
            3 * (ticks_per_interval + 1)
        ]
    );
}

#[test]
fn a_passive_client_takes_over_on_its_next_due_tick_after_the_updater_exits() {
    let root = tempfile::tempdir().unwrap();
    let mut first = State::new();
    let mut second = State::new();
    assert!(first.tick(root.path(), 1));
    assert!(!second.tick(root.path(), 1));
    assert!(second.updater.is_none());
    drop(first);
    assert!(!second.tick(root.path(), 2));
    assert!(second.tick(root.path(), 1 + CHECK_INTERVAL.as_secs()));
    assert!(second.updater.is_some());
}

#[test]
fn update_http_client_refuses_plaintext_before_connecting() {
    let listener = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
    listener.set_nonblocking(true).unwrap();
    let url = format!("http://{}/update.json", listener.local_addr().unwrap());
    let runtime = tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
        .unwrap();
    assert!(
        runtime
            .block_on(worker::fetch(&worker::http_client().unwrap(), &url))
            .is_err()
    );
    assert_eq!(
        listener.accept().unwrap_err().kind(),
        std::io::ErrorKind::WouldBlock
    );
}

#[test]
fn checks_remain_periodic_when_the_clock_moves_back() {
    assert!(due(None, 10));
    assert!(!due(Some(1_000), 1_000 + CHECK_INTERVAL.as_secs() - 1));
    assert!(due(Some(1_000), 1_000 + CHECK_INTERVAL.as_secs()));
    assert!(due(Some(5_000), 100));
}

/// Release hints are channel-scoped and compare nightly build numbers before scheduling.
#[test]
fn release_notices_check_only_newer_builds_on_the_selected_channel() {
    let release = |channel: &str, version: &str, build| bridge::CinnabarRelease {
        channel: channel.into(),
        version: version.into(),
        build,
        ..Default::default()
    };
    let mut state = State::new();
    for hint in [
        release("nightly", "9.0.0", 50),
        release("beta", "1.2.2", 50),
        release("beta", "not a version", 50),
        release("beta", "1.2.3", 42),
    ] {
        assert!(!state.release_published(&hint, ("1.2.3", 42), 1000, 300));
    }
    let hint = release("beta", "1.2.3", 43);
    assert!(state.release_published(&hint, ("1.2.3", 42), 1000, 300));
    assert!(!state.check_now);
    assert_eq!(state.announcement.as_ref().unwrap().not_before, 1300);
    assert!(!state.release_published(&hint, ("1.2.3", 42), 1100, 0));
    let checked = state.announcement.clone();
    state.finish_announcement(checked.as_ref(), true, 1300, 0);
    assert_eq!(state.announcement.as_ref().unwrap().not_before, 1330);
    let checked = state.announcement.clone();
    state.finish_announcement(checked.as_ref(), false, 1330, 0);
    assert!(state.announcement.is_none());
    state.preferences.checks = false;
    assert!(!state.release_published(&hint, ("1.2.3", 42), 1000, 0));
}

/// Older v1 servers omit build numbers, so their same-version rolling notices still warrant a check.
#[test]
fn legacy_release_without_build_still_triggers_a_check() {
    let mut state = State::new();
    let release = bridge::CinnabarRelease {
        channel: "beta".into(),
        version: "1.2.3".into(),
        ..Default::default()
    };
    assert!(state.release_published(&release, ("1.2.3", 42), 1000, 300));
    assert_eq!(state.announcement.as_ref().unwrap().not_before, 1300);
}

/// A verified notice model lets consent tests avoid any network or platform installer.
fn ready(writable: bool) -> Ready {
    Ready {
        expires: OffsetDateTime::UNIX_EPOCH + time::Duration::days(1),
        stage: PathBuf::from("ready"),
        channel: Channel::Beta,
        switch_now: false,
        lease: None,
        notice: Notice {
            version: "2.0.0".into(),
            writable,
            protocol_required: false,
        },
    }
}

#[test]
fn checking_does_not_authorize_installing_and_later_defers_an_opt_in() {
    let mut state = State::new();
    state.ready = Some(ready(true));
    assert!(!state.applies_on_exit(OffsetDateTime::UNIX_EPOCH));
    state.preferences.install_on_quit = true;
    assert!(state.applies_on_exit(OffsetDateTime::UNIX_EPOCH));
    state.dismissed = true;
    assert!(!state.applies_on_exit(OffsetDateTime::UNIX_EPOCH));
    state.requested = true;
    assert!(state.applies_on_exit(OffsetDateTime::UNIX_EPOCH));
}

#[test]
fn a_not_writable_install_cannot_launch_an_apply_helper() {
    let mut state = State::new();
    state.ready = Some(ready(false));
    state.preferences.install_on_quit = true;
    state.requested = true;
    assert!(!state.applies_on_exit(OffsetDateTime::UNIX_EPOCH));
    assert!(
        state
            .handoff_after_exit(OffsetDateTime::UNIX_EPOCH)
            .is_none()
    );
    assert_eq!(state.ready.unwrap().notice.action(), Action::Download);
}

#[test]
fn failed_helper_preparation_reopens_the_retained_client_once() {
    let mut restarted = 0;
    let error = handoff::spawn_or_restart(
        || anyhow::bail!("helper copy failed"),
        || {
            restarted += 1;
            Ok(())
        },
    )
    .unwrap_err();
    assert_eq!(restarted, 1);
    assert_eq!(error.to_string(), "helper copy failed");
    handoff::spawn_or_restart(|| Ok(()), || panic!("a running helper owns the restart")).unwrap();
}

#[test]
fn expired_ready_updates_never_authorize_exit_time_installation() {
    let mut state = State::new();
    state.ready = Some(ready(true));
    state.requested = true;
    assert!(!state.applies_on_exit(OffsetDateTime::UNIX_EPOCH + time::Duration::days(2)));
}

#[test]
fn accepted_restart_freezes_the_stage_while_a_newer_download_finishes() {
    let root = tempfile::tempdir().unwrap();
    let stage = root.path().join("ready");
    std::fs::create_dir(&stage).unwrap();
    std::fs::write(
        stage.join(update_installer::stage::ARTIFACT_FILE),
        "accepted release",
    )
    .unwrap();
    let mut state = State::new();
    state.ready = Some(Ready {
        stage: stage.clone(),
        ..ready(true)
    });
    let downloading_generation = state.generation;
    assert!(state.request_restart(OffsetDateTime::UNIX_EPOCH));
    assert!(!state.preferences.install_on_quit);
    assert!(!state.accepts_worker(downloading_generation));
    assert!(!state.accepts_worker(state.generation));
    assert!(state.applies_on_exit(OffsetDateTime::UNIX_EPOCH));
    assert_eq!(state.ready.as_ref().unwrap().stage, stage);
    assert_eq!(
        std::fs::read(stage.join(update_installer::stage::ARTIFACT_FILE)).unwrap(),
        b"accepted release"
    );
}

#[test]
fn expiry_during_shutdown_keeps_the_accepted_handoff_but_never_authorizes_an_expired_install() {
    let mut state = State::new();
    state.ready = Some(ready(true));
    assert!(state.request_restart(OffsetDateTime::UNIX_EPOCH));
    let expired = OffsetDateTime::UNIX_EPOCH + time::Duration::days(2);
    assert!(!state.applies_on_exit(expired));
    assert!(state.handoff_after_exit(expired).is_some());
    let mut unaccepted = State::new();
    unaccepted.ready = Some(ready(true));
    unaccepted.preferences.install_on_quit = true;
    assert!(unaccepted.handoff_after_exit(expired).is_none());
}

#[test]
fn a_missing_payload_refreshes_the_channel_once_and_keeps_failures_retryable() {
    let mut fetched = 0;
    worker::retry_missing(
        || {
            fetched += 1;
            worker::check_missing(if fetched == 1 {
                reqwest::StatusCode::NOT_FOUND
            } else {
                reqwest::StatusCode::OK
            })
        },
        || true,
    )
    .unwrap();
    assert_eq!(fetched, 2);
    fetched = 0;
    assert!(
        worker::retry_missing(
            || {
                fetched += 1;
                worker::check_missing(reqwest::StatusCode::NOT_FOUND)
            },
            || true
        )
        .is_err()
    );
    assert_eq!(fetched, 2);
    assert!(due(None, 0));
}

#[test]
fn cancellation_and_other_download_failures_do_not_trigger_manifest_retry() {
    for missing in [false, true] {
        let mut fetched = 0;
        assert!(
            worker::retry_missing(
                || {
                    fetched += 1;
                    if missing {
                        Err(worker::MissingArtifact.into())
                    } else {
                        anyhow::bail!("connection failed")
                    }
                },
                || false
            )
            .is_err()
        );
        assert_eq!(fetched, 1);
    }
}

/// A signed envelope for `version` on the stable channel, as a manifest fetch would cache it.
fn cached_release(path: &std::path::Path, version: &str, build_number: u64) -> TrustedKeys {
    let seed = TEST_SEED;
    let manifest = update_manifest::Manifest {
        schema: update_manifest::SCHEMA,
        channel: "stable".into(),
        version: version.into(),
        build_number,
        expires: OffsetDateTime::now_utc() + time::Duration::days(1),
        notes_url: String::new(),
        artifacts: [(
            "macos-arm64".to_owned(),
            update_manifest::Artifact {
                url: "https://example.test/c.dmg".into(),
                sha256: "a".repeat(64),
                size: 10,
            },
        )]
        .into(),
    };
    std::fs::write(path, update_manifest::sign(&manifest, "k", &seed).unwrap()).unwrap();
    TrustedKeys::from([("k".to_owned(), update_manifest::public_key(&seed).unwrap())])
}

#[cfg(target_os = "linux")]
#[test]
fn a_manually_downloaded_appimage_is_executable() {
    use sha2::Digest;
    use std::os::unix::fs::PermissionsExt;
    let stage = tempfile::tempdir().unwrap();
    let payload = b"verified AppImage";
    std::fs::write(
        stage.path().join(update_installer::stage::ARTIFACT_FILE),
        payload,
    )
    .unwrap();
    let artifact = update_manifest::Artifact {
        url: "https://example.test/Cinnabar-x86_64.AppImage".into(),
        sha256: format!("{:x}", sha2::Sha256::digest(payload)),
        size: payload.len() as i64,
    };
    let package = handoff::named_package(
        stage.path(),
        update_manifest::platforms::artifact_name("linux", "x86_64").unwrap(),
        &artifact,
    )
    .unwrap();
    let mode = std::fs::metadata(package).unwrap().permissions().mode();
    assert_eq!(mode & 0o755, 0o755);
}

/// Publishes a signed fixture through the real stage writer without network or installer processes.
fn manual_stage(
    root: &std::path::Path,
    version: &str,
    payload: &[u8],
    channel: Channel,
) -> (PathBuf, TrustedKeys) {
    use sha2::Digest;
    let seed = TEST_SEED;
    let platform = update_manifest::platform_key("macos", "arm64").unwrap();
    let keys = TrustedKeys::from([("test".into(), update_manifest::public_key(&seed).unwrap())]);
    let manifest = update_manifest::Manifest {
        schema: update_manifest::SCHEMA,
        channel: channel.name().into(),
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
    let envelope = update_manifest::sign(&manifest, "test", &seed).unwrap();
    let selection = update_installer::stage::Selection::verify(
        &envelope,
        &keys,
        update_manifest::select::Check {
            channel: channel.name(),
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
    (download.finish(&root.join("ready")).unwrap(), keys)
}

#[test]
fn a_new_owner_restores_downloaded_offers_while_the_completion_stamp_is_fresh() {
    let root = tempfile::tempdir().unwrap();
    let mut first = State::new();
    let mut passive = State::new();
    assert!(first.tick(root.path(), 100));
    let (directory, keys) =
        manual_stage(root.path(), "2.0.0", b"downloaded release", Channel::Beta);
    let check = update_manifest::select::Check {
        channel: Channel::Beta.name(),
        platform: "macos-arm64",
        current: env!("CARGO_PKG_VERSION"),
        current_build: update_manifest::build_number(),
        switch_now: false,
    };
    worker::record(&worker::stamp_path(root.path(), Channel::Beta), check, 100).unwrap();
    assert!(!passive.tick(root.path(), 100));
    assert!(passive.ready.is_none());
    drop(first);
    let now = 100 + CHECK_POLL_INTERVAL.as_secs();
    assert!(!due(worker::last_check(root.path(), Channel::Beta), now));
    assert!(
        passive.tick(root.path(), now),
        "ownership acquisition must start cache restoration"
    );
    let writer = update_installer::lease::Lease::acquire(root.path()).unwrap();
    let selected =
        update_installer::stage::verify(&directory, &keys, check, OffsetDateTime::UNIX_EPOCH)
            .unwrap();
    let (stage, reader) = update_installer::stage::retain(&directory, &writer, &selected).unwrap();
    passive.offer(
        Ready::new(stage, reader, selected, false, true),
        passive.generation,
    );
    assert_eq!(passive.ready.as_ref().unwrap().notice.version, "2.0.0");
    assert_eq!(worker::last_check(root.path(), Channel::Beta), Some(100));
}

#[test]
fn an_explicit_switch_downloads_an_older_release_instead_of_retaining_a_newer_stage() {
    let root = tempfile::tempdir().unwrap();
    let (directory, keys) = manual_stage(
        root.path(),
        "0.0.1",
        b"requested older payload",
        Channel::Beta,
    );
    let check = update_manifest::select::Check {
        channel: Channel::Beta.name(),
        platform: "macos-arm64",
        current: env!("CARGO_PKG_VERSION"),
        current_build: update_manifest::build_number(),
        switch_now: true,
    };
    let selected =
        update_installer::stage::verify(&directory, &keys, check, OffsetDateTime::UNIX_EPOCH)
            .unwrap();
    let (directory, _) = manual_stage(
        root.path(),
        "0.0.2",
        b"retained newer payload",
        Channel::Beta,
    );
    let retained =
        update_installer::stage::verify(&directory, &keys, check, OffsetDateTime::UNIX_EPOCH)
            .unwrap();
    assert!(!worker::supersedes(&selected.manifest, Some(&retained.manifest), false).unwrap());
    let mut state = State::new();
    worker::offer_switch(&mut state);
    assert!(state.request_switch());
    let generation = state.generation;
    assert!(
        worker::supersedes(
            &selected.manifest,
            Some(&retained.manifest),
            state.switch_now
        )
        .unwrap()
    );
    let writer = update_installer::lease::Lease::acquire(root.path()).unwrap();
    assert!(
        worker::reuse_stage(&writer, &selected, [directory.as_path()])
            .unwrap()
            .is_none()
    );
    let mut download = update_installer::stage::Download::begin(root.path(), selected).unwrap();
    download.write(b"requested older payload").unwrap();
    let directory = download.finish(&directory).unwrap();
    let selected =
        update_installer::stage::verify(&directory, &keys, check, OffsetDateTime::UNIX_EPOCH)
            .unwrap();
    let (stage, reader) = update_installer::stage::retain(&directory, &writer, &selected).unwrap();
    state.offer(Ready::new(stage, reader, selected, true, false), generation);
    assert!(!state.finish_check(generation, true, false));
    let offer = state.ready.as_ref().unwrap();
    assert_eq!(offer.notice.version, "0.0.1");
    assert_eq!(offer.notice.action(), Action::Download);
    assert_eq!(
        std::fs::read(offer.stage.join(update_installer::stage::ARTIFACT_FILE)).unwrap(),
        b"requested older payload"
    );
    assert!(state.message.is_none());
    assert!(!state.switch_now);
    assert!(
        worker::retained_manifest(
            Some(offer),
            &keys,
            update_manifest::select::Check {
                switch_now: false,
                ..check
            },
            OffsetDateTime::UNIX_EPOCH
        )
        .is_some()
    );
}

#[test]
fn every_manual_switch_outcome_ends_progress_or_preserves_only_a_newer_actions_progress() {
    for failed in [false, true] {
        let mut state = State::new();
        worker::offer_switch(&mut state);
        assert!(state.request_switch());
        state.running = true;
        let progress = state.message.clone();
        assert!(!state.finish_check(state.generation, true, failed));
        assert_ne!(state.message, progress);
        assert_eq!(state.message.is_some(), failed);
        assert_eq!(state.switch_available, failed);
        assert!(!state.switch_now && !state.running);
    }
    let mut state = State::new();
    worker::offer_switch(&mut state);
    assert!(state.request_switch());
    let canceled = state.generation;
    state.configure(Preferences {
        channel: Channel::Nightly,
        ..state.preferences
    });
    assert!(state.finish_check(canceled, true, false));
    assert!(state.message.is_none());
    worker::offer_switch(&mut state);
    assert!(state.request_switch());
    let previous = state.generation;
    worker::offer_switch(&mut state);
    assert!(state.request_switch());
    let current_progress = state.message.clone();
    assert!(state.finish_check(previous, true, false));
    assert_eq!(state.message, current_progress);
    assert!(state.switch_now);
    assert!(!state.finish_check(state.generation, true, false));
    assert!(state.message.is_none());
}

#[test]
fn reselecting_a_channel_restores_its_cached_release_before_the_next_deadline() {
    let root = tempfile::tempdir().unwrap();
    let payload = b"cached Beta payload";
    let (beta, keys) = manual_stage(root.path(), "2.0.0", payload, Channel::Beta);
    let cache = root.path().join("manifest-beta.json");
    std::fs::copy(beta.join(update_installer::stage::ENVELOPE_FILE), &cache).unwrap();
    let check = update_manifest::select::Check {
        channel: Channel::Beta.name(),
        platform: "macos-arm64",
        current: env!("CARGO_PKG_VERSION"),
        current_build: update_manifest::build_number(),
        switch_now: false,
    };
    worker::record(&worker::stamp_path(root.path(), Channel::Beta), check, 100).unwrap();
    assert!(!due(worker::last_check(root.path(), Channel::Beta), 101));
    let mut state = State::new();
    state.ready = Some(ready(true));
    assert_eq!(
        state.configure(Preferences {
            channel: Channel::Nightly,
            ..state.preferences
        }),
        Some(true)
    );
    let _ = manual_stage(root.path(), "3.0.0", b"Nightly payload", Channel::Nightly);
    assert_eq!(
        state.configure(Preferences {
            channel: Channel::Beta,
            ..state.preferences
        }),
        Some(true)
    );
    let selected =
        releases::cached_channel(&cache, &keys, check, OffsetDateTime::UNIX_EPOCH).unwrap();
    let writer = update_installer::lease::Lease::acquire(root.path()).unwrap();
    assert!(
        worker::reuse_stage(&writer, &selected, [beta.as_path()])
            .unwrap()
            .is_none()
    );
    let mut download = update_installer::stage::Download::begin(root.path(), selected).unwrap();
    download.write(payload).unwrap();
    let directory = download.finish(&beta).unwrap();
    let selected =
        update_installer::stage::verify(&directory, &keys, check, OffsetDateTime::UNIX_EPOCH)
            .unwrap();
    let (directory, reader) =
        update_installer::stage::retain(&directory, &writer, &selected).unwrap();
    state.ready = Some(Ready::new(directory, reader, selected, false, true));
    let restored = state.ready.unwrap();
    assert_eq!(restored.notice.version, "2.0.0");
    assert_eq!(restored.channel, Channel::Beta);
    assert_eq!(
        std::fs::read(restored.stage.join(update_installer::stage::ARTIFACT_FILE)).unwrap(),
        payload
    );
}

#[test]
fn renewed_envelopes_replace_expiring_stages_and_offers_without_a_download() {
    let root = tempfile::tempdir().unwrap();
    let payload = b"unchanged authenticated payload";
    let (directory, keys) = manual_stage(root.path(), "2.0.0", payload, Channel::Beta);
    let check = update_manifest::select::Check {
        channel: Channel::Beta.name(),
        platform: "macos-arm64",
        current: "1.0.0",
        current_build: 1,
        switch_now: false,
    };
    let original =
        update_installer::stage::verify(&directory, &keys, check, OffsetDateTime::UNIX_EPOCH)
            .unwrap();
    let mut manifest = original.manifest.clone();
    manifest.expires += time::Duration::days(1);
    let renewed = update_manifest::sign(&manifest, "test", &TEST_SEED).unwrap();
    let now = original.manifest.expires + time::Duration::seconds(1);
    let selected = update_installer::stage::Selection::verify(&renewed, &keys, check, now).unwrap();
    assert!(worker::supersedes(&selected.manifest, Some(&original.manifest), false).unwrap());
    assert!(update_installer::stage::verify(&directory, &keys, check, now).is_err());
    let writer = update_installer::lease::Lease::acquire(root.path()).unwrap();
    let reused = worker::reuse_stage(&writer, &selected, [directory.as_path()])
        .unwrap()
        .expect("renewal must reuse the payload without a download");
    assert_eq!(reused, directory);
    assert_eq!(
        std::fs::read(directory.join(update_installer::stage::ENVELOPE_FILE)).unwrap(),
        renewed
    );
    let selected = update_installer::stage::verify(&reused, &keys, check, now).unwrap();
    let (stage, reader) = update_installer::stage::retain(&reused, &writer, &selected).unwrap();
    let offer = Ready::new(stage, reader, selected, false, true);
    assert_eq!(offer.expires, manifest.expires);
    assert_eq!(
        std::fs::read(offer.stage.join(update_installer::stage::ARTIFACT_FILE)).unwrap(),
        payload
    );
    assert!(update_installer::stage::verify(&offer.stage, &keys, check, now).is_ok());
}

#[test]
fn replacing_ready_during_manual_handoff_keeps_the_handed_off_installer() {
    let root = tempfile::tempdir().unwrap();
    let (stage, keys) = manual_stage(
        root.path(),
        "2.0.0",
        b"accepted manual installer",
        Channel::Beta,
    );
    let platform = update_manifest::platform_key("macos", "arm64").unwrap();
    let package = handoff::snapshot_package(
        &stage,
        update_manifest::platforms::artifact_name("macos", "arm64").unwrap(),
        &keys,
        update_manifest::select::Check {
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
    // The worker can take its lease again before the desktop opens the manual installer.
    let writer = std::fs::File::options()
        .write(true)
        .open(root.path().join(update_installer::LOCK_FILE))
        .unwrap();
    writer.try_lock().unwrap();
    manual_stage(
        root.path(),
        "3.0.0",
        b"replacement installer",
        Channel::Beta,
    );
    assert_eq!(
        std::fs::read(stage.join(update_installer::stage::ARTIFACT_FILE)).unwrap(),
        b"replacement installer"
    );
    std::fs::remove_dir_all(stage).unwrap();
    let later = std::time::SystemTime::now() + update_installer::helper::HANDOFF_WINDOW * 2;
    update_installer::helper::prune(root.path(), later);
    assert_eq!(std::fs::read(&path).unwrap(), b"accepted manual installer");
    drop(package);
    update_installer::helper::prune(root.path(), later);
    assert!(!path.exists());
}

#[test]
fn a_retained_offer_stays_valid_when_the_shared_cache_changes() {
    let root = tempfile::tempdir().unwrap();
    let (shared_stage, keys) =
        manual_stage(root.path(), "2.0.0", b"accepted release", Channel::Beta);
    let platform = update_manifest::platform_key("macos", "arm64").unwrap();
    let check = update_manifest::select::Check {
        channel: Channel::Beta.name(),
        platform: &platform,
        current: "1.0.0",
        current_build: 1,
        switch_now: false,
    };
    let writer = update_installer::lease::Lease::acquire(root.path()).unwrap();
    let selected =
        update_installer::stage::verify(&shared_stage, &keys, check, OffsetDateTime::UNIX_EPOCH)
            .unwrap();
    let (stage, lease) =
        update_installer::stage::retain(&shared_stage, &writer, &selected).unwrap();
    drop(writer);
    let mut state = State::new();
    state.ready = Some(Ready {
        stage: stage.clone(),
        lease: Some(std::sync::Arc::new(lease)),
        ..ready(true)
    });
    assert!(state.request_restart(OffsetDateTime::UNIX_EPOCH));
    let accepted = state
        .handoff_after_exit(OffsetDateTime::UNIX_EPOCH)
        .unwrap();
    // The helper's reader can overlap the parent's reader before the parent actually exits.
    let helper_reader = update_installer::helper::hold_snapshot(&accepted.stage).unwrap();
    let other_client = update_installer::lease::Lease::acquire(root.path()).unwrap();
    std::fs::remove_dir_all(&shared_stage).unwrap();
    let _ = manual_stage(
        root.path(),
        "3.0.0",
        b"another channel's download",
        Channel::Nightly,
    );
    std::fs::remove_dir_all(&shared_stage).unwrap();
    drop(other_client);
    let selected =
        worker::retained_manifest(Some(&accepted), &keys, check, OffsetDateTime::UNIX_EPOCH)
            .unwrap();
    assert_eq!(selected.version, accepted.notice.version);
    assert_eq!(
        std::fs::read(accepted.stage.join(update_installer::stage::ARTIFACT_FILE)).unwrap(),
        b"accepted release"
    );
    assert!(
        update_installer::stage::verify(&accepted.stage, &keys, check, OffsetDateTime::UNIX_EPOCH)
            .is_ok()
    );
    let later = std::time::SystemTime::now() + update_installer::helper::HANDOFF_WINDOW * 2;
    drop(state);
    drop(accepted);
    update_installer::helper::prune(root.path(), later);
    assert!(stage.exists());
    drop(helper_reader);
    update_installer::helper::prune(root.path(), later);
    assert!(!stage.exists());
}

#[test]
fn a_skipped_periodic_check_restores_the_older_channel_switch_offer() {
    let root = tempfile::tempdir().unwrap();
    let path = root.path().join("manifest-stable.json");
    let check = update_manifest::select::Check {
        channel: "stable",
        platform: "macos-arm64",
        current: "2.0.0-nightly",
        current_build: 50,
        switch_now: false,
    };
    assert!(!worker::cached_switch_offer(
        root.path(),
        &TrustedKeys::new(),
        check,
        OffsetDateTime::now_utc()
    ));
    let keys = cached_release(&path, "1.0.0", 40);
    assert!(worker::cached_switch_offer(
        root.path(),
        &keys,
        check,
        OffsetDateTime::now_utc()
    ));
    let mut state = State::new();
    worker::offer_switch(&mut state);
    assert!(state.switch_available && state.message.is_some());
    // A newer release is a download, not a switch.
    let keys = cached_release(&path, "3.0.0", 60);
    assert!(!worker::cached_switch_offer(
        root.path(),
        &keys,
        check,
        OffsetDateTime::now_utc()
    ));
}

#[test]
fn a_postponed_stage_is_replaced_only_by_a_newer_release() {
    let root = tempfile::tempdir().unwrap();
    let path = root.path().join("manifest.json");
    let release = |version, build| {
        let keys = cached_release(&path, version, build);
        let mut manifest = update_manifest::verify(
            &std::fs::read(&path).unwrap(),
            &keys,
            OffsetDateTime::now_utc(),
        )
        .unwrap();
        manifest.expires = OffsetDateTime::UNIX_EPOCH + time::Duration::days(1);
        manifest
    };
    let staged = release("2.0.0", 10);
    assert!(worker::supersedes(&staged, None, false).unwrap());
    assert!(!worker::supersedes(&release("2.0.0", 10), Some(&staged), false).unwrap());
    assert!(worker::supersedes(&release("2.0.0", 11), Some(&staged), false).unwrap());
    assert!(worker::supersedes(&release("2.1.0", 1), Some(&staged), false).unwrap());
}

/// Manual checks bypass the saved automatic-check preference and reject duplicate work.
#[test]
fn manual_checks_preserve_preferences_and_wait_for_an_idle_started_client() {
    let mut state = State::new();
    state.preferences.checks = false;
    assert!(!state.request_check());
    state.startup = StartupPhase::Complete;
    assert!(state.request_check());
    assert!(!state.preferences.checks);
    assert!(state.manual_check && state.check_now);
    state.running = true;
    assert!(!state.request_check());
    state.running = false;
    state.requested = true;
    assert!(!state.request_check());
}

/// Completed, failed and cancelled workers must not leave a permanent busy indicator.
#[test]
fn check_status_finishes_without_overwriting_a_newer_channels_state() {
    let mut state = State::new();
    state.running = true;
    state.status = Status::Downloading;
    assert!(!state.finish_check(state.generation, false, false));
    assert_eq!(state.status, Status::Idle);
    state.status = Status::Checking;
    assert!(!state.finish_check(state.generation, false, true));
    assert_eq!(state.status, Status::Failed);
    let old = state.generation;
    state.configure(Preferences {
        channel: Channel::Nightly,
        ..state.preferences
    });
    assert!(state.finish_check(old, false, true));
    assert_eq!(state.status, Status::Idle);
    state.status = Status::UpToDate;
    assert!(!state.finish_check(state.generation, false, false));
    assert_eq!(state.status, Status::UpToDate);
}

/// Cancelling a worker on a preference change keeps Check now disabled until it exits.
#[test]
fn changing_channels_keeps_pending_worker_cancellation_busy() {
    let mut state = State::new();
    state.startup = StartupPhase::Complete;
    state.running = true;
    state.status = Status::Downloading;
    let old = state.generation;
    state.configure(Preferences {
        channel: Channel::Nightly,
        ..state.preferences
    });
    assert!(state.status.is_busy());
    assert!(!state.request_check());
    assert!(state.finish_check(old, false, true));
    assert!(state.request_check());
}
