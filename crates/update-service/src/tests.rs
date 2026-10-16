//! Signing fixtures shared by the crate's tests.

use update_manifest::TrustedKeys;

pub(crate) const TEST_SEED: [u8; update_manifest::KEY_BYTES] = [7; update_manifest::KEY_BYTES];

/// The in-memory trust root for fixtures signed with [`TEST_SEED`].
pub(crate) fn keys() -> TrustedKeys {
    TrustedKeys::from([(
        "test".into(),
        update_manifest::public_key(&TEST_SEED).unwrap(),
    )])
}
