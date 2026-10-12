//! Deterministic signed payloads for offline updater tests.
use sha2::{Digest, Sha256};
use std::{
    collections::BTreeMap,
    path::{Path, PathBuf},
};
use time::OffsetDateTime;
use update_manifest::{Artifact, Manifest, TrustedKeys, channel::Channel, select::Check};

pub const SEED: [u8; update_manifest::KEY_BYTES] = [7; update_manifest::KEY_BYTES];
pub const CURRENT: &str = "1.0.0";
pub const PAYLOAD: &[u8] = b"a verified release payload";

/// Fixed clock used by every fixture, independent of the machine date.
pub fn now() -> OffsetDateTime {
    OffsetDateTime::UNIX_EPOCH
}

/// The running fixture's channel and build, with no downgrade permission.
pub fn check(platform: &str) -> Check<'_> {
    Check {
        channel: Channel::Beta.name(),
        platform,
        current: CURRENT,
        current_build: 1,
        switch_now: false,
    }
}

/// Builds a release signed only by an in-memory test seed.
pub fn signed(platform: &str, version: &str, payload: &[u8]) -> (Vec<u8>, TrustedKeys) {
    let keys = BTreeMap::from([("test".into(), update_manifest::public_key(&SEED).unwrap())]);
    let manifest = Manifest {
        schema: update_manifest::SCHEMA,
        channel: Channel::Beta.name().into(),
        version: version.into(),
        build_number: 2,
        expires: now() + time::Duration::days(1),
        notes_url: String::new(),
        artifacts: BTreeMap::from([(
            platform.into(),
            Artifact {
                url: "https://example.invalid/release".into(),
                sha256: format!("{:x}", Sha256::digest(payload)),
                size: payload.len() as i64,
            },
        )]),
    };
    (
        update_manifest::sign(&manifest, "test", &SEED).unwrap(),
        keys,
    )
}

/// Stages fixture chunks through the same resumable writer as the background HTTP worker.
pub fn staged(root: &Path, platform: &str) -> (PathBuf, TrustedKeys) {
    let (envelope, keys) = signed(platform, "2.0.0", PAYLOAD);
    let selected =
        crate::stage::Selection::verify(&envelope, &keys, check(platform), now()).unwrap();
    let mut download = crate::stage::Download::begin(root, selected).unwrap();
    for chunk in PAYLOAD.chunks(3) {
        download.write(chunk).unwrap();
    }
    (download.finish(&root.join("ready")).unwrap(), keys)
}
