use super::*;
use crate::tests::{TEST_SEED, keys};
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
    let expires = OffsetDateTime::UNIX_EPOCH + time::Duration::days(1);
    let (beta, requested) = select_release(
        root.path(),
        Channel::Nightly,
        11,
        Ok(envelope(Channel::Beta, 12, expires)),
    );
    assert_eq!(requested, [Channel::Nightly, Channel::Beta]);
    assert_eq!(beta.manifest.channel, Channel::Beta.name());
    let (nightly, _) = select_release(
        root.path(),
        Channel::Nightly,
        13,
        Ok(envelope(Channel::Beta, 12, expires)),
    );
    assert_eq!(nightly.manifest.channel, Channel::Nightly.name());
    assert_eq!(nightly.manifest.build_number, 13);
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
fn an_unavailable_nightly_cache_falls_back_to_the_cached_beta() {
    let root = tempfile::tempdir().unwrap();
    let expires = OffsetDateTime::UNIX_EPOCH + time::Duration::days(1);
    let policy = check(Channel::Nightly);
    select_release(
        root.path(),
        Channel::Nightly,
        13,
        Ok(envelope(Channel::Beta, 12, expires)),
    );
    let selected = cached(root.path(), &keys(), policy, OffsetDateTime::UNIX_EPOCH).unwrap();
    assert_eq!(selected.manifest.channel, Channel::Nightly.name());
    assert_eq!(selected.manifest.build_number, 13);
    fs::remove_file(cache_path(root.path(), Channel::Nightly)).unwrap();
    let cached_beta = cached(root.path(), &keys(), policy, OffsetDateTime::UNIX_EPOCH).unwrap();
    assert_eq!(cached_beta.manifest.channel, Channel::Beta.name());
}

#[test]
fn cached_checks_use_both_authenticated_channels() {
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
}
