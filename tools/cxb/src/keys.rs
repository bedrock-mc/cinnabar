//! Raw 32-byte Ed25519 seeds, stored as one line of lowercase hex. There is no key container.

use std::{fs::OpenOptions, io::Write, path::Path};

use anyhow::{Context, Result};
use {
    ring::signature::{Ed25519KeyPair, KeyPair},
    server_experience::crypto,
};

/// Writes a fresh seed to a new file and returns its public key. An existing file is never
/// replaced, so a key in use cannot be lost by rerunning the command.
pub fn generate(path: &Path) -> Result<String> {
    let seed = crypto::challenge()?;
    let mut options = OpenOptions::new();
    options.write(true).create_new(true);
    #[cfg(unix)]
    {
        use std::os::unix::fs::OpenOptionsExt;
        options.mode(0o600);
    }
    let mut file = options
        .open(path)
        .with_context(|| format!("creating {}", path.display()))?;
    file.write_all(format!("{seed}\n").as_bytes())?;
    file.sync_all()?;
    Ok(public_key(&pair(&crypto::fixed_hex::<32>(&seed)?)?))
}

/// Loads a seed file; surrounding whitespace is ignored.
pub fn read(path: &Path) -> Result<Ed25519KeyPair> {
    let text =
        std::fs::read_to_string(path).with_context(|| format!("reading {}", path.display()))?;
    let seed = crypto::fixed_hex::<32>(text.trim())
        .with_context(|| format!("{} is not 64 lowercase hex digits", path.display()))?;
    pair(&seed)
}

/// Derives the signing key from a seed.
pub fn pair(seed: &[u8; 32]) -> Result<Ed25519KeyPair> {
    Ed25519KeyPair::from_seed_unchecked(seed)
        .map_err(|error| anyhow::anyhow!("invalid Ed25519 seed: {error}"))
}

/// The hex public key that offers and manifests name.
pub fn public_key(key: &Ed25519KeyPair) -> String {
    crypto::hex(key.public_key().as_ref())
}
