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
    staged_payload(root, platform, PAYLOAD)
}

/// Stages an arbitrary original payload through signature verification and the resumable writer.
pub fn staged_payload(root: &Path, platform: &str, payload: &[u8]) -> (PathBuf, TrustedKeys) {
    let (envelope, keys) = signed(platform, "2.0.0", payload);
    let selected =
        crate::stage::Selection::verify(&envelope, &keys, check(platform), now()).unwrap();
    let mut download = crate::stage::Download::begin(root, selected).unwrap();
    for chunk in payload.chunks(3) {
        download.write(chunk).unwrap();
    }
    (download.finish(&root.join("ready")).unwrap(), keys)
}

/// A tiny executable archive fixture with the Linux release's entry points and build query.
#[cfg(target_os = "linux")]
pub fn linux_payload() -> Vec<u8> {
    let entry = crate::target::APP_RUN;
    let client = crate::target::linux_executable(Path::new("."));
    let client = client.to_str().unwrap();
    let bin = Path::new(client).parent().unwrap().to_str().unwrap();
    format!(
        r#"#!/bin/sh
set -eu
[ "$1" = --appimage-extract ] || exit 1
mkdir -p 'squashfs-root/{bin}'
cat > 'squashfs-root/{entry}' <<'APP'
#!/bin/sh
exec "$(dirname "$0")/{client}" "$@"
APP
cat > 'squashfs-root/{client}' <<'CLIENT'
#!/bin/sh
printf '%s\n' '{{"version":"2.0.0","number":2}}'
CLIENT
chmod +x 'squashfs-root/{entry}' 'squashfs-root/{client}'
"#
    )
    .into_bytes()
}
