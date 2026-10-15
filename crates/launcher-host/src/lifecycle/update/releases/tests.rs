use super::*;
use crate::lifecycle::update::{Ready, State, tests::TEST_SEED, worker};
use sha2::{Digest, Sha256};

const PLATFORM: &str = "macos-arm64";
const PAYLOAD: &[u8] = b"authenticated update payload";

/// Describes a numbered client on the selected channel without downgrade consent.
fn check(channel: Channel) -> Check<'static> {
    Check {
        channel: channel.name(),
        platform: PLATFORM,
        current: "1.0.0-nightly",
        current_build: 10,
        switch_now: false,
    }
}

/// Supplies the same in-memory trust root for both publishing channels.
fn keys() -> TrustedKeys {
    TrustedKeys::from([(
        "test".into(),
        update_manifest::public_key(&TEST_SEED).unwrap(),
    )])
}

/// Signs a deterministic channel release with a real payload digest and caller-controlled expiry.
fn envelope(channel: Channel, build: u64, expires: OffsetDateTime) -> Vec<u8> {
    update_manifest::sign(
        &update_manifest::Manifest {
            schema: update_manifest::SCHEMA,
            channel: channel.name().into(),
            version: if channel == Channel::Nightly {
                "1.0.0-nightly"
            } else {
                "1.0.0"
            }
            .into(),
            build_number: build,
            expires,
            notes_url: String::new(),
            artifacts: [(
                PLATFORM.into(),
                update_manifest::Artifact {
                    url: "https://example.test/update.dmg".into(),
                    sha256: format!("{:x}", Sha256::digest(PAYLOAD)),
                    size: PAYLOAD.len() as i64,
                },
            )]
            .into(),
        },
        "test",
        &TEST_SEED,
    )
    .unwrap()
}

/// Runs the real selection and cache path against a bounded fake manifest transport.
fn select_release(
    root: &Path,
    channel: Channel,
    nightly_build: u64,
    beta: Result<Vec<u8>>,
) -> (Selection, Vec<Channel>) {
    let mut requested = Vec::new();
    let mut beta = Some(beta);
    let runtime = tokio::runtime::Builder::new_current_thread()
        .build()
        .unwrap();
    let selected = runtime
        .block_on(select(
            root,
            &keys(),
            check(channel),
            false,
            OffsetDateTime::UNIX_EPOCH,
            async |channel| {
                requested.push(channel);
                match channel {
                    Channel::Nightly => Ok(envelope(
                        channel,
                        nightly_build,
                        OffsetDateTime::UNIX_EPOCH + time::Duration::days(1),
                    )),
                    Channel::Beta => beta.take().expect("one beta request"),
                    Channel::Stable => panic!("stable was not selected"),
                }
            },
        ))
        .unwrap();
    (selected, requested)
}

#[test]
fn nightly_offers_a_newer_beta_and_returns_to_a_newer_nightly() {
    let root = tempfile::tempdir().unwrap();
    let policy = check(Channel::Nightly);
    let expires = OffsetDateTime::UNIX_EPOCH + time::Duration::days(1);
    let (beta, requested) = select_release(
        root.path(),
        Channel::Nightly,
        11,
        Ok(envelope(Channel::Beta, 12, expires)),
    );
    assert_eq!(requested, [Channel::Nightly, Channel::Beta]);
    assert_eq!(beta.manifest.channel, Channel::Beta.name());
    let mut download = update_installer::stage::Download::begin(root.path(), beta).unwrap();
    download.write(PAYLOAD).unwrap();
    let directory = download.finish(&root.path().join("ready")).unwrap();
    let lease = update_installer::lease::Lease::acquire(root.path()).unwrap();
    let selected =
        update_installer::stage::verify(&directory, &keys(), policy, OffsetDateTime::UNIX_EPOCH)
            .unwrap();
    let staged = selected.manifest.clone();
    let (stage, reader) = update_installer::stage::retain(&directory, &lease, &selected).unwrap();
    let mut state = State::new();
    state.configure(launcher::menu::updates::Preferences {
        channel: Channel::Nightly,
        ..state.preferences
    });
    worker::offer_switch(&mut state);
    state.offer(
        Ready::new(stage, reader, selected, false, true),
        state.generation,
    );
    assert_eq!(state.preferences.channel, Channel::Nightly);
    assert_eq!(state.ready.as_ref().unwrap().channel, Channel::Beta);
    assert!(!state.switch_available);
    let restored = worker::retained_manifest(
        state.ready.as_ref(),
        &keys(),
        policy,
        OffsetDateTime::UNIX_EPOCH,
    )
    .unwrap();
    assert_eq!(restored, staged);
    assert!(!worker::cached_switch_offer(
        root.path(),
        &keys(),
        policy,
        OffsetDateTime::UNIX_EPOCH
    ));

    let (nightly, _) = select_release(
        root.path(),
        Channel::Nightly,
        13,
        Ok(envelope(Channel::Beta, 12, expires)),
    );
    assert_eq!(nightly.manifest.channel, Channel::Nightly.name());
    assert!(worker::supersedes(&nightly.manifest, Some(&staged), false).unwrap());
    let mut download = update_installer::stage::Download::begin(root.path(), nightly).unwrap();
    download.write(PAYLOAD).unwrap();
    download.finish(&directory).unwrap();
    let selected =
        update_installer::stage::verify(&directory, &keys(), policy, OffsetDateTime::UNIX_EPOCH)
            .unwrap();
    let (stage, reader) = update_installer::stage::retain(&directory, &lease, &selected).unwrap();
    state.offer(
        Ready::new(stage, reader, selected, false, true),
        state.generation,
    );
    assert_eq!(state.ready.as_ref().unwrap().channel, Channel::Nightly);
    assert_eq!(state.preferences.channel, Channel::Nightly);
}

#[test]
fn optional_beta_failures_leave_nightly_available() {
    let expires = OffsetDateTime::UNIX_EPOCH + time::Duration::days(1);
    let mut tampered: update_manifest::Envelope =
        serde_json::from_slice(&envelope(Channel::Beta, 12, expires)).unwrap();
    tampered.signature = "AAAA".into();
    let foreign = update_manifest::sign(
        &update_manifest::verify(
            &envelope(Channel::Beta, 12, expires),
            &keys(),
            OffsetDateTime::UNIX_EPOCH,
        )
        .unwrap(),
        "test",
        &[9; update_manifest::KEY_BYTES],
    )
    .unwrap();
    for beta in [
        Err(anyhow::anyhow!("unreachable")),
        Ok(serde_json::to_vec(&tampered).unwrap()),
        Ok(foreign),
        Ok(envelope(Channel::Beta, 12, OffsetDateTime::UNIX_EPOCH)),
        Ok(envelope(Channel::Nightly, 12, expires)),
    ] {
        let root = tempfile::tempdir().unwrap();
        let (selected, requested) = select_release(root.path(), Channel::Nightly, 11, beta);
        assert_eq!(requested, [Channel::Nightly, Channel::Beta]);
        assert_eq!(selected.manifest.channel, Channel::Nightly.name());
        assert_eq!(selected.manifest.build_number, 11);
        assert!(!cache_path(root.path(), Channel::Beta).exists());
    }
}

#[test]
fn beta_selection_never_fetches_or_reads_nightly() {
    let root = tempfile::tempdir().unwrap();
    let expires = OffsetDateTime::UNIX_EPOCH + time::Duration::days(1);
    fs::write(
        cache_path(root.path(), Channel::Nightly),
        envelope(Channel::Nightly, 99, expires),
    )
    .unwrap();
    let (selected, requested) = select_release(
        root.path(),
        Channel::Beta,
        99,
        Ok(envelope(Channel::Beta, 11, expires)),
    );
    assert_eq!(requested, [Channel::Beta]);
    assert_eq!(selected.manifest.channel, Channel::Beta.name());
    let cached = cached(
        root.path(),
        &keys(),
        check(Channel::Beta),
        OffsetDateTime::UNIX_EPOCH,
    )
    .unwrap();
    assert_eq!(cached.manifest, selected.manifest);
}

#[test]
fn cached_winner_clears_a_stale_switch_offer_without_downgrading_a_staged_nightly() {
    let root = tempfile::tempdir().unwrap();
    let expires = OffsetDateTime::UNIX_EPOCH + time::Duration::days(1);
    let policy = check(Channel::Nightly);
    select_release(
        root.path(),
        Channel::Nightly,
        8,
        Ok(envelope(Channel::Beta, 9, expires)),
    );
    assert!(worker::cached_switch_offer(
        root.path(),
        &keys(),
        policy,
        OffsetDateTime::UNIX_EPOCH
    ));
    let mut state = State::new();
    worker::offer_switch(&mut state);
    let (staged, _) = select_release(
        root.path(),
        Channel::Nightly,
        13,
        Ok(envelope(Channel::Beta, 12, expires)),
    );
    let selected = cached(root.path(), &keys(), policy, OffsetDateTime::UNIX_EPOCH).unwrap();
    assert_eq!(selected.manifest.channel, Channel::Nightly.name());
    assert_eq!(selected.manifest.build_number, 13);
    worker::set_switch_offer(
        &mut state,
        worker::cached_switch_offer(root.path(), &keys(), policy, OffsetDateTime::UNIX_EPOCH),
    );
    assert!(!state.switch_available && state.message.is_none());
    let beta = verify_channel(
        &envelope(Channel::Beta, 12, expires),
        &keys(),
        check(Channel::Beta),
        OffsetDateTime::UNIX_EPOCH,
    )
    .unwrap();
    assert!(!worker::supersedes(&beta.manifest, Some(&staged.manifest), false).unwrap());
    // An unavailable nightly endpoint must not replace an already staged newer nightly with beta.
    fs::remove_file(cache_path(root.path(), Channel::Nightly)).unwrap();
    let cached_beta = cached(root.path(), &keys(), policy, OffsetDateTime::UNIX_EPOCH).unwrap();
    assert_eq!(cached_beta.manifest.channel, Channel::Beta.name());
    assert!(!worker::supersedes(&cached_beta.manifest, Some(&staged.manifest), false).unwrap());
}

#[test]
fn manual_switch_progress_survives_an_older_winning_release() {
    let expires = OffsetDateTime::UNIX_EPOCH + time::Duration::days(1);
    for (channel, nightly_build, beta_build) in [
        (Channel::Nightly, 8, 9),
        (Channel::Nightly, 9, 8),
        (Channel::Beta, 8, 9),
    ] {
        for failed in [false, true] {
            let root = tempfile::tempdir().unwrap();
            select_release(
                root.path(),
                channel,
                nightly_build,
                Ok(envelope(Channel::Beta, beta_build, expires)),
            );
            let mut state = State::new();
            state.configure(launcher::menu::updates::Preferences {
                channel,
                ..state.preferences
            });
            worker::set_switch_offer(&mut state, true);
            assert!(state.request_switch());
            state.running = true;
            let generation = state.generation;
            let progress = state.message.clone();
            let available = worker::cached_switch_offer(
                root.path(),
                &keys(),
                Check {
                    switch_now: state.switch_now,
                    ..check(channel)
                },
                OffsetDateTime::UNIX_EPOCH,
            );
            assert!(available);
            worker::set_switch_offer(&mut state, available);
            assert!(!state.switch_available);
            assert_eq!(state.message, progress);
            assert!(!state.request_switch());
            assert_eq!(state.generation, generation);
            assert!(state.accepts_worker(generation));
            assert!(!state.finish_check(generation, true, failed));
            assert!(!state.switch_now && !state.running);
            assert_eq!(state.switch_available, failed);
            assert_eq!(state.message.is_some(), failed);
        }
    }
}

#[test]
fn forced_checks_use_both_caches_and_stamps_stay_with_the_nightly_preference() {
    let root = tempfile::tempdir().unwrap();
    let expires = OffsetDateTime::UNIX_EPOCH + time::Duration::days(1);
    select_release(
        root.path(),
        Channel::Nightly,
        11,
        Ok(envelope(Channel::Beta, 12, expires)),
    );
    let runtime = tokio::runtime::Builder::new_current_thread()
        .build()
        .unwrap();
    let policy = check(Channel::Nightly);
    let selected = runtime
        .block_on(select(
            root.path(),
            &keys(),
            policy,
            true,
            OffsetDateTime::UNIX_EPOCH,
            async |_| panic!("both authenticated candidates are cached"),
        ))
        .unwrap();
    assert_eq!(selected.manifest.channel, Channel::Beta.name());
    let stamp_check = Check {
        current: env!("CARGO_PKG_VERSION"),
        current_build: update_manifest::build_number(),
        ..policy
    };
    worker::record(
        &worker::stamp_path(root.path(), Channel::Nightly),
        stamp_check,
        100,
    )
    .unwrap();
    assert_eq!(worker::last_check(root.path(), Channel::Nightly), Some(100));
    assert_eq!(worker::last_check(root.path(), Channel::Beta), None);
}
