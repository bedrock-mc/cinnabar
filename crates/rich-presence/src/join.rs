//! Discord join invites: the secret Discord hands to a joining friend and the party it groups.

use sha2::{Digest, Sha256};

/// Versions the secret so a future format can't be misread as an address.
const SECRET_PREFIX: &str = "cinnabar1:";
/// Discord rejects longer join secrets.
const MAX_SECRET_BYTES: usize = 128;

/// The join secret naming `address`, or `None` when it can't be carried.
pub(crate) fn secret(address: &str) -> Option<String> {
    let secret = format!("{SECRET_PREFIX}{address}");
    (secret.len() <= MAX_SECRET_BYTES && plausible(address)).then_some(secret)
}

/// The menu address a received join secret names; anything Cinnabar didn't publish is `None`.
pub fn join_address(secret: &str) -> Option<&str> {
    let address = secret.strip_prefix(SECRET_PREFIX)?;
    (secret.len() <= MAX_SECRET_BYTES && plausible(address)).then_some(address)
}

fn plausible(address: &str) -> bool {
    !address.is_empty() && address.bytes().all(|byte| byte.is_ascii_graphic())
}

/// Groups everyone on one destination without publishing its address.
pub(crate) fn party_id(address: &str) -> String {
    Sha256::digest(address.as_bytes())[..16]
        .iter()
        .map(|byte| format!("{byte:02x}"))
        .collect()
}
